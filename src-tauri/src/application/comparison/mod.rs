//! BuildComparison: two ready papers × chosen aspects. Each cell runs its own retrieval
//! inside one paper, then a cited, validated generation. Cells without evidence say so.
use crate::application::answering::prompt;
use crate::application::error::{AppError, AppResult};
use crate::application::generation::{GenerationRunner, Task};
use crate::application::jobs::{JobControl, Signal};
use crate::application::models;
use crate::application::ports::{AppEvent, CellUpdate, ChatRequest};
use crate::application::retrieval::{self, ResolvedScope};
use crate::application::views::{CellEvidence, ComparisonRow, ComparisonView, RetrievalMode, ScopeInput};
use crate::application::{settings, Services};
use crate::domain::evidence::{parse_model_json, validate_answer, AnswerStatus, Evidence, RawAnswer, RawClaim};
use crate::domain::ids::DocumentId;
use crate::domain::onboarding::{StepKey, StepState, FLOW_VERSION};

pub const DEFAULT_ROWS: [(&str, &str); 6] = [
    ("method", "Method"),
    ("data", "Data"),
    ("metric", "Metric"),
    ("result", "Result"),
    ("limitations", "Limitations"),
    ("sample_size", "Sample size"),
];

/// Retrieval query per aspect (the label is added so custom rows work too).
fn row_query(key: &str, label: &str) -> String {
    let base = match key {
        "method" => "method approach proposed model architecture technique we propose",
        "data" => "dataset data corpus benchmark collection training test split",
        "metric" => "evaluation metric measure accuracy F1 recall precision nDCG score",
        "result" => "results improvement outperforms achieved table performance significant",
        "limitations" => "limitations limitation future work drawback threats to validity fails",
        "sample_size" => "sample size participants number of examples queries documents n =",
        _ => "",
    };
    format!("{label} {base}")
}

fn valid_rows(rows: &[ComparisonRow]) -> AppResult<Vec<ComparisonRow>> {
    if rows.is_empty() || rows.len() > 8 {
        return Err(AppError::validation("choose 1–8 aspects"));
    }
    let mut out: Vec<ComparisonRow> = Vec::new();
    for r in rows {
        let label = r.label.split_whitespace().collect::<Vec<_>>().join(" ");
        if label.is_empty() || label.chars().count() > 80 {
            return Err(AppError::validation("aspect label"));
        }
        let key = if DEFAULT_ROWS.iter().any(|(k, _)| *k == r.key) { r.key.clone() } else { format!("custom_{}", out.len()) };
        if out.iter().any(|o| o.key == key) {
            continue;
        }
        out.push(ComparisonRow { key, label });
    }
    Ok(out)
}

pub fn build(svc: &Services, runner: &GenerationRunner, request_id: &str, document_ids: &[String], rows: &[ComparisonRow]) -> AppResult<String> {
    if let Some(existing) = svc.answers.find_comparison_by_request(request_id)? {
        return Ok(existing);
    }
    if document_ids.len() != 2 || document_ids[0] == document_ids[1] {
        return Err(AppError::validation("select two different papers"));
    }
    let docs = document_ids.iter().map(|d| DocumentId::parse(d).ok_or_else(|| AppError::validation("bad document id"))).collect::<AppResult<Vec<_>>>()?;
    let ready = svc.index.searchable_docs(&docs)?;
    if ready.len() != 2 || ready.iter().any(|d| !d.lexical_ready) {
        return Err(AppError::not_allowed("both papers must be ready for search"));
    }
    let choice = models::generation_choice(svc)?.ok_or_else(|| AppError::new(crate::application::error::ErrorCode::Unsupported, "no generation model"))?;
    let rows = valid_rows(rows)?;
    let id = uuid::Uuid::now_v7().to_string();
    let mut cells = Vec::new();
    for r in &rows {
        for d in &docs {
            cells.push((uuid::Uuid::now_v7().to_string(), d.clone(), r.key.clone(), r.label.clone()));
        }
    }
    svc.answers.create_comparison(&id, request_id, &docs, &serde_json::to_string(&rows).unwrap(), &format!("{}|{}", choice.provider_id, choice.model), &cells, svc.now())?;
    runner.submit(Task::Comparison(id.clone()));
    Ok(id)
}

