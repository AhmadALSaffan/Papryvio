//! Integration tests: real SQLite (in memory) + real file store (temp dir) + a scripted
//! PDF extractor double, exercising the import → extraction pipeline end to end.
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::application::error::ErrorCode;
use crate::application::ingestion::{delete, extract, import, recovery};
use crate::application::jobs::{JobControl, JobScheduler};
use crate::application::ports::*;
use crate::application::views::{DocumentQuery, ImportOutcome, MetadataUpdate};
use crate::application::{library, onboarding, Services};
use crate::domain::document::{ExtractionState, FailureKind, MetadataOrigin};
use crate::domain::ids::{CollectionId, DocumentId};
use crate::domain::job::{JobKind, JobState};
use crate::domain::onboarding::{StepAction, StepKey, StepState};
use crate::infrastructure::filesystem::{AppPaths, LocalFileStore};

use super::{Db, SqliteStore};

struct NullEvents;
impl EventSink for NullEvents {
    fn emit(&self, _event: AppEvent) {}
}

struct FixedClock(Mutex<i64>);
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        let mut t = self.0.lock().unwrap();
        *t += 1;
        *t
    }
}

/// Scripted extractor: `pages[i]` is the text of page i. Can request a pause after N pages.
pub(super) struct FakePdf {
    pub pages: Vec<String>,
    pub title: Option<String>,
    pub pause_after: Arc<Mutex<Option<u32>>>,
    pub fail_with: Option<FailureKind>,
}

impl PdfExtractor for FakePdf {
    fn is_available(&self) -> bool {
        true
    }
    fn extract(
        &self,
        _file: &Path,
        start_page: u32,
        control: &JobControl,
        on_event: &mut dyn FnMut(ExtractEvent) -> Flow,
    ) -> Result<ExtractOutcome, ExtractFailure> {
        if let Some(kind) = self.fail_with {
            return Err(ExtractFailure { kind, detail: "scripted".into() });
        }
        let meta = ExtractedMetadata {
            page_count: self.pages.len() as u32,
            title: self.title.clone(),
            author: Some("Ada Lovelace".into()),
            creation_date: Some("D:20210301000000Z".into()),
        };
        if on_event(ExtractEvent::Meta(meta)) != Flow::Continue {
            return Ok(ExtractOutcome::Paused);
        }
        for i in start_page..self.pages.len() as u32 {
            let page = ExtractedPage {
                index: i,
                width: 612.0,
                height: 792.0,
                rotation: 0,
                label: None,
                text: self.pages[i as usize].clone(),
                spans: vec![SpanBox { start: 0, end: 4, left: 72.0, bottom: 700.0, right: 200.0, top: 712.0 }],
                image_objects: if self.pages[i as usize].is_empty() { 1 } else { 0 },
            };
            if *self.pause_after.lock().unwrap() == Some(i) {
                control.request_pause();
            }
            match on_event(ExtractEvent::Page(page)) {
                Flow::Continue => {}
                Flow::Pause => return Ok(ExtractOutcome::Paused),
                Flow::Cancel => return Ok(ExtractOutcome::Cancelled),
            }
        }
        Ok(ExtractOutcome::Completed)
    }
}

pub(super) struct Harness {
    pub _dir: tempfile::TempDir,
    pub svc: Arc<Services>,
    pub store: Arc<SqliteStore>,
    pub src_dir: std::path::PathBuf,
}

pub(super) fn harness(pdf: FakePdf) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::create(dir.path().join("data")).unwrap();
    let store = Arc::new(SqliteStore::new(Db::open_in_memory().unwrap()));
    let svc = Arc::new(Services {
        clock: Arc::new(FixedClock(Mutex::new(1_700_000_000_000))),
        collections: store.clone(),
        documents: store.clone(),
        pages: store.clone(),
        jobs: store.clone(),
        settings: store.clone(),
        onboarding: store.clone(),
        idempotency: store.clone(),
        files: Arc::new(LocalFileStore::new(paths).unwrap()),
        pdf: Arc::new(pdf),
        events: Arc::new(NullEvents),
        index: store.clone(),
        answers: store.clone(),
        providers: store.clone(),
        secrets: Arc::new(super::rag_tests::MemorySecrets::default()),
        provider_factory: Arc::new(super::rag_tests::FakeFactory),
        sign_in: Arc::new(super::rag_tests::NoSignIn),
    });
    let src_dir = dir.path().join("user files");
    std::fs::create_dir_all(&src_dir).unwrap();
    Harness { _dir: dir, svc, store, src_dir }
}

fn text_pages(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Page {i}: retrieval augmented generation evaluates evidence carefully.")).collect()
}

