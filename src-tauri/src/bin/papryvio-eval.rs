//! Retrieval/answer evaluation harness.
//!
//! Builds a throwaway library from a folder of PDFs with the real pipeline (PDFium worker,
//! chunking, FTS5, optional Ollama embeddings), then runs a JSONL dataset of questions and
//! reports Hit@k, Recall@k and MRR for lexical, semantic and hybrid retrieval. With
//! `--chat-model` it also asks every question and records answer status, structural
//! citation validity and behaviour on unanswerable questions.
//!
//! ```text
//! papryvio-eval --dataset evaluation/datasets/smoke.jsonl --papers src-tauri/tests/fixtures \
//!     [--ollama http://127.0.0.1:11434 --embed-model nomic-embed-text --chat-model qwen2.5:7b] \
//!     [--k 5] [--out evaluation/reports]
//! ```
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use papryvio_lib::application::error::{AppError, AppResult};
use papryvio_lib::application::generation::GenerationRunner;
use papryvio_lib::application::ingestion::{extract, import};
use papryvio_lib::application::jobs::{JobControl, JobScheduler};
use papryvio_lib::application::ports::*;
use papryvio_lib::application::views::{RetrievalMode, ScopeInput};
use papryvio_lib::application::{answering, indexing, library, models, retrieval, Services};
use papryvio_lib::domain::ids::DocumentId;
use papryvio_lib::domain::job::{JobKind, JobState};
use papryvio_lib::infrastructure::filesystem::{AppPaths, LocalFileStore};
use papryvio_lib::infrastructure::models::ollama::Ollama;
use papryvio_lib::infrastructure::pdf::WorkerPdfExtractor;
use papryvio_lib::infrastructure::sqlite::{Db, SqliteStore};
use papryvio_lib::infrastructure::SystemClock;
use serde::Deserialize;

#[derive(Deserialize)]
struct Item {
    id: String,
    question: String,
    /// File name of the paper the answer is in (inside --papers).
    #[serde(default)]
    paper: Option<String>,
    /// 1-based pages that contain the evidence.
    #[serde(default)]
    pages: Vec<u32>,
    /// Optional phrase that must appear in a retrieved passage for it to count.
    #[serde(default)]
    contains: Option<String>,
    #[serde(default = "yes")]
    answerable: bool,
    #[serde(default)]
    kind: Option<String>,
}
fn yes() -> bool {
    true
}