pub fn run_comparison(svc: &Services, id: &str, control: &JobControl, runner: &GenerationRunner) -> AppResult<()> {
    let view = svc.answers.comparison(id)?.ok_or_else(|| AppError::not_found("comparison"))?;
    for cell in view.cells.iter().filter(|c| c.status == "pending") {
        if control.signal() == Signal::Cancel {
            svc.answers.set_comparison_state(id, "cancelled", svc.now())?;
            svc.events.emit(AppEvent::ComparisonUpdated { comparison_id: id.into() });
            return Ok(());
        }
        let cell_control = runner.control(&cell.id);
        run_cell(svc, &view, &cell.id, cell_control.as_deref().unwrap_or(control))?;
        svc.events.emit(AppEvent::ComparisonUpdated { comparison_id: id.into() });
    }
    svc.answers.set_comparison_state(id, "completed", svc.now())?;
    if let Some(done) = svc.answers.comparison(id)? {
        if done.linked_cells > 0 {
            svc.onboarding.set(FLOW_VERSION, StepKey::Compare, StepState::Completed, Some(id), svc.now())?;
        }
    }
    svc.events.emit(AppEvent::ComparisonUpdated { comparison_id: id.into() });
    Ok(())
}

pub fn run_single_cell(svc: &Services, comparison_id: &str, cell_id: &str, control: &JobControl) -> AppResult<()> {
    let view = svc.answers.comparison(comparison_id)?.ok_or_else(|| AppError::not_found("comparison"))?;
    run_cell(svc, &view, cell_id, control)?;
    svc.events.emit(AppEvent::ComparisonUpdated { comparison_id: comparison_id.into() });
    Ok(())
}

pub fn retry_cell(svc: &Services, runner: &GenerationRunner, comparison_id: &str, cell_id: &str) -> AppResult<()> {
    let view = svc.answers.comparison(comparison_id)?.ok_or_else(|| AppError::not_found("comparison"))?;
    if !view.cells.iter().any(|c| c.id == cell_id) {
        return Err(AppError::not_found("cell"));
    }
    svc.answers.update_cell(&CellUpdate { id: cell_id.into(), status: "pending".into(), text: None, evidence_json: "[]".into(), searched: None, now: svc.now() })?;
    svc.events.emit(AppEvent::ComparisonUpdated { comparison_id: comparison_id.into() });
    runner.submit(Task::Cell { comparison_id: comparison_id.into(), cell_id: cell_id.into() });
    Ok(())
}

fn update(svc: &Services, cell_id: &str, status: &str, text: Option<String>, evidence: &[CellEvidence], searched: Option<String>) -> AppResult<()> {
    svc.answers.update_cell(&CellUpdate {
        id: cell_id.into(),
        status: status.into(),
        text,
        evidence_json: serde_json::to_string(evidence).unwrap(),
        searched,
        now: svc.now(),
    })
}

