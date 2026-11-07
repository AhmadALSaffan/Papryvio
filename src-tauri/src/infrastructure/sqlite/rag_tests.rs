//! End-to-end tests of indexing, retrieval, answering with citations and comparison,
//! using real SQLite/FTS5 and deterministic model doubles (no network).
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::application::error::AppResult;
use crate::application::generation::GenerationRunner;
use crate::application::indexing;
use crate::application::jobs::JobControl;
use crate::application::ports::*;
use crate::application::views::{ComparisonRow, EvidenceTarget, RetrievalMode, ScopeInput};
use crate::application::{answering, comparison, library, models, retrieval};
use crate::domain::document::ExtractionState;
use crate::domain::ids::DocumentId;
use crate::domain::job::{JobKind, JobState};

use super::tests::{harness, idle_scheduler, import_one, run_job, FakePdf, Harness};

#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);
impl SecretStore for MemorySecrets {
    fn set(&self, k: &str, v: &str) -> AppResult<()> {
        self.0.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    fn get(&self, k: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(k).cloned())
    }
    fn delete(&self, k: &str) -> AppResult<()> {
        self.0.lock().unwrap().remove(k);
        Ok(())
    }
}

pub struct NoSignIn;
impl SignInService for NoSignIn {
    fn start(&self, _k: ProviderKind) -> AppResult<SignInStart> {
        Err(crate::application::error::AppError::validation("not in tests"))
    }
    fn poll(&self, _f: &str) -> AppResult<SignInPoll> {
        Ok(SignInPoll::Expired)
    }
    fn cancel(&self, _f: &str) {}
}

pub struct FakeFactory;
impl ProviderFactory for FakeFactory {
    fn open(&self, _r: &ProviderRecord) -> AppResult<Arc<dyn ModelProvider>> {
        Ok(Arc::new(FakeModel))
    }
}

/// Embeddings: hashed bag of words. Chat: cites E1 with an exact quote from it, plus one
/// claim citing a passage that was never retrieved (must be dropped by the validator).
pub struct FakeModel;

fn bow(text: &str) -> Vec<f32> {
    let mut v = vec![0.001f32; 64];
    for w in text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2) {
        let h = w.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
        v[(h % 64) as usize] += 1.0;
    }
    v
}

impl ModelProvider for FakeModel {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }
    fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        Ok(vec![
            ModelInfo { id: "fake-chat".into(), label: "fake-chat".into(), digest: Some("sha256:c".into()), size_bytes: None, can_generate: true, can_embed: false },
            ModelInfo { id: "fake-embed".into(), label: "fake-embed".into(), digest: Some("sha256:e".into()), size_bytes: None, can_generate: false, can_embed: true },
        ])
    }
    fn supports_embeddings(&self) -> bool {
        true
    }
    fn embed(&self, _m: &str, inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
        Ok(inputs.iter().map(|t| bow(t)).collect())
    }
    fn chat(&self, _m: &str, req: &ChatRequest, _c: &JobControl, on_delta: &mut dyn FnMut(&str)) -> AppResult<ChatOutcome> {
        if !req.json {
            on_delta("OK");
            return Ok(ChatOutcome { text: "OK".into(), ..Default::default() });
        }
        let body = req.user.split("<evidence id=\"E1\"").nth(1).and_then(|s| s.split_once(">\n")).map(|(_, rest)| rest).unwrap_or("");
        let quote: String = body.split_whitespace().take(8).collect::<Vec<_>>().join(" ");
        let reply = if req.system.contains("comparison table") {
            serde_json::json!({ "status": "supported", "text": "Uses hybrid retrieval.", "evidence_ids": ["E1"], "quote": quote })
        } else {
            serde_json::json!({
                "answer_status": "answered",
                "claims": [
                    { "text": "The paper evaluates evidence.", "kind": "stated", "evidence_ids": ["E1"], "quote": quote },
                    { "text": "Invented claim.", "kind": "stated", "evidence_ids": ["E99"] }
                ],
                "limitations": ""
            })
        }
        .to_string();
        on_delta(&reply);
        Ok(ChatOutcome { text: reply, ..Default::default() })
    }
}

fn pages(topic: &str, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("Section {i}. This study of {topic} measures recall at k = 20 and reports that hybrid retrieval combined lexical and dense signals on page {i}. The evaluation uses a held-out benchmark and three random seeds."))
        .collect()
}

