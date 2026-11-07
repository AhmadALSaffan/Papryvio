//! Collections and documents in the user's library.
use crate::application::error::{AppError, AppResult};
use crate::application::ports::{AppEvent, UserMetadata};
use crate::application::views::{CollectionView, DocumentListPage, DocumentQuery, DocumentView, MetadataUpdate};
use crate::application::{payload_hash, settings, Services};
use crate::domain::collection::{Collection, CollectionName};
use crate::domain::document::{validate_user_text, validate_user_year};
use crate::domain::ids::{CollectionId, DocumentId};

const CREATE_COLLECTION: &str = "create_collection";

pub fn list_collections(svc: &Services) -> AppResult<Vec<CollectionView>> {
    svc.collections.list()
}

pub fn create_collection(svc: &Services, request_id: &str, raw_name: &str) -> AppResult<CollectionView> {
    let name = CollectionName::parse(raw_name)?;
    let hash = payload_hash(name.as_str());
    if let Some((cmd, stored, response)) = svc.idempotency.get(request_id)? {
        if cmd == CREATE_COLLECTION && stored == hash {
            let id = CollectionId::parse(&response).ok_or_else(|| AppError::internal("bad stored id"))?;
            return svc.collections.get(&id)?.ok_or_else(|| AppError::not_found("collection"));
        }
        return Err(AppError::conflict("request id reused with a different payload"));
    }
    let now = svc.now();
    let collection = Collection { id: CollectionId::new(), name: name.as_str().to_string(), created_at: now, updated_at: now };
    svc.collections.create(&collection)?;
    svc.idempotency.put(request_id, CREATE_COLLECTION, &hash, collection.id.as_str(), now)?;
    svc.events.emit(AppEvent::LibraryChanged);
    svc.collections.get(&collection.id)?.ok_or_else(|| AppError::internal("collection vanished"))
}

pub fn rename_collection(svc: &Services, id: &CollectionId, raw_name: &str) -> AppResult<CollectionView> {
    let name = CollectionName::parse(raw_name)?;
    svc.collections.rename(id, name.as_str(), svc.now())?;
    svc.events.emit(AppEvent::LibraryChanged);
    svc.collections.get(id)?.ok_or_else(|| AppError::not_found("collection"))
}

/// Deletes the collection and its links. Papers stay in the library.
pub fn delete_collection(svc: &Services, id: &CollectionId) -> AppResult<()> {
    if !svc.collections.exists(id)? {
        return Err(AppError::not_found("collection"));
    }
    svc.collections.delete(id)?;
    let current = settings::get_settings(svc)?;
    if current.active_collection_id.as_ref() == Some(id) {
        settings::clear_active_collection(svc)?;
    }
    svc.events.emit(AppEvent::LibraryChanged);
    Ok(())
}

pub fn list_documents(svc: &Services, query: &DocumentQuery) -> AppResult<DocumentListPage> {
    if let Some(f) = &query.filter {
        if f.chars().count() > 200 {
            return Err(AppError::validation("filter too long"));
        }
    }
    svc.documents.list_views(query)
}

pub fn get_document(svc: &Services, id: &DocumentId) -> AppResult<DocumentView> {
    svc.documents.view(id)?.ok_or_else(|| AppError::not_found("document"))
}

/// User corrections always win over PDF metadata, including after re-extraction.
pub fn update_metadata(svc: &Services, id: &DocumentId, update: &MetadataUpdate) -> AppResult<DocumentView> {
    let title = match update.title.as_deref() {
        Some(t) => Some(validate_user_text(Some(t), "title", 400)?.ok_or_else(|| AppError::validation("title must not be empty"))?),
        None => None,
    };
    let authors = match update.authors.as_deref() {
        Some(a) => Some(validate_user_text(Some(a), "authors", 300)?),
        None => None,
    };
    let year = if update.clear_year {
        Some(None)
    } else if update.year.is_some() {
        Some(validate_user_year(update.year)?)
    } else {
        None
    };
    let doc = svc.documents.get(id)?.ok_or_else(|| AppError::not_found("document"))?;
    if doc.deleted_at.is_some() {
        return Err(AppError::not_found("document"));
    }
    svc.documents.update_user_metadata(id, &UserMetadata { title, authors, year }, svc.now())?;
    svc.events.emit(AppEvent::LibraryChanged);
    get_document(svc, id)
}

pub fn add_to_collection(svc: &Services, collection: &CollectionId, id: &DocumentId) -> AppResult<bool> {
    if !svc.collections.exists(collection)? {
        return Err(AppError::not_found("collection"));
    }
    let doc = svc.documents.get(id)?.ok_or_else(|| AppError::not_found("document"))?;
    if doc.deleted_at.is_some() {
        return Err(AppError::not_found("document"));
    }
    let added = svc.documents.link(collection, id, svc.now())?;
    svc.events.emit(AppEvent::LibraryChanged);
    Ok(added)
}

/// Removes only the link; the paper stays in the library and in other collections.
pub fn remove_from_collection(svc: &Services, collection: &CollectionId, id: &DocumentId) -> AppResult<bool> {
    let removed = svc.documents.unlink(collection, id)?;
    svc.events.emit(AppEvent::LibraryChanged);
    Ok(removed)
}