fn run_cell(svc: &Services, view: &ComparisonView, cell_id: &str, control: &JobControl) -> AppResult<()> {
    let cell = view.cells.iter().find(|c| c.id == cell_id).ok_or_else(|| AppError::not_found("cell"))?;
    let row = view.rows.iter().find(|r| r.key == cell.row_key).ok_or_else(|| AppError::not_found("row"))?;
    let doc = view.documents.iter().find(|d| d.id == cell.document_id).ok_or_else(|| AppError::not_found("document"))?;
    let Some(choice) = models::generation_choice(svc)? else { return update(svc, cell_id, "failed", None, &[], Some("no_generation_model".into())) };
    let (record, client) = match models::open(svc, &choice.provider_id) {
        Ok(v) => v,
        Err(e) => return update(svc, cell_id, "failed", None, &[], Some(e.detail)),
    };
    if models::ensure_allowed(svc, &record).is_err() {
        return update(svc, cell_id, "failed", None, &[], Some("not_allowed".into()));
    }
    let scope_input = ScopeInput { collection_id: None, document_ids: Some(vec![doc.id.to_string()]), viewing: None };
    let scope: ResolvedScope = retrieval::resolve_scope(svc, &scope_input, RetrievalMode::Hybrid)?;
    if scope.lexical.is_empty() {
        return update(svc, cell_id, "evidence_not_found", None, &[], Some("paper not searchable".into()));
    }
    let query = row_query(&row.key, &row.label);
    let qvec = retrieval::embed_query(&scope, &query).ok().flatten();
    let hits = retrieval::search_in(svc, &scope, &query, qvec.as_deref(), None, 4)?;
    let pages: Vec<String> = {
        let mut p: Vec<u32> = hits.iter().map(|h| h.chunk.page_index + 1).collect();
        p.sort_unstable();
        p.dedup();
        p.iter().map(|x| x.to_string()).collect()
    };
    let searched = format!("{} passages · pp. {}", hits.len(), pages.join(", "));
    if hits.is_empty() {
        return update(svc, cell_id, "evidence_not_found", None, &[], Some(searched));
    }
    let bundle: Vec<Evidence> = hits
        .iter()
        .enumerate()
        .map(|(i, h)| Evidence {
            id: format!("E{}", i + 1),
            chunk_id: h.chunk.id,
            document_id: h.chunk.document_id.clone(),
            generation_id: h.chunk.generation_id.clone(),
            title: doc.title.clone(),
            authors: doc.authors.clone(),
            year: doc.year,
            page_index: h.chunk.page_index,
            page_offset: h.chunk.start,
            text: h.chunk.text.chars().take(3000).collect(),
        })
        .collect();
    let language = prompt::answer_language(&settings::get_settings(svc)?.answer_locale, &row.label);
    let req = ChatRequest {
        system: prompt::cell_system(language),
        user: prompt::cell_user(&doc.title, &row.label, &bundle),
        json: true,
        max_output_tokens: 500,
    };
    let reply = match client.chat(&choice.model, &req, control, &mut |_| {}) {
        Ok(o) if o.cancelled => return update(svc, cell_id, "failed", None, &[], Some("cancelled".into())),
        Ok(o) => o.text,
        Err(e) => return update(svc, cell_id, "failed", None, &[], Some(e.detail.chars().take(200).collect())),
    };
    #[derive(serde::Deserialize, Default)]
    struct RawCell {
        #[serde(default)]
        status: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        evidence_ids: Vec<String>,
        #[serde(default)]
        quote: Option<String>,
    }
    let raw: RawCell = match reply.find('{').and_then(|s| reply.rfind('}').map(|e| &reply[s..=e])).and_then(|j| serde_json::from_str(j).ok()) {
        Some(r) => r,
        None => {
            // Fall back to the answer parser (handles fenced/nested JSON).
            match parse_model_json(&reply) {
                Ok(a) => {
                    let c = a.claims.into_iter().next().unwrap_or_default();
                    RawCell { status: a.answer_status.unwrap_or_default(), text: c.text, evidence_ids: c.evidence_ids, quote: c.quote }
                }
                Err(_) => return update(svc, cell_id, "failed", None, &[], Some("invalid_output".into())),
            }
        }
    };
    if raw.status == "evidence_not_found" || raw.text.trim().is_empty() {
        return update(svc, cell_id, "evidence_not_found", None, &[], Some(searched));
    }
    let checked = validate_answer(
        &RawAnswer {
            answer_status: Some(raw.status.clone()),
            claims: vec![RawClaim { text: raw.text.clone(), kind: Some("stated".into()), evidence_ids: raw.evidence_ids, quote: raw.quote }],
            limitations: None,
        },
        &bundle,
    );
    let Some(claim) = checked.claims.first().filter(|_| checked.status != AnswerStatus::NoEvidence) else {
        // The model wrote something but cited nothing valid: not shown as supported.
        return update(svc, cell_id, "evidence_not_found", None, &[], Some(searched));
    };
    let evidence: Vec<CellEvidence> = claim
        .citations
        .iter()
        .map(|c| CellEvidence {
            number: c.number,
            document_id: c.document_id.clone(),
            page_index: c.page_index,
            span: c.span,
            quote: c.quote.clone(),
            excerpt: bundle.iter().find(|e| e.id == c.evidence_id).map(|e| e.text.chars().take(600).collect()).unwrap_or_default(),
        })
        .collect();
    let status = if raw.status == "partial" || claim.unverified_number { "partial" } else { "supported" };
    update(svc, cell_id, status, Some(claim.text.clone()), &evidence, Some(searched))
}

