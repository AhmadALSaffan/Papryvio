//! ImportDocuments use case.
use std::path::PathBuf;

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::jobs::JobScheduler;
use crate::application::ports::{AppEvent, NewDocument, NewJob};
use crate::application::views::{ImportItemResult, ImportOutcome, ImportReport, RejectReason};
use crate::application::{payload_hash, Services};
use crate::domain::document::title_from_file_name;
use crate::domain::ids::{AttemptId, CollectionId, DocumentId, JobId};
use crate::domain::job::JobKind;

/// Largest PDF accepted for import.
pub const MAX_PDF_BYTES: u64 = 512 * 1024 * 1024;
/// Upper bound of files per import request.
pub const MAX_FILES_PER_IMPORT: usize = 200;

const COMMAND: &str = "import_documents";

pub fn import_files(
    svc: &Services,
    scheduler: &JobScheduler,
    request_id: &str,
    token: &str,
    paths: Option<&[PathBuf]>,
    collection_id: &CollectionId,
) -> AppResult<ImportReport> {
    let hash = payload_hash(&format!("{token}|{collection_id}"));
    if let Some((cmd, stored_hash, response)) = svc.idempotency.get(request_id)? {
        if cmd == COMMAND && stored_hash == hash {
            return serde_json::from_str(&response).map_err(|e| AppError::internal(e.to_string()));
        }
        return Err(AppError::conflict("request id reused with a different payload"));
    }
    // Tokens are single-use; a replay with the same request id was answered above.
    let paths = paths.ok_or_else(|| AppError::new(ErrorCode::ImportTokenExpired, "unknown or expired import token"))?;
    if paths.len() > MAX_FILES_PER_IMPORT {
        return Err(AppError::validation("too many files in one import"));
    }
    if !svc.collections.exists(collection_id)? {
        return Err(AppError::not_found("collection"));
    }

    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let outcome = match import_one(svc, scheduler, path, &file_name, collection_id) {
            Ok(o) => o,
            Err(e) if e.code == ErrorCode::FileTooLarge => ImportOutcome::Rejected { reason: RejectReason::TooLarge },
            Err(e) if e.code == ErrorCode::FileEmpty => ImportOutcome::Rejected { reason: RejectReason::Empty },
            Err(e) if e.code == ErrorCode::NotAPdf => ImportOutcome::Rejected { reason: RejectReason::NotAPdf },
            Err(e) if e.code == ErrorCode::FileAccess => {
                tracing::warn!("import: unreadable source: {}", e.detail);
                ImportOutcome::Rejected { reason: RejectReason::Unreadable }
            }
            Err(e) => return Err(e),
        };
        items.push(ImportItemResult { file_name, outcome });
    }

    let report = ImportReport { collection_id: collection_id.clone(), items };
    let response = serde_json::to_string(&report).map_err(|e| AppError::internal(e.to_string()))?;
    svc.idempotency.put(request_id, COMMAND, &hash, &response, svc.now())?;
    svc.events.emit(AppEvent::LibraryChanged);
    Ok(report)
}

fn import_one(
    svc: &Services,
    scheduler: &JobScheduler,
    path: &std::path::Path,
    file_name: &str,
    collection_id: &CollectionId,
) -> AppResult<ImportOutcome> {
    let info = svc.files.inspect_source(path, MAX_PDF_BYTES)?;
    if info.size == 0 {
        return Err(AppError::new(ErrorCode::FileEmpty, "empty file"));
    }
    if !info.looks_like_pdf {
        return Err(AppError::new(ErrorCode::NotAPdf, "missing %PDF- signature"));
    }

    // Same bytes already in the library: link instead of copying and re-extracting.
    if let Some(existing) = svc.documents.find_live_by_hash(&info.sha256)? {
        let added = svc.documents.link(collection_id, &existing, svc.now())?;
        return Ok(ImportOutcome::AlreadyInLibrary { document_id: existing, added_to_collection: added });
    }

    let doc_id = DocumentId::new();
    // 1) copy into staging (the user's original file is never modified)
    svc.files.stage_copy(path, &doc_id)?;
    // 2) one transaction: document row + collection link + queued extraction job
    let job = NewJob {
        id: JobId::new(),
        kind: JobKind::Extract,
        entity_id: doc_id.to_string(),
        attempt_id: AttemptId::new(),
        stage: "queued".into(),
        now: svc.now(),
    };
    let new_doc = NewDocument {
        id: doc_id.clone(),
        content_hash: info.sha256,
        managed_file: svc.files.managed_rel_path(&doc_id),
        original_file_name: file_name.to_string(),
        file_size: info.size,
        title: title_from_file_name(file_name),
        now: svc.now(),
    };
    if let Err(e) = svc.documents.register_import(&new_doc, collection_id, &job) {
        svc.files.remove_staged(&doc_id);
        return Err(e);
    }
    // 3) promote staging → library. A crash between 2 and 3 is repaired by recovery.
    svc.files.promote(&doc_id)?;
    scheduler.enqueue(job.id.clone());
    Ok(ImportOutcome::Imported { document_id: doc_id, job_id: job.id })
}
