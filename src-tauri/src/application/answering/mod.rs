//! AskQuestion: scope → hybrid retrieval → evidence bundle → local/cloud generation →
//! structural citation validation → persisted answer with openable sources.
pub mod prompt;

use std::time::{Duration, Instant};

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::generation::{GenerationRunner, Task};
use crate::application::jobs::{JobControl, Signal};
use crate::application::models;
use crate::application::ports::{AppEvent, ChatRequest, NewAttempt};
use crate::application::retrieval::{self, Hit, ResolvedScope};
use crate::application::views::{AskStarted, ConversationSummary, ConversationView, EvidenceLocation, EvidenceTarget, Rect, RetrievalMode, ScopeInput};
use crate::application::{settings, Services};
use crate::domain::evidence::{parse_model_json, validate_answer, AnswerStatus, Evidence, PROMPT_VERSION};
use crate::domain::ids::DocumentId;
use crate::domain::onboarding::{StepKey, StepState, FLOW_VERSION};

pub const MAX_QUESTION_CHARS: usize = 4000;
const CONTEXT_PASSAGES: usize = 8;
const MAX_PASSAGES_WITH_COVERAGE: usize = 10;
const MAX_PASSAGE_CHARS: usize = 3000;
const CONTEXT_BUDGET_CHARS: usize = 24_000;
const COVERAGE_DOC_LIMIT: usize = 12;
const MAX_VIEWED_PAGE_CHUNKS: usize = 3;

fn title_from_question(q: &str) -> String {
    let one_line = q.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= 80 {
        one_line
    } else {
        format!("{}…", one_line.chars().take(78).collect::<String>())
    }
}

pub fn ask(
    svc: &Services,
    runner: &GenerationRunner,
    request_id: &str,
    conversation_id: Option<&str>,
    question: &str,
    scope: &ScopeInput,
) -> AppResult<AskStarted> {
    // Double submit with the same request id returns the original attempt.
    if let Some((attempt, conversation)) = svc.answers.find_attempt_by_request(request_id)? {
        let view = svc.answers.conversation(&conversation)?.ok_or_else(|| AppError::not_found("conversation"))?;
        let message = view.turns.iter().find(|t| t.attempt.as_ref().is_some_and(|a| a.id == attempt)).map(|t| t.message_id.clone()).unwrap_or_default();
        return Ok(AskStarted { conversation_id: conversation, message_id: message, attempt_id: attempt });
    }
    let q = question.trim();
    if q.is_empty() || q.chars().count() > MAX_QUESTION_CHARS {
        return Err(AppError::validation("question length"));
    }
    let now = svc.now();
    let conversation = match conversation_id {
        Some(c) if svc.answers.conversation_exists(c)? => c.to_string(),
        Some(_) => return Err(AppError::not_found("conversation")),
        None => {
            let id = uuid::Uuid::now_v7().to_string();
            svc.answers.create_conversation(&id, &title_from_question(q), scope.collection_id.as_deref(), now)?;
            id
        }
    };
    let message_id = uuid::Uuid::now_v7().to_string();
    svc.answers.add_message(&message_id, &conversation, "user", q, "asked", now)?;
    let attempt_id = start_attempt(svc, runner, request_id, &message_id, scope)?;
    Ok(AskStarted { conversation_id: conversation, message_id, attempt_id })
}

fn start_attempt(svc: &Services, runner: &GenerationRunner, request_id: &str, message_id: &str, scope: &ScopeInput) -> AppResult<String> {
    let model = models::generation_choice(svc)?;
    let attempt_id = uuid::Uuid::now_v7().to_string();
    svc.answers.create_attempt(&NewAttempt {
        id: attempt_id.clone(),
        message_id: message_id.into(),
        request_id: request_id.into(),
        model_fingerprint: model.as_ref().map(|m| format!("{}|{}", m.provider_id, m.model)).unwrap_or_else(|| "none".into()),
        prompt_version: PROMPT_VERSION,
        scope_snapshot: serde_json::to_string(scope).unwrap(),
        retrieval_config: serde_json::json!({
            "mode": "hybrid", "candidates_per_path": crate::domain::retrieval::CANDIDATES_PER_PATH,
            "rrf_k": crate::domain::retrieval::RRF_K, "context_passages": CONTEXT_PASSAGES,
            "max_per_document": crate::domain::retrieval::MAX_PER_DOCUMENT
        })
        .to_string(),
        now: svc.now(),
    })?;
    runner.submit(Task::Answer(attempt_id.clone()));
    Ok(attempt_id)
}