fn write_pdf(h: &Harness, name: &str, body: &str) -> std::path::PathBuf {
    let p = h.src_dir.join(name);
    std::fs::write(&p, format!("%PDF-1.7\n{body}")).unwrap();
    p
}

pub(super) fn run_job(h: &Harness, doc: &DocumentId) -> JobState {
    let mut job = h.svc.jobs.latest_for_entity(doc.as_str(), JobKind::Extract).unwrap().unwrap();
    job.state = JobState::Running;
    h.svc.jobs.save(&job).unwrap();
    let control = JobControl::new();
    extract::run_extract_job(&h.svc, &mut job, &control).unwrap();
    job.state
}

pub(super) fn import_one(h: &Harness, scheduler: &JobScheduler, coll: &CollectionId, name: &str, body: &str) -> DocumentId {
    let path = write_pdf(h, name, body);
    let rid = uuid::Uuid::new_v4().to_string();
    let report = import::import_files(&h.svc, scheduler, &rid, "tok", Some(&[path]), coll).unwrap();
    match &report.items[0].outcome {
        ImportOutcome::Imported { document_id, .. } => document_id.clone(),
        other => panic!("unexpected {other:?}"),
    }
}

/// A scheduler whose worker thread never receives work (we drive jobs manually).
pub(super) fn idle_scheduler(h: &Harness) -> Arc<JobScheduler> {
    let s = JobScheduler::start(Arc::clone(&h.svc));
    s.shutdown();
    s
}

#[test]
fn fresh_database_is_empty() {
    let h = harness(FakePdf { pages: vec![], title: None, pause_after: Default::default(), fail_with: None });
    assert!(library::list_collections(&h.svc).unwrap().is_empty());
    assert_eq!(h.svc.documents.count_live().unwrap(), 0);
    let ob = onboarding::get_progress(&h.svc).unwrap();
    assert!(ob.steps.iter().all(|s| s.state == StepState::Pending));
    assert!(h.store.db.fts5_available().unwrap(), "bundled SQLite must include FTS5");
}

#[test]
fn collections_validate_and_are_idempotent() {
    let h = harness(FakePdf { pages: vec![], title: None, pause_after: Default::default(), fail_with: None });
    let a = library::create_collection(&h.svc, "req-1", "  Retrieval   papers ").unwrap();
    assert_eq!(a.name, "Retrieval papers");
    // Same request id + same payload → same collection, no duplicate.
    let again = library::create_collection(&h.svc, "req-1", "Retrieval papers").unwrap();
    assert_eq!(a.id, again.id);
    // Same id, different payload → conflict.
    assert_eq!(library::create_collection(&h.svc, "req-1", "Other").unwrap_err().code, ErrorCode::Conflict);
    // Case-insensitive unique names.
    assert_eq!(library::create_collection(&h.svc, "req-2", "RETRIEVAL PAPERS").unwrap_err().code, ErrorCode::Conflict);
    assert_eq!(library::create_collection(&h.svc, "req-3", "   ").unwrap_err().code, ErrorCode::Validation);
    assert_eq!(library::list_collections(&h.svc).unwrap().len(), 1);
}

#[test]
fn import_extract_and_metadata_precedence() {
    let h = harness(FakePdf {
        pages: text_pages(3),
        title: Some("Dense Retrieval Revisited".into()),
        pause_after: Default::default(),
        fail_with: None,
    });
    let sched = idle_scheduler(&h);
    let coll = library::create_collection(&h.svc, "c1", "Thesis").unwrap();
    let doc = import_one(&h, &sched, &coll.id, "dense_retrieval.pdf", "a");

    let v = library::get_document(&h.svc, &doc).unwrap();
    assert_eq!(v.title, "dense retrieval");
    assert_eq!(v.title_origin, MetadataOrigin::FileName);
    assert_eq!(v.state, ExtractionState::Imported);
    assert!(h.svc.files.managed_exists(&doc));

    // User corrects the title before extraction finishes.
    library::update_metadata(&h.svc, &doc, &MetadataUpdate { title: Some("My Title".into()), ..Default::default() }).unwrap();

    assert_eq!(run_job(&h, &doc), JobState::Completed);
    let v = library::get_document(&h.svc, &doc).unwrap();
    assert_eq!(v.state, ExtractionState::TextReady);
    assert_eq!(v.page_count, Some(3));
    assert_eq!(v.text_page_count, 3);
    assert_eq!(v.title, "My Title", "user correction wins over PDF metadata");
    assert_eq!(v.authors.as_deref(), Some("Ada Lovelace"));
    assert_eq!(v.year, Some(2021));

    let c = library::list_collections(&h.svc).unwrap();
    assert_eq!((c[0].document_count, c[0].text_ready_count), (1, 1));
}