struct NullEvents;
impl EventSink for NullEvents {
    fn emit(&self, _e: AppEvent) {}
}
#[derive(Default)]
struct MemSecrets(Mutex<HashMap<String, String>>);
impl SecretStore for MemSecrets {
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
struct OllamaOnly;
impl ProviderFactory for OllamaOnly {
    fn open(&self, r: &ProviderRecord) -> AppResult<Arc<dyn ModelProvider>> {
        Ok(Arc::new(Ollama::new(&r.base_url)))
    }
}
struct NoSignIn;
impl SignInService for NoSignIn {
    fn start(&self, _k: ProviderKind) -> AppResult<SignInStart> {
        Err(AppError::validation("not available in evaluation"))
    }
    fn poll(&self, _f: &str) -> AppResult<SignInPoll> {
        Ok(SignInPoll::Expired)
    }
    fn cancel(&self, _f: &str) {}
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn run_job(svc: &Services, doc: &DocumentId, kind: JobKind) -> AppResult<JobState> {
    let mut job = match svc.jobs.latest_for_entity(doc.as_str(), kind)? {
        Some(j) if j.state.is_active() => j,
        _ => {
            let id = models_enqueue(svc, kind, doc)?;
            svc.jobs.get(&id)?.unwrap()
        }
    };
    job.state = JobState::Running;
    svc.jobs.save(&job)?;
    let c = JobControl::new();
    match kind {
        JobKind::Extract => extract::run_extract_job(svc, &mut job, &c)?,
        JobKind::Index => indexing::run_index_job(svc, &mut job, &c)?,
        JobKind::Embed => indexing::run_embed_job(svc, &mut job, &c)?,
        JobKind::DeleteDocument => {}
    }
    Ok(job.state)
}

fn models_enqueue(svc: &Services, kind: JobKind, doc: &DocumentId) -> AppResult<papryvio_lib::domain::ids::JobId> {
    let job = NewJob {
        id: papryvio_lib::domain::ids::JobId::new(),
        kind,
        entity_id: doc.to_string(),
        attempt_id: papryvio_lib::domain::ids::AttemptId::new(),
        stage: "queued".into(),
        now: svc.now(),
    };
    svc.jobs.insert(&job)?;
    Ok(job.id)
}

#[derive(Default, Clone, serde::Serialize)]
struct ModeScore {
    questions: u32,
    hit_at_k: f64,
    recall_at_k: f64,
    mrr: f64,
    median_ms: u64,
    p95_ms: u64,
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some(papryvio_lib::PDF_WORKER_FLAG) {
        std::process::exit(papryvio_lib::run_pdf_worker());
    }
    if let Err(e) = run() {
        eprintln!("evaluation failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> AppResult<()> {
    let args: Vec<String> = std::env::args().collect();
    let dataset = PathBuf::from(arg(&args, "--dataset").ok_or_else(|| AppError::validation("--dataset required"))?);
    let papers = PathBuf::from(arg(&args, "--papers").ok_or_else(|| AppError::validation("--papers required"))?);
    let k: usize = arg(&args, "--k").and_then(|v| v.parse().ok()).unwrap_or(5);
    let out_dir = PathBuf::from(arg(&args, "--out").unwrap_or_else(|| "evaluation/reports".into()));
    let ollama = arg(&args, "--ollama").unwrap_or_else(|| "http://127.0.0.1:11434".into());
    let embed_model = arg(&args, "--embed-model");
    let chat_model = arg(&args, "--chat-model");
    let pdfium = PathBuf::from(arg(&args, "--pdfium").unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/resources/pdfium/pdfium.dll").into()));

    let items: Vec<Item> = std::fs::read_to_string(&dataset)
        .map_err(|e| AppError::file_access(e.to_string()))?
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .map(|l| serde_json::from_str(l).map_err(|e| AppError::validation(format!("dataset line: {e}"))))
        .collect::<AppResult<_>>()?;

    let tmp = std::env::temp_dir().join(format!("papryvio-eval-{}", uuid::Uuid::new_v4()));
    let paths = AppPaths::create(tmp.clone()).map_err(|e| AppError::file_access(e.to_string()))?;
    let store = Arc::new(SqliteStore::new(Db::open(&paths.database)?));
    let exe = std::env::current_exe().map_err(|e| AppError::internal(e.to_string()))?;
    let svc = Arc::new(Services {
        clock: Arc::new(SystemClock),
        collections: store.clone(),
        documents: store.clone(),
        pages: store.clone(),
        jobs: store.clone(),
        settings: store.clone(),
        onboarding: store.clone(),
        idempotency: store.clone(),
        files: Arc::new(LocalFileStore::new(paths).map_err(|e| AppError::file_access(e.to_string()))?),
        pdf: Arc::new(WorkerPdfExtractor::new(exe, pdfium, papryvio_lib::PDF_WORKER_FLAG)),
        events: Arc::new(NullEvents),
        index: store.clone(),
        answers: store.clone(),
        providers: store.clone(),
        secrets: Arc::new(MemSecrets::default()),
        provider_factory: Arc::new(OllamaOnly),
        sign_in: Arc::new(NoSignIn),
    });
    let scheduler = JobScheduler::start(Arc::clone(&svc));
    scheduler.shutdown(); // jobs are driven synchronously below
    let collection = library::create_collection(&svc, &uuid::Uuid::new_v4().to_string(), "Evaluation")?;

    // Import every PDF referenced by the dataset.
    let mut docs: HashMap<String, DocumentId> = HashMap::new();
    let started = Instant::now();
    let mut names: Vec<String> = items.iter().filter_map(|i| i.paper.clone()).collect();
    names.sort();
    names.dedup();
    for name in &names {
        let path = papers.join(name);
        let report = import::import_files(&svc, &scheduler, &uuid::Uuid::new_v4().to_string(), "eval", Some(&[path]), &collection.id)?;
        let id = match &report.items[0].outcome {
            papryvio_lib::application::views::ImportOutcome::Imported { document_id, .. } => document_id.clone(),
            papryvio_lib::application::views::ImportOutcome::AlreadyInLibrary { document_id, .. } => document_id.clone(),
            other => return Err(AppError::validation(format!("{name}: {other:?}"))),
        };
        run_job(&svc, &id, JobKind::Extract)?;
        run_job(&svc, &id, JobKind::Index)?;
        docs.insert(name.clone(), id);
    }
    let index_ms = started.elapsed().as_millis();
    eprintln!("indexed {} papers in {index_ms} ms", docs.len());

    let provider = models::add_ollama(&svc, &ollama)?;
    let mut modes = vec![RetrievalMode::Lexical];
    if let Some(m) = &embed_model {
        models::save_embedding(&svc, &scheduler, &provider.id, m)?;
        for id in docs.values() {
            run_job(&svc, id, JobKind::Embed)?;
        }
        modes.push(RetrievalMode::Semantic);
        modes.push(RetrievalMode::Hybrid);
    }

    let scope = ScopeInput { collection_id: Some(collection.id.to_string()), document_ids: None, viewing: None };
    let mut scores: Vec<(RetrievalMode, ModeScore)> = Vec::new();
    for mode in &modes {
        let mut s = ModeScore::default();
        let mut times = Vec::new();
        for it in items.iter().filter(|i| i.answerable && i.paper.is_some()) {
            let target = &docs[it.paper.as_ref().unwrap()];
            let t0 = Instant::now();
            let res = retrieval::search(&svc, &it.question, &scope, *mode, k as u32)?;
            times.push(t0.elapsed().as_millis() as u64);
            let relevant = |h: &papryvio_lib::application::views::SearchHitView| {
                &h.document_id == target
                    && (it.pages.is_empty() || it.pages.contains(&(h.page_index + 1)))
                    && it.contains.as_ref().is_none_or(|c| h.excerpt.to_lowercase().contains(&c.to_lowercase()))
            };
            let first = res.hits.iter().take(k).position(relevant);
            s.questions += 1;
            if let Some(p) = first {
                s.hit_at_k += 1.0;
                s.mrr += 1.0 / (p as f64 + 1.0);
            }
            let wanted = it.pages.len().max(1) as f64;
            let found: std::collections::HashSet<u32> = res.hits.iter().take(k).filter(|h| relevant(h)).map(|h| h.page_index + 1).collect();
            s.recall_at_k += (found.len() as f64 / wanted).min(1.0);
        }
        if s.questions > 0 {
            let n = s.questions as f64;
            s.hit_at_k /= n;
            s.recall_at_k /= n;
            s.mrr /= n;
        }
        times.sort_unstable();
        s.median_ms = times.get(times.len() / 2).copied().unwrap_or(0);
        s.p95_ms = times.get(((times.len() as f64) * 0.95) as usize).or(times.last()).copied().unwrap_or(0);
        scores.push((*mode, s));
    }

    // Optional: answers.
    let mut answer_rows = Vec::new();
    if let Some(m) = &chat_model {
        models::save_generation(&svc, &provider.id, m)?;
        let runner = GenerationRunner::start(Arc::clone(&svc));
        runner.shutdown();
        for it in &items {
            let t0 = Instant::now();
            let started = answering::ask(&svc, &runner, &uuid::Uuid::new_v4().to_string(), None, &it.question, &scope)?;
            answering::run_attempt(&svc, &started.attempt_id, &JobControl::new())?;
            let conv = answering::conversation(&svc, &started.conversation_id)?;
            let a = conv.turns[0].attempt.as_ref().unwrap();
            let claims = a.answer.as_ref().map(|x| x.claims.len()).unwrap_or(0);
            let located = a.answer.as_ref().map(|x| x.claims.iter().flat_map(|c| &c.citations).filter(|c| c.span.is_some()).count()).unwrap_or(0);
            let cites = a.answer.as_ref().map(|x| x.claims.iter().map(|c| c.citations.len()).sum::<usize>()).unwrap_or(0);
            let correct_refusal = !it.answerable && a.state == "no_evidence";
            answer_rows.push(serde_json::json!({
                "id": it.id, "kind": it.kind, "answerable": it.answerable, "state": a.state, "error": a.error_kind,
                "claims": claims, "citations": cites, "quotes_located": located,
                "dropped_claims": a.answer.as_ref().map(|x| x.dropped_claims).unwrap_or(0),
                "unanswerable_handled": if it.answerable { serde_json::Value::Null } else { correct_refusal.into() },
                "ms": t0.elapsed().as_millis() as u64
            }));
        }
    }

    std::fs::create_dir_all(&out_dir).map_err(|e| AppError::file_access(e.to_string()))?;
    let stamp = chrono_like();
    let report = serde_json::json!({
        "dataset": dataset.display().to_string(),
        "papers": names,
        "k": k,
        "index_ms": index_ms as u64,
        "embedding_model": embed_model,
        "chat_model": chat_model,
        "chunker_version": papryvio_lib::domain::chunking::CHUNKER_VERSION,
        "prompt_version": papryvio_lib::domain::evidence::PROMPT_VERSION,
        "retrieval": scores.iter().map(|(m, s)| (format!("{m:?}").to_lowercase(), s.clone())).collect::<HashMap<_, _>>(),
        "answers": answer_rows,
        "os": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    });
    let json_path = out_dir.join(format!("report-{stamp}.json"));
    std::fs::write(&json_path, serde_json::to_string_pretty(&report).unwrap()).map_err(|e| AppError::file_access(e.to_string()))?;
    let mut md = format!("# Retrieval evaluation — {stamp}\n\nDataset `{}`, {} papers, k = {k}.\n\n| Mode | Questions | Hit@{k} | Recall@{k} | MRR | median ms | p95 ms |\n|---|---|---|---|---|---|---|\n", dataset.display(), names.len());
    for (m, s) in &scores {
        md.push_str(&format!("| {m:?} | {} | {:.3} | {:.3} | {:.3} | {} | {} |\n", s.questions, s.hit_at_k, s.recall_at_k, s.mrr, s.median_ms, s.p95_ms));
    }
    if !answer_rows.is_empty() {
        md.push_str("\n## Answers\n\n| id | answerable | state | claims | citations | quotes located |\n|---|---|---|---|---|---|\n");
        for r in &answer_rows {
            md.push_str(&format!("| {} | {} | {} | {} | {} | {} |\n", r["id"], r["answerable"], r["state"], r["claims"], r["citations"], r["quotes_located"]));
        }
        md.push_str("\nStructural checks only: whether each claim is semantically supported needs human review.\n");
    }
    let md_path = out_dir.join(format!("report-{stamp}.md"));
    std::fs::write(&md_path, md).map_err(|e| AppError::file_access(e.to_string()))?;
    println!("{}\n{}", json_path.display(), md_path.display());
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

fn chrono_like() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}")
}