/// New attempt for the same question (after a failure, cancel or model setup).
pub fn retry(svc: &Services, runner: &GenerationRunner, request_id: &str, message_id: &str, scope: Option<&ScopeInput>) -> AppResult<String> {
    if let Some((attempt, _)) = svc.answers.find_attempt_by_request(request_id)? {
        return Ok(attempt);
    }
    let (conversation, _) = svc.answers.message_question(message_id)?.ok_or_else(|| AppError::not_found("message"))?;
    let view = svc.answers.conversation(&conversation)?.ok_or_else(|| AppError::not_found("conversation"))?;
    let previous_scope = view
        .turns
        .iter()
        .find(|t| t.message_id == message_id)
        .and_then(|t| t.attempt.as_ref())
        .map(|a| a.id.clone());
    let scope = match (scope, previous_scope) {
        (Some(s), _) => s.clone(),
        (None, Some(a)) => svc.answers.attempt_scope(&a)?.unwrap_or_default(),
        (None, None) => ScopeInput::default(),
    };
    start_attempt(svc, runner, request_id, message_id, &scope)
}

fn finish(svc: &Services, attempt: &str, conversation: &str, state: &str, result: Option<&crate::domain::evidence::ValidatedAnswer>, error: Option<&str>, timing: &serde_json::Value) -> AppResult<()> {
    svc.answers.finish_attempt(attempt, state, result, error, &timing.to_string(), svc.now())?;
    svc.events.emit(AppEvent::GenerationFinished { attempt_id: attempt.into(), conversation_id: conversation.into(), state: state.into() });
    Ok(())
}

fn to_evidence(scope: &ResolvedScope, hits: &[Hit]) -> Vec<Evidence> {
    let mut used = 0usize;
    let mut out = Vec::new();
    for h in hits {
        let text: String = h.chunk.text.chars().take(MAX_PASSAGE_CHARS).collect();
        if used + text.len() > CONTEXT_BUDGET_CHARS && !out.is_empty() {
            break;
        }
        used += text.len();
        let doc = scope.doc(&h.chunk.document_id);
        out.push(Evidence {
            id: format!("E{}", out.len() + 1),
            chunk_id: h.chunk.id,
            document_id: h.chunk.document_id.clone(),
            generation_id: h.chunk.generation_id.clone(),
            title: doc.map(|d| d.title.clone()).unwrap_or_default(),
            authors: doc.and_then(|d| d.authors.clone()),
            year: doc.and_then(|d| d.year),
            page_index: h.chunk.page_index,
            page_offset: h.chunk.start,
            text,
        });
    }
    out
}

/// Words that point at the page on screen ("this page", "هذه الصفحة", "هنا").
fn refers_to_page(question: &str) -> bool {
    let q = crate::domain::text::normalize_for_search(question);
    ["صفح", "هنا", "page", "here"].iter().any(|w| q.contains(w))
}

/// The page open in the reader joins the passages (first when the question points at it), so
/// questions about what the user is looking at have evidence even without matching keywords.
fn add_viewed_page(svc: &Services, scope: &ResolvedScope, input: &ScopeInput, question: &str, hits: &mut Vec<Hit>) -> AppResult<()> {
    let Some(v) = &input.viewing else { return Ok(()) };
    let Some(doc) = DocumentId::parse(&v.document_id).and_then(|id| scope.doc(&id).cloned()) else { return Ok(()) };
    let Some(generation) = doc.generation_id.as_deref() else { return Ok(()) };
    let page: Vec<Hit> = svc
        .index
        .chunks_for_page(generation, v.page_index)?
        .into_iter()
        .filter(|c| !hits.iter().any(|h| h.chunk.id == c.id))
        .take(MAX_VIEWED_PAGE_CHUNKS)
        .map(|chunk| Hit { chunk, score: 0.0, lexical_rank: None, semantic_rank: None })
        .collect();
    if page.is_empty() {
        return Ok(());
    }
    if refers_to_page(question) || hits.is_empty() {
        hits.splice(0..0, page);
    } else {
        hits.extend(page);
    }
    hits.truncate(MAX_PASSAGES_WITH_COVERAGE);
    Ok(())
}