#[test]
fn duplicate_import_links_instead_of_copying() {
    let h = harness(FakePdf { pages: text_pages(1), title: None, pause_after: Default::default(), fail_with: None });
    let sched = idle_scheduler(&h);
    let a = library::create_collection(&h.svc, "c1", "A").unwrap();
    let b = library::create_collection(&h.svc, "c2", "B").unwrap();
    let doc = import_one(&h, &sched, &a.id, "x.pdf", "same bytes");
    let again = write_pdf(&h, "copy of x.pdf", "same bytes");
    let report = import::import_files(&h.svc, &sched, "r2", "t2", Some(&[again]), &b.id).unwrap();
    match &report.items[0].outcome {
        ImportOutcome::AlreadyInLibrary { document_id, added_to_collection } => {
            assert_eq!(document_id, &doc);
            assert!(added_to_collection);
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(h.svc.documents.count_live().unwrap(), 1);
    let page = library::list_documents(&h.svc, &DocumentQuery { collection_id: Some(b.id.clone()), ..Default::default() }).unwrap();
    assert_eq!(page.total, 1);
}

#[test]
fn rejects_non_pdf_and_expired_token() {
    let h = harness(FakePdf { pages: vec![], title: None, pause_after: Default::default(), fail_with: None });
    let sched = idle_scheduler(&h);
    let c = library::create_collection(&h.svc, "c1", "A").unwrap();
    let notes = h.src_dir.join("notes.pdf");
    std::fs::write(&notes, "not a pdf").unwrap();
    let r = import::import_files(&h.svc, &sched, "r1", "t", Some(&[notes]), &c.id).unwrap();
    assert!(matches!(r.items[0].outcome, ImportOutcome::Rejected { .. }));
    // Replay with the same request id returns the stored report even though the token is gone.
    let replay = import::import_files(&h.svc, &sched, "r1", "t", None, &c.id).unwrap();
    assert_eq!(replay.items.len(), 1);
    let err = import::import_files(&h.svc, &sched, "r9", "t", None, &c.id).unwrap_err();
    assert_eq!(err.code, ErrorCode::ImportTokenExpired);
}

#[test]
fn pause_checkpoints_and_resume_continues() {
    let pause_after = Arc::new(Mutex::new(Some(1)));
    let h = harness(FakePdf { pages: text_pages(5), title: None, pause_after: Arc::clone(&pause_after), fail_with: None });
    let sched = idle_scheduler(&h);
    let c = library::create_collection(&h.svc, "c1", "A").unwrap();
    let doc = import_one(&h, &sched, &c.id, "p.pdf", "p");

    assert_eq!(run_job(&h, &doc), JobState::Paused);
    let job = h.svc.jobs.latest_for_entity(doc.as_str(), JobKind::Extract).unwrap().unwrap();
    assert_eq!(job.completed_units, 2);
    assert_eq!(job.total_units, Some(5));
    assert!(job.checkpoint.as_deref().unwrap().contains("\"next_page\":2"));

    // Resume: the extractor is asked to start at page 2.
    let mut job = job;
    job.state = JobState::Queued;
    h.svc.jobs.save(&job).unwrap();
    *pause_after.lock().unwrap() = None;
    assert_eq!(run_job(&h, &doc), JobState::Completed);
    let v = library::get_document(&h.svc, &doc).unwrap();
    assert_eq!(v.text_page_count, 5);
    assert_eq!(v.state, ExtractionState::TextReady);
}

#[test]
fn scanned_pdf_needs_ocr() {
    let h = harness(FakePdf { pages: vec![String::new(); 4], title: None, pause_after: Default::default(), fail_with: None });
    let sched = idle_scheduler(&h);
    let c = library::create_collection(&h.svc, "c1", "A").unwrap();
    let doc = import_one(&h, &sched, &c.id, "scan.pdf", "s");
    run_job(&h, &doc);
    let v = library::get_document(&h.svc, &doc).unwrap();
    assert_eq!(v.state, ExtractionState::NeedsOcr);
    assert_eq!(v.text_page_count, 0);
}

#[test]
fn password_protected_fails_with_reason() {
    let h = harness(FakePdf {
        pages: vec![],
        title: None,
        pause_after: Default::default(),
        fail_with: Some(FailureKind::PasswordProtected),
    });
    let sched = idle_scheduler(&h);
    let c = library::create_collection(&h.svc, "c1", "A").unwrap();
    let doc = import_one(&h, &sched, &c.id, "locked.pdf", "l");
    assert_eq!(run_job(&h, &doc), JobState::Failed);
    let v = library::get_document(&h.svc, &doc).unwrap();
    assert_eq!(v.state, ExtractionState::Failed);
    assert_eq!(v.failure_kind, Some(FailureKind::PasswordProtected));
}

#[test]
fn delete_tombstones_then_purges() {
    let h = harness(FakePdf { pages: text_pages(2), title: None, pause_after: Default::default(), fail_with: None });
    let sched = idle_scheduler(&h);
    let c = library::create_collection(&h.svc, "c1", "A").unwrap();
    let doc = import_one(&h, &sched, &c.id, "d.pdf", "d");
    run_job(&h, &doc);

    delete::delete_document(&h.svc, &sched, &doc).unwrap();
    assert!(library::get_document(&h.svc, &doc).is_err(), "hidden immediately");
    assert_eq!(library::list_collections(&h.svc).unwrap()[0].document_count, 0);

    let mut job = h.svc.jobs.latest_for_entity(doc.as_str(), JobKind::DeleteDocument).unwrap().unwrap();
    job.state = JobState::Running;
    delete::run_delete_job(&h.svc, &mut job).unwrap();
    assert!(!h.svc.files.managed_exists(&doc));
    let rec = h.svc.documents.get(&doc).unwrap().unwrap();
    assert_eq!(rec.state, ExtractionState::Deleted);
    // Re-importing the same bytes later creates a fresh document.
    let again = import_one(&h, &sched, &c.id, "d2.pdf", "d");
    assert_ne!(again, doc);
}

#[test]
fn recovery_promotes_staged_files_and_requeues() {
    let h = harness(FakePdf { pages: text_pages(1), title: None, pause_after: Default::default(), fail_with: None });
    let sched = idle_scheduler(&h);
    let c = library::create_collection(&h.svc, "c1", "A").unwrap();
    let doc = import_one(&h, &sched, &c.id, "r.pdf", "r");
    // Simulate a crash after the DB commit but before promotion.
    let lib = h.svc.files.resolve_managed(&h.svc.files.managed_rel_path(&doc)).unwrap();
    let staged = lib.parent().unwrap().parent().unwrap().join("staging").join(lib.file_name().unwrap());
    std::fs::rename(&lib, &staged).unwrap();
    // And an orphaned staging file.
    std::fs::write(staged.parent().unwrap().join("orphan.pdf"), "%PDF-").unwrap();
    // And a job left running.
    let mut job = h.svc.jobs.latest_for_entity(doc.as_str(), JobKind::Extract).unwrap().unwrap();
    job.state = JobState::Running;
    h.svc.jobs.save(&job).unwrap();

    let report = recovery::recover_on_startup(&h.svc, &sched).unwrap();
    assert_eq!(report.promoted_files, 1);
    assert_eq!(report.orphan_staging_removed, 1);
    assert_eq!(report.resumed_jobs, 1);
    assert!(h.svc.files.managed_exists(&doc));
    let job = h.svc.jobs.get(&job.id).unwrap().unwrap();
    assert_eq!(job.state, JobState::Queued);
}

#[test]
fn onboarding_requires_real_actions() {
    let h = harness(FakePdf { pages: text_pages(1), title: None, pause_after: Default::default(), fail_with: None });
    let sched = idle_scheduler(&h);
    assert!(onboarding::update_step(&h.svc, StepKey::Collection, StepAction::Complete, None).is_err());
    let c = library::create_collection(&h.svc, "c1", "Mine").unwrap();
    let v = onboarding::update_step(&h.svc, StepKey::Collection, StepAction::Complete, Some(c.id.as_str())).unwrap();
    assert_eq!(v.steps.iter().find(|s| s.key == StepKey::Collection).unwrap().state, StepState::Completed);
    // Models cannot be marked complete by hand; it completes when a model answers a test.
    assert!(onboarding::update_step(&h.svc, StepKey::Models, StepAction::Complete, None).is_err());
    onboarding::update_step(&h.svc, StepKey::Models, StepAction::Skip, None).unwrap();
    // Readiness needs a paper with extracted text.
    let doc = import_one(&h, &sched, &c.id, "o.pdf", "o");
    assert!(onboarding::update_step(&h.svc, StepKey::Readiness, StepAction::Complete, None).is_err());
    run_job(&h, &doc);
    onboarding::update_step(&h.svc, StepKey::Readiness, StepAction::Complete, None).unwrap();
    let v = onboarding::restart(&h.svc).unwrap();
    assert!(v.steps.iter().all(|s| s.state == StepState::Pending));
}