fn csv_field(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Markdown or CSV with titles, pages and excerpts so sources can be found outside the app.
pub fn export(svc: &Services, id: &str, format: &str) -> AppResult<String> {
    let v = svc.answers.comparison(id)?.ok_or_else(|| AppError::not_found("comparison"))?;
    let cell = |doc: &DocumentId, row: &str| v.cells.iter().find(|c| &c.document_id == doc && c.row_key == row);
    let render = |c: Option<&crate::application::views::ComparisonCellView>| -> (String, String) {
        match c {
            Some(c) if c.status == "supported" || c.status == "partial" => {
                let refs = c.evidence.iter().map(|e| format!("p. {}", e.page_index + 1)).collect::<Vec<_>>().join("; ");
                (format!("{}{}", c.text.clone().unwrap_or_default(), if c.status == "partial" { " (partial)" } else { "" }), refs)
            }
            Some(c) if c.status == "evidence_not_found" => ("No evidence found in the retrieved passages".into(), String::new()),
            Some(c) => (format!("Not completed ({})", c.status), String::new()),
            None => (String::new(), String::new()),
        }
    };
    match format {
        "csv" => {
            let mut out = String::from("aspect,paper,authors,year,text,pages,quotes\n");
            for r in &v.rows {
                for d in &v.documents {
                    let c = cell(&d.id, &r.key);
                    let (text, pages) = render(c);
                    let quotes = c.map(|c| c.evidence.iter().filter_map(|e| e.quote.clone()).collect::<Vec<_>>().join(" | ")).unwrap_or_default();
                    out.push_str(&[csv_field(&r.label), csv_field(&d.title), csv_field(d.authors.as_deref().unwrap_or("")), d.year.map(|y| y.to_string()).unwrap_or_default(), csv_field(&text), csv_field(&pages), csv_field(&quotes)].join(","));
                    out.push('\n');
                }
            }
            Ok(out)
        }
        "markdown" => {
            let esc = |s: &str| s.replace('|', "\\|").replace('\n', " ");
            let mut out = String::from("# Paper comparison\n\n");
            for (i, d) in v.documents.iter().enumerate() {
                out.push_str(&format!(
                    "- **Paper {}:** {}{}{}\n",
                    if i == 0 { "A" } else { "B" },
                    d.title,
                    d.authors.as_deref().map(|a| format!(" — {a}")).unwrap_or_default(),
                    d.year.map(|y| format!(" ({y})")).unwrap_or_default()
                ));
            }
            out.push_str(&format!("\n| Aspect | {} | {} |\n|---|---|---|\n", esc(&v.documents[0].title), esc(&v.documents.get(1).map(|d| d.title.clone()).unwrap_or_default())));
            for r in &v.rows {
                let cols: Vec<String> = v
                    .documents
                    .iter()
                    .map(|d| {
                        let (t, p) = render(cell(&d.id, &r.key));
                        esc(&if p.is_empty() { t } else { format!("{t} [{p}]") })
                    })
                    .collect();
                out.push_str(&format!("| {} | {} |\n", esc(&r.label), cols.join(" | ")));
            }
            out.push_str("\n## Quoted passages\n\n");
            for c in &v.cells {
                for e in &c.evidence {
                    let title = v.documents.iter().find(|d| d.id == c.document_id).map(|d| d.title.clone()).unwrap_or_default();
                    out.push_str(&format!("- {} — p. {}: “{}”\n", title, e.page_index + 1, e.quote.clone().unwrap_or_else(|| e.excerpt.chars().take(240).collect())));
                }
            }
            out.push_str(&format!(
                "\n_{} of {} completed cells are linked to sources. Links inside Papryvio work only on the computer that holds the library; use the titles and page numbers above to find the passages elsewhere._\n",
                v.linked_cells, v.completed_cells
            ));
            Ok(out)
        }
        _ => Err(AppError::validation("format must be markdown or csv")),
    }
}