/// Best passages overall, then the best passage of each paper not yet represented so the
/// answer can speak to coverage across the scope.
fn select_passages(svc: &Services, scope: &ResolvedScope, input: &ScopeInput, question: &str, qvec: Option<&[f32]>) -> AppResult<Vec<Hit>> {
    let mut hits = retrieval::search_in(svc, scope, question, qvec, None, CONTEXT_PASSAGES)?;
    add_viewed_page(svc, scope, input, question, &mut hits)?;
    if scope.lexical.len() > 1 && scope.lexical.len() <= COVERAGE_DOC_LIMIT {
        for d in &scope.lexical {
            if hits.len() >= MAX_PASSAGES_WITH_COVERAGE {
                break;
            }
            if hits.iter().any(|h| h.chunk.document_id == d.id) {
                continue;
            }
            if let Some(best) = retrieval::search_in(svc, scope, question, qvec, Some(std::slice::from_ref(&d.id)), 1)?.into_iter().next() {
                hits.push(best);
            }
        }
    }
    Ok(hits)
}

pub fn run_attempt(svc: &Services, attempt_id: &str, control: &JobControl) -> AppResult<()> {
    let started = Instant::now();
    let (message_id, scope_input) = svc.answers.attempt_inputs(attempt_id)?.ok_or_else(|| AppError::not_found("attempt"))?;
    let (conversation, question) = svc.answers.message_question(&message_id)?.ok_or_else(|| AppError::not_found("message"))?;
    let mut timing = serde_json::json!({});
    let progress = |state: &str, chars: u32| {
        svc.events.emit(AppEvent::GenerationProgress { attempt_id: attempt_id.into(), conversation_id: conversation.clone(), state: state.into(), chars });
    };
    progress("retrieving", 0);

    // Model setup is checked first so the saved question can be retried after setup.
    let Some(choice) = models::generation_choice(svc)? else {
        return finish(svc, attempt_id, &conversation, "failed", None, Some("no_generation_model"), &timing);
    };
    let (record, client) = match models::open(svc, &choice.provider_id) {
        Ok(v) => v,
        Err(e) => {
            let kind = if e.code == ErrorCode::NotFound { "no_generation_model" } else { "provider_unavailable" };
            return finish(svc, attempt_id, &conversation, "failed", None, Some(kind), &timing);
        }
    };
    if models::ensure_allowed(svc, &record).is_err() {
        return finish(svc, attempt_id, &conversation, "failed", None, Some("not_allowed"), &timing);
    }

    let scope = retrieval::resolve_scope(svc, &scope_input, RetrievalMode::Hybrid)?;
    svc.answers.set_attempt_scope(attempt_id, &scope.summary)?;
    if scope.lexical.is_empty() {
        return finish(svc, attempt_id, &conversation, "no_evidence", None, Some("empty_scope"), &timing);
    }
    let qvec = match retrieval::embed_query(&scope, &question) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("query embedding failed, keyword search only: {}", e.detail);
            None
        }
    };
    let hits = select_passages(svc, &scope, &scope_input, &question, qvec.as_deref())?;
    timing["retrieval_ms"] = (started.elapsed().as_millis() as u64).into();
    let bundle = to_evidence(&scope, &hits);
    svc.answers.save_evidence(attempt_id, &bundle)?;
    if bundle.is_empty() {
        return finish(svc, attempt_id, &conversation, "no_evidence", None, None, &timing);
    }
    if control.signal() == Signal::Cancel {
        return finish(svc, attempt_id, &conversation, "cancelled", None, None, &timing);
    }

    svc.answers.set_attempt_state(attempt_id, "generating")?;
    progress("generating", 0);
    let language = prompt::answer_language(&settings::get_settings(svc)?.answer_locale, &question);
    let req = ChatRequest {
        system: prompt::answer_system(language),
        user: prompt::answer_user(&question, &bundle, scope.summary.searched),
        json: true,
        max_output_tokens: 1800,
    };
    let gen_started = Instant::now();
    let mut chars = 0u32;
    let mut last = Instant::now();
    let outcome = client.chat(&choice.model, &req, control, &mut |delta| {
        chars += delta.chars().count() as u32;
        if last.elapsed() > Duration::from_millis(250) {
            last = Instant::now();
            progress("generating", chars);
        }
    });
    timing["generation_ms"] = (gen_started.elapsed().as_millis() as u64).into();
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("generation failed: {}", e.detail);
            let kind = if e.detail.contains("timed out") || e.detail.contains("timeout") { "model_timeout" } else { "model_error" };
            return finish(svc, attempt_id, &conversation, "failed", None, Some(kind), &timing);
        }
    };
    if outcome.cancelled || control.signal() == Signal::Cancel {
        return finish(svc, attempt_id, &conversation, "cancelled", None, None, &timing);
    }
    if let Some(ms) = outcome.load_ms {
        timing["model_load_ms"] = ms.into();
    }

    svc.answers.set_attempt_state(attempt_id, "validating")?;
    progress("validating", chars);
    let raw = match parse_model_json(&outcome.text) {
        Ok(r) => r,
        Err(first_error) => {
            // One bounded repair request.
            tracing::info!("answer JSON invalid ({first_error}); asking once for a repair");
            let repair = ChatRequest {
                system: req.system.clone(),
                user: format!("{}\n\nYour previous reply:\n{}\n\n{}", req.user, outcome.text.chars().take(6000).collect::<String>(), prompt::REPAIR),
                json: true,
                max_output_tokens: 1800,
            };
            match client.chat(&choice.model, &repair, control, &mut |_| {}).map_err(|e| e.detail).and_then(|o| parse_model_json(&o.text)) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("answer still invalid after repair: {e}");
                    return finish(svc, attempt_id, &conversation, "failed", None, Some("invalid_output"), &timing);
                }
            }
        }
    };
    // Sources deleted while generating cannot be cited.
    let live: Vec<Evidence> = bundle
        .iter()
        .filter(|e| svc.documents.get(&e.document_id).ok().flatten().is_some_and(|d| d.deleted_at.is_none()))
        .cloned()
        .collect();
    let mut answer = validate_answer(&raw, &live);
    svc.answers.save_citations(attempt_id, &mut answer)?;
    let state = match answer.status {
        AnswerStatus::Answered => "answered",
        AnswerStatus::Partial => "partial",
        AnswerStatus::NoEvidence => "no_evidence",
    };
    timing["total_ms"] = (started.elapsed().as_millis() as u64).into();
    finish(svc, attempt_id, &conversation, state, Some(&answer), None, &timing)?;
    if answer.status != AnswerStatus::NoEvidence {
        svc.onboarding.set(FLOW_VERSION, StepKey::FirstQuestion, StepState::Completed, Some(attempt_id), svc.now())?;
    }
    Ok(())
}

