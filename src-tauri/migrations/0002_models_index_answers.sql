-- Papryvio schema v2: model providers, index generations, chunks + FTS5, embeddings,
-- conversations with attempts/evidence/citations, comparisons.

-- Jobs gain indexing and embedding kinds (SQLite cannot alter a CHECK: rebuild the table).
CREATE TABLE jobs_v2 (
    id               TEXT PRIMARY KEY,
    kind             TEXT NOT NULL CHECK (kind IN ('extract','index','embed','delete_document')),
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
INSERT INTO jobs_v2 SELECT * FROM jobs;
DROP TABLE jobs;
ALTER TABLE jobs_v2 RENAME TO jobs;
CREATE INDEX jobs_entity ON jobs (entity_id);
CREATE INDEX jobs_state ON jobs (state);

-- Connected model providers. Secrets live in the OS credential store; only a reference is here.
CREATE TABLE providers (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL CHECK (kind IN ('ollama','openai_compatible','anthropic','chatgpt','github_copilot')),
    label        TEXT NOT NULL,
    base_url     TEXT NOT NULL,
    secret_ref   TEXT,
    external_id  TEXT,
    config       TEXT NOT NULL DEFAULT '{}',
    is_local     INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE TABLE index_generations (
    id                   TEXT PRIMARY KEY,
    document_id          TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    extraction_revision  INTEGER NOT NULL,
    chunker_version      INTEGER NOT NULL,
    state                TEXT NOT NULL CHECK (state IN ('building','published','superseded','failed')),
    chunk_count          INTEGER NOT NULL DEFAULT 0,
    created_at           INTEGER NOT NULL
);
CREATE INDEX index_generations_doc ON index_generations (document_id);

CREATE TABLE chunks (
    id             INTEGER PRIMARY KEY,
    generation_id  TEXT NOT NULL REFERENCES index_generations(id) ON DELETE CASCADE,
    document_id    TEXT NOT NULL,
    page_index     INTEGER NOT NULL,
    chunk_index    INTEGER NOT NULL,
    text           TEXT NOT NULL,
    -- Normalized copy used only by the keyword index (Arabic letter forms, case).
    search_text    TEXT NOT NULL,
    start_offset   INTEGER NOT NULL,
    end_offset     INTEGER NOT NULL,
    token_estimate INTEGER NOT NULL
);
CREATE INDEX chunks_generation ON chunks (generation_id);
CREATE INDEX chunks_document ON chunks (document_id, page_index);

-- External-content FTS5 index kept in sync by triggers.
CREATE VIRTUAL TABLE chunks_fts USING fts5(
    search_text,
    content='chunks',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, search_text) VALUES (new.id, new.search_text);
END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, search_text) VALUES ('delete', old.id, old.search_text);
END;
CREATE TRIGGER chunks_au AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, search_text) VALUES ('delete', old.id, old.search_text);
    INSERT INTO chunks_fts(rowid, search_text) VALUES (new.id, new.search_text);
END;

-- Normalized float32 vectors. The fingerprint identifies provider, model, digest and dimensions.
CREATE TABLE chunk_embeddings (
    chunk_id     INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    fingerprint  TEXT NOT NULL,
    dimensions   INTEGER NOT NULL,
    vector       BLOB NOT NULL,
    PRIMARY KEY (chunk_id, fingerprint)
);
CREATE INDEX chunk_embeddings_fp ON chunk_embeddings (fingerprint);

ALTER TABLE documents ADD COLUMN embedding_fingerprint TEXT;

CREATE TABLE conversations (
    id             TEXT PRIMARY KEY,
    title          TEXT NOT NULL,
    collection_id  TEXT REFERENCES collections(id) ON DELETE SET NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE messages (
    id               TEXT PRIMARY KEY,
    conversation_id  TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role             TEXT NOT NULL CHECK (role IN ('user','assistant')),
    text             TEXT NOT NULL,
    state            TEXT NOT NULL,
    created_at       INTEGER NOT NULL
);
CREATE INDEX messages_conversation ON messages (conversation_id, created_at);

CREATE TABLE generation_attempts (
    id                 TEXT PRIMARY KEY,
    message_id         TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    request_id         TEXT NOT NULL,
    model_fingerprint  TEXT NOT NULL,
    prompt_version     INTEGER NOT NULL,
    scope_snapshot     TEXT NOT NULL,
    scope_summary      TEXT,
    retrieval_config   TEXT NOT NULL,
    state              TEXT NOT NULL CHECK (state IN ('retrieving','generating','validating','answered','partial','no_evidence','failed','cancelled')),
    result             TEXT,
    error_kind         TEXT,
    started_at         INTEGER NOT NULL,
    finished_at        INTEGER,
    timing             TEXT
);
CREATE INDEX generation_attempts_message ON generation_attempts (message_id);

CREATE TABLE retrieved_evidence (
    attempt_id        TEXT NOT NULL REFERENCES generation_attempts(id) ON DELETE CASCADE,
    evidence_id       TEXT NOT NULL,
    chunk_id          INTEGER,
    document_id       TEXT NOT NULL,
    generation_id     TEXT NOT NULL,
    rank              INTEGER NOT NULL,
    page_index        INTEGER NOT NULL,
    start_offset      INTEGER NOT NULL,
    end_offset        INTEGER NOT NULL,
    document_title    TEXT NOT NULL,
    excerpt_snapshot  TEXT NOT NULL,
    PRIMARY KEY (attempt_id, evidence_id)
);

CREATE TABLE citations (
    id            TEXT PRIMARY KEY,
    attempt_id    TEXT NOT NULL REFERENCES generation_attempts(id) ON DELETE CASCADE,
    claim_index   INTEGER NOT NULL,
    evidence_id   TEXT NOT NULL,
    document_id   TEXT NOT NULL,
    page_index    INTEGER NOT NULL,
    -- Character span of the quoted passage within the page text, when it was located.
    span_start    INTEGER,
    span_end      INTEGER,
    quote         TEXT
);
CREATE INDEX citations_attempt ON citations (attempt_id);

CREATE TABLE comparisons (
    id                  TEXT PRIMARY KEY,
    request_id          TEXT NOT NULL,
    selected_documents  TEXT NOT NULL,
    selected_rows       TEXT NOT NULL,
    model_fingerprint   TEXT NOT NULL,
    state               TEXT NOT NULL CHECK (state IN ('running','completed','failed','cancelled')),
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);

CREATE TABLE comparison_cells (
    id             TEXT PRIMARY KEY,
    comparison_id  TEXT NOT NULL REFERENCES comparisons(id) ON DELETE CASCADE,
    document_id    TEXT NOT NULL,
    row_key        TEXT NOT NULL,
    row_label      TEXT NOT NULL,
    text           TEXT,
    evidence       TEXT NOT NULL DEFAULT '[]',
    status         TEXT NOT NULL CHECK (status IN ('pending','supported','partial','evidence_not_found','failed')),
    searched       TEXT,
    updated_at     INTEGER NOT NULL,
    UNIQUE (comparison_id, document_id, row_key)
);
