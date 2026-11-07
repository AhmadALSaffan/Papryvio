-- Papryvio schema v1: library, documents, extraction, jobs, settings, onboarding.
-- Timestamps are unix epoch milliseconds (UTC). page_index is zero-based.

CREATE TABLE collections (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
CREATE UNIQUE INDEX collections_name_unique ON collections (lower(name));

CREATE TABLE documents (
    id                    TEXT PRIMARY KEY,
    content_hash          TEXT NOT NULL,
    managed_file          TEXT,
    original_file_name    TEXT NOT NULL,
    file_size             INTEGER NOT NULL,
    title                 TEXT NOT NULL,
    title_origin          TEXT NOT NULL CHECK (title_origin IN ('pdf_metadata','file_name','user')),
    authors               TEXT,
    authors_origin        TEXT CHECK (authors_origin IN ('pdf_metadata','user')),
    year                  INTEGER,
    year_origin           TEXT CHECK (year_origin IN ('pdf_metadata','user')),
    page_count            INTEGER,
    extraction_state      TEXT NOT NULL CHECK (extraction_state IN
                            ('imported','extracting','text_ready','needs_ocr','indexing','ready','failed','deleting','deleted')),
    extraction_revision   INTEGER NOT NULL DEFAULT 0,
    failure_kind          TEXT,
    text_page_count       INTEGER NOT NULL DEFAULT 0,
    low_text_page_count   INTEGER NOT NULL DEFAULT 0,
    lexical_ready         INTEGER NOT NULL DEFAULT 0,
    semantic_ready        INTEGER NOT NULL DEFAULT 0,
    active_generation_id  TEXT,
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL,
    deleted_at            INTEGER
);
CREATE UNIQUE INDEX documents_live_hash ON documents (content_hash) WHERE deleted_at IS NULL;
CREATE INDEX documents_state ON documents (extraction_state);

CREATE TABLE collection_documents (
    collection_id  TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    document_id    TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    added_at       INTEGER NOT NULL,
    PRIMARY KEY (collection_id, document_id)
);
CREATE INDEX collection_documents_doc ON collection_documents (document_id);

CREATE TABLE document_pages (
    id                   INTEGER PRIMARY KEY,
    document_id          TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    extraction_revision  INTEGER NOT NULL,
    page_index           INTEGER NOT NULL CHECK (page_index >= 0),
    printed_label        TEXT,
    width                REAL NOT NULL,
    height               REAL NOT NULL,
    rotation             INTEGER NOT NULL DEFAULT 0,
    text                 TEXT NOT NULL,
    char_count           INTEGER NOT NULL,
    quality_flags        TEXT NOT NULL DEFAULT '',
    UNIQUE (document_id, extraction_revision, page_index)
);

CREATE TABLE text_spans (
    id           INTEGER PRIMARY KEY,
    page_id      INTEGER NOT NULL REFERENCES document_pages(id) ON DELETE CASCADE,
    start_offset INTEGER NOT NULL,
    end_offset   INTEGER NOT NULL,
    -- PDF user-space coordinates (origin bottom-left), unrotated page.
    left         REAL NOT NULL,
    bottom       REAL NOT NULL,
    right        REAL NOT NULL,
    top          REAL NOT NULL
);
CREATE INDEX text_spans_page ON text_spans (page_id);

CREATE TABLE jobs (
    id               TEXT PRIMARY KEY,
    kind             TEXT NOT NULL CHECK (kind IN ('extract','delete_document')),
    entity_id        TEXT NOT NULL,
    attempt_id       TEXT NOT NULL,
    state            TEXT NOT NULL CHECK (state IN ('queued','running','paused','completed','failed','cancelled')),
    stage            TEXT NOT NULL DEFAULT '',
    checkpoint       TEXT,
    completed_units  INTEGER NOT NULL DEFAULT 0,
    total_units      INTEGER,
    error_kind       TEXT,
    error_message    TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);
CREATE INDEX jobs_entity ON jobs (entity_id);
CREATE INDEX jobs_state ON jobs (state);

CREATE TABLE settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE onboarding_progress (
    flow_version       INTEGER NOT NULL,
    step_key           TEXT NOT NULL,
    state              TEXT NOT NULL CHECK (state IN ('pending','completed','skipped')),
    related_entity_id  TEXT,
    updated_at         INTEGER NOT NULL,
    PRIMARY KEY (flow_version, step_key)
);

CREATE TABLE idempotency_keys (
    request_id    TEXT PRIMARY KEY,
    command       TEXT NOT NULL,
    payload_hash  TEXT NOT NULL,
    response      TEXT NOT NULL,
    created_at    INTEGER NOT NULL
);