fn ready_doc(h: &Harness, sched: &crate::application::jobs::JobScheduler, coll: &crate::domain::ids::CollectionId, name: &str) -> DocumentId {
    let doc = import_one(h, sched, coll, name, name);
    assert_eq!(run_job(h, &doc), JobState::Completed);
    run_kind(h, &doc, JobKind::Index);
    doc
}

fn run_kind(h: &Harness, doc: &DocumentId, kind: JobKind) -> JobState {
    let job = match h.svc.jobs.latest_for_entity(doc.as_str(), kind).unwrap() {
        Some(j) if j.state.is_active() => j,
        _ => {
            let id = models::enqueue(&h.svc, &idle_scheduler(h), kind, doc.as_str()).unwrap();
            h.svc.jobs.get(&id).unwrap().unwrap()
        }
    };
    let mut job = job;
    job.state = JobState::Running;
    h.svc.jobs.save(&job).unwrap();
    let control = JobControl::new();
    match kind {
        JobKind::Index => indexing::run_index_job(&h.svc, &mut job, &control).unwrap(),
        JobKind::Embed => indexing::run_embed_job(&h.svc, &mut job, &control).unwrap(),
        _ => unreachable!(),
    }
    job.state
}

fn setup() -> (Harness, Arc<crate::application::jobs::JobScheduler>, crate::domain::ids::CollectionId) {
    let h = harness(FakePdf { pages: pages("retrieval", 3), title: Some("Hybrid Retrieval Study".into()), pause_after: Default::default(), fail_with: None });
    let sched = idle_scheduler(&h);
    let c = library::create_collection(&h.svc, "c1", "Review").unwrap();
    (h, sched, c.id)
}

#[test]
fn indexing_publishes_keyword_search() {
    let (h, sched, coll) = setup();
    let doc = ready_doc(&h, &sched, &coll, "a.pdf");
    let v = library::get_document(&h.svc, &doc).unwrap();
    assert_eq!(v.state, ExtractionState::Ready);
    assert!(v.lexical_ready && !v.semantic_ready);
    assert!(h.svc.index.fts_consistent().unwrap());

    let scope = ScopeInput { collection_id: Some(coll.to_string()), document_ids: None, viewing: None };
    let r = retrieval::search(&h.svc, "hybrid recall benchmark", &scope, RetrievalMode::Hybrid, 10).unwrap();
    assert_eq!(r.scope.mode, RetrievalMode::Lexical, "no embedding model yet");
    assert_eq!(r.scope.searched, 1);
    assert!(!r.hits.is_empty());
    assert!(r.hits[0].lexical_rank.is_some());
    // FTS operators in user input are inert.
    assert!(retrieval::search(&h.svc, "NEAR(\"x\" OR) * -", &scope, RetrievalMode::Lexical, 10).is_ok());

    // Re-indexing replaces the generation without leaving stale FTS rows.
    run_kind(&h, &doc, JobKind::Index);
    assert!(h.svc.index.fts_consistent().unwrap());
}

#[test]
fn embeddings_enable_semantic_and_hybrid_search_and_reset_on_model_change() {
    let (h, sched, coll) = setup();
    let doc = ready_doc(&h, &sched, &coll, "a.pdf");
    let p = models::add_ollama(&h.svc, "http://127.0.0.1:11434").unwrap();
    models::save_embedding(&h.svc, &sched, &p.id, "fake-embed").unwrap();
    assert_eq!(run_kind(&h, &doc, JobKind::Embed), JobState::Completed);
    let v = library::get_document(&h.svc, &doc).unwrap();
    assert!(v.semantic_ready);

    let scope = ScopeInput::default();
    let r = retrieval::search(&h.svc, "held-out benchmark seeds", &scope, RetrievalMode::Hybrid, 10).unwrap();
    assert_eq!(r.scope.mode, RetrievalMode::Hybrid);
    assert!(r.hits.iter().any(|x| x.semantic_rank.is_some()));
    let s = retrieval::search(&h.svc, "benchmark", &scope, RetrievalMode::Semantic, 10).unwrap();
    assert_eq!(s.scope.mode, RetrievalMode::Semantic);

    // A different embedding model invalidates vectors.
    h.svc.settings.set("model.embedding", "", 0).unwrap();
    models::clear_embedding(&h.svc).unwrap();
    assert!(!library::get_document(&h.svc, &doc).unwrap().semantic_ready);
}