pub fn cancel(runner: &GenerationRunner, attempt_id: &str) -> bool {
    runner.cancel(attempt_id)
}

pub fn conversation(svc: &Services, id: &str) -> AppResult<ConversationView> {
    svc.answers.conversation(id)?.ok_or_else(|| AppError::not_found("conversation"))
}

pub fn conversations(svc: &Services) -> AppResult<Vec<ConversationSummary>> {
    svc.answers.list_conversations(100)
}

/// Turns a char span of a page into the PDF boxes of the text segments it covers.
fn boxes_for_span(svc: &Services, doc: &DocumentId, page_index: u32, span: (u32, u32)) -> AppResult<Vec<Rect>> {
    let spans = svc.index.page_spans(doc, page_index)?;
    Ok(spans
        .iter()
        .filter(|s| s.start < span.1 && s.end > span.0)
        .map(|s| Rect { left: s.left, bottom: s.bottom, right: s.right, top: s.top })
        .collect())
}

/// Resolves a citation (or comparison cell source, or search hit) to its page and highlight.
pub fn open_evidence(svc: &Services, target: &EvidenceTarget) -> AppResult<EvidenceLocation> {
    let (doc, page, span, quote, excerpt, record_open) = match target {
        EvidenceTarget::Citation { citation_id } => {
            let c = svc.answers.citation(citation_id)?.ok_or_else(|| AppError::not_found("citation"))?;
            (c.document_id, c.page_index, c.span, c.quote, c.excerpt, true)
        }
        EvidenceTarget::Cell { cell_id, number } => {
            let (c, excerpt) = svc.answers.cell_citation(cell_id, *number)?.ok_or_else(|| AppError::not_found("citation"))?;
            (c.document_id, c.page_index, c.span, c.quote, excerpt, false)
        }
        EvidenceTarget::Hit { chunk_id } => {
            let c = svc.index.chunks_by_ids(&[*chunk_id])?.into_iter().next().ok_or_else(|| AppError::not_found("passage"))?;
            (c.document_id, c.page_index, Some((c.start, c.end)), None, c.text, false)
        }
    };
    let record = svc.documents.get(&doc)?;
    let deleted = record.as_ref().is_none_or(|d| d.deleted_at.is_some());
    let title = svc.documents.view(&doc)?.map(|v| v.title).unwrap_or_default();
    let boxes = match (deleted, span) {
        (false, Some(s)) => boxes_for_span(svc, &doc, page, s)?,
        _ => Vec::new(),
    };
    if record_open && !deleted {
        // The guide's "open a source" step completes on a real citation being opened.
        svc.onboarding.set(FLOW_VERSION, StepKey::OpenSource, StepState::Completed, None, svc.now())?;
    }
    Ok(EvidenceLocation { document_id: doc, title, page_index: page, quote, excerpt, boxes, deleted })
}