#[test]
fn ask_answers_with_validated_citations_and_opens_source() {
    let (h, sched, coll) = setup();
    let doc = ready_doc(&h, &sched, &coll, "a.pdf");
    // A paper still processing is reported as not searched.
    let _pending = import_one(&h, &sched, &coll, "b.pdf", "pending");
    let p = models::add_ollama(&h.svc, "http://127.0.0.1:11434").unwrap();
    models::save_generation(&h.svc, &p.id, "fake-chat").unwrap();

    let runner = GenerationRunner::start(Arc::clone(&h.svc));
    runner.shutdown(); // drive the attempt by hand
    let scope = ScopeInput { collection_id: Some(coll.to_string()), document_ids: None, viewing: None };
    let started = answering::ask(&h.svc, &runner, "req-1", None, "What does the study measure?", &scope).unwrap();
    // Same request id → same attempt (double click).
    let again = answering::ask(&h.svc, &runner, "req-1", None, "What does the study measure?", &scope).unwrap();
    assert_eq!(started.attempt_id, again.attempt_id);

    answering::run_attempt(&h.svc, &started.attempt_id, &JobControl::new()).unwrap();
    let conv = answering::conversation(&h.svc, &started.conversation_id).unwrap();
    let attempt = conv.turns[0].attempt.as_ref().unwrap();
    assert_eq!(attempt.state, "partial", "one invented claim was dropped");
    let answer = attempt.answer.as_ref().unwrap();
    assert_eq!(answer.claims.len(), 1);
    assert_eq!(answer.dropped_claims, 1);
    let scope_summary = attempt.scope.as_ref().unwrap();
    assert_eq!((scope_summary.requested, scope_summary.searched), (2, 1));
    assert_eq!(scope_summary.not_searched[0].reason, "processing");

    let cite = &answer.claims[0].citations[0];
    assert_eq!(cite.document_id, doc);
    assert!(cite.span.is_some(), "quote located on the page");
    let loc = answering::open_evidence(&h.svc, &EvidenceTarget::Citation { citation_id: cite.citation_id.clone().unwrap() }).unwrap();
    assert_eq!(loc.document_id, doc);
    assert!(!loc.boxes.is_empty(), "highlight boxes from extracted spans");
    assert!(!loc.deleted);
}

#[test]
fn ask_without_model_keeps_question_for_retry() {
    let (h, sched, coll) = setup();
    ready_doc(&h, &sched, &coll, "a.pdf");
    let runner = GenerationRunner::start(Arc::clone(&h.svc));
    runner.shutdown();
    let s = answering::ask(&h.svc, &runner, "r1", None, "Anything?", &ScopeInput::default()).unwrap();
    answering::run_attempt(&h.svc, &s.attempt_id, &JobControl::new()).unwrap();
    let conv = answering::conversation(&h.svc, &s.conversation_id).unwrap();
    let a = conv.turns[0].attempt.as_ref().unwrap();
    assert_eq!((a.state.as_str(), a.error_kind.as_deref()), ("failed", Some("no_generation_model")));
    assert_eq!(conv.turns[0].question, "Anything?");

    // After setup, a retry is a new attempt on the same question.
    let p = models::add_ollama(&h.svc, "http://127.0.0.1:11434").unwrap();
    models::save_generation(&h.svc, &p.id, "fake-chat").unwrap();
    let retry = answering::retry(&h.svc, &runner, "r2", &s.message_id, None).unwrap();
    answering::run_attempt(&h.svc, &retry, &JobControl::new()).unwrap();
    let conv = answering::conversation(&h.svc, &s.conversation_id).unwrap();
    assert_eq!(conv.turns[0].attempt_count, 2);
    assert_ne!(conv.turns[0].attempt.as_ref().unwrap().state, "failed");
}

#[test]
fn cloud_provider_requires_consent() {
    let (h, _sched, _coll) = setup();
    let p = models::add_api_provider(&h.svc, ProviderKind::OpenaiCompatible, "OpenAI", "https://api.openai.com/v1", "sk-test-1234567890").unwrap();
    assert!(!p.is_local);
    assert!(models::save_generation(&h.svc, &p.id, "fake-chat").is_err());
    models::set_consent(&h.svc, &p.id, true).unwrap();
    assert!(models::save_generation(&h.svc, &p.id, "fake-chat").is_ok());
    models::remove_provider(&h.svc, &p.id).unwrap();
    assert!(models::generation_choice(&h.svc).unwrap().is_none());
}

#[test]
fn keyless_gateway_only_on_loopback() {
    let (h, _sched, _coll) = setup();
    let p = models::add_api_provider(&h.svc, ProviderKind::OpenaiCompatible, "LiteLLM", "http://127.0.0.1:4000", "").unwrap();
    assert!(!p.has_secret);
    assert!(!p.is_local, "a gateway may forward to cloud providers, so consent is still required");
    assert!(models::add_api_provider(&h.svc, ProviderKind::OpenaiCompatible, "Remote", "https://litellm.example.com", "").is_err());
    assert!(models::add_api_provider(&h.svc, ProviderKind::Anthropic, "A", "http://127.0.0.1:4000", "").is_err());
    assert!(models::add_api_provider(&h.svc, ProviderKind::OpenaiCompatible, "LAN", "http://192.168.1.4:4000", "").is_err());
    models::remove_provider(&h.svc, &p.id).unwrap();
}

#[test]
fn comparison_cells_are_cited_and_exported() {
    let (h, sched, coll) = setup();
    let a = ready_doc(&h, &sched, &coll, "a.pdf");
    let b = ready_doc(&h, &sched, &coll, "b.pdf");
    let p = models::add_ollama(&h.svc, "http://127.0.0.1:11434").unwrap();
    models::save_generation(&h.svc, &p.id, "fake-chat").unwrap();
    let runner = GenerationRunner::start(Arc::clone(&h.svc));
    runner.shutdown();
    let rows = vec![
        ComparisonRow { key: "method".into(), label: "Method".into() },
        ComparisonRow { key: "anything".into(), label: "Benchmark recall".into() },
    ];
    assert!(comparison::build(&h.svc, &runner, "x0", &[a.to_string()], &rows).is_err(), "needs two papers");
    let id = comparison::build(&h.svc, &runner, "x1", &[a.to_string(), b.to_string()], &rows).unwrap();
    comparison::run_comparison(&h.svc, &id, &JobControl::new(), &runner).unwrap();
    let v = h.svc.answers.comparison(&id).unwrap().unwrap();
    assert_eq!(v.state, "completed");
    assert_eq!(v.cells.len(), 4);
    assert!(v.cells.iter().all(|c| c.status != "pending"));
    assert_eq!(v.linked_cells, 2, "the benchmark row is supported in both papers");
    assert!(v.cells.iter().filter(|c| c.row_key == "method").all(|c| c.status == "evidence_not_found"), "no passage mentions a method");
    let cell = v.cells.iter().find(|c| c.status == "supported").unwrap();
    let loc = answering::open_evidence(&h.svc, &EvidenceTarget::Cell { cell_id: cell.id.clone(), number: cell.evidence[0].number }).unwrap();
    assert!(loc.page_index < 3);
    let md = comparison::export(&h.svc, &id, "markdown").unwrap();
    assert!(md.contains("| Method |") && md.contains("p. "));
    let csv = comparison::export(&h.svc, &id, "csv").unwrap();
    assert!(csv.lines().count() == 5);
}

#[test]
fn page_open_in_reader_gives_evidence_without_keyword_matches() {
    let (h, sched, coll) = setup();
    let doc = ready_doc(&h, &sched, &coll, "a.pdf");
    let p = models::add_ollama(&h.svc, "http://127.0.0.1:11434").unwrap();
    models::save_generation(&h.svc, &p.id, "fake-chat").unwrap();
    let runner = GenerationRunner::start(Arc::clone(&h.svc));
    runner.shutdown();
    let question = "ايش مكتوب بهالصفحة؟";

    let plain = ScopeInput { collection_id: Some(coll.to_string()), document_ids: None, viewing: None };
    let a = answering::ask(&h.svc, &runner, "req-a", None, question, &plain).unwrap();
    answering::run_attempt(&h.svc, &a.attempt_id, &JobControl::new()).unwrap();
    let conv = answering::conversation(&h.svc, &a.conversation_id).unwrap();
    assert_eq!(conv.turns[0].attempt.as_ref().unwrap().state, "no_evidence");

    let viewing = ScopeInput { viewing: Some(crate::application::views::ViewingPage { document_id: doc.to_string(), page_index: 1 }), ..plain };
    let b = answering::ask(&h.svc, &runner, "req-b", None, question, &viewing).unwrap();
    answering::run_attempt(&h.svc, &b.attempt_id, &JobControl::new()).unwrap();
    let conv = answering::conversation(&h.svc, &b.conversation_id).unwrap();
    let attempt = conv.turns[0].attempt.as_ref().unwrap();
    assert_ne!(attempt.state, "no_evidence");
    assert!(attempt.sources.iter().all(|s| s.page_index == 1), "only the viewed page had evidence");
}
