pub const CURRENT_SCHEMA_VERSION: i32 = 5;

/// Rebuild `files`, replace the FTS table, and add the v5 bookkeeping tables.
/// Run only when `schema_version` is missing or below 5. It is not idempotent.
pub const SCHEMA_V5_SQL: &str = r#"
CREATE TABLE files_v5 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    git_blob_id TEXT,
    size_bytes INTEGER NOT NULL,
    language TEXT NOT NULL,
    kind TEXT NOT NULL,
    is_test INTEGER NOT NULL DEFAULT 0,
    is_doc INTEGER NOT NULL DEFAULT 0,
    indexed_at TEXT NOT NULL,
    raw_fingerprint TEXT,
    redacted_fingerprint TEXT,
    UNIQUE (repo_id, relative_path),
    FOREIGN KEY (repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

INSERT INTO files_v5 (
    id, repo_id, relative_path, file_hash, git_blob_id, size_bytes,
    language, kind, is_test, is_doc, indexed_at, raw_fingerprint, redacted_fingerprint
)
SELECT
    id, repo_id, relative_path, file_hash, git_blob_id, size_bytes,
    language, kind, is_test, is_doc, indexed_at, NULL, NULL
FROM files;

DROP TABLE files;
ALTER TABLE files_v5 RENAME TO files;

CREATE INDEX idx_files_repo ON files (repo_id);
CREATE INDEX idx_files_path ON files (repo_id, relative_path);
CREATE INDEX idx_files_kind ON files (kind);
CREATE INDEX idx_files_language ON files (language);

DROP TABLE IF EXISTS files_fts;
CREATE VIRTUAL TABLE files_fts USING fts5(
    repo_id UNINDEXED,
    relative_path,
    content,
    tokenize = 'porter unicode61'
);

ALTER TABLE symbols ADD COLUMN raw_fingerprint TEXT;
ALTER TABLE symbols ADD COLUMN redacted_fingerprint TEXT;
UPDATE symbols
SET redacted_fingerprint = fingerprint
WHERE redacted_fingerprint IS NULL;

CREATE INDEX IF NOT EXISTS idx_refs_source_symbol
    ON symbol_references (repo_id, source_symbol_id);

CREATE TABLE IF NOT EXISTS actors (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN (
        'person', 'daemon', 'mcp-client', 'agt', 'extension', 'unknown'
    )),
    display_name TEXT NOT NULL,
    peer_uid INTEGER,
    created_at TEXT NOT NULL,
    UNIQUE (repo_id, kind, display_name),
    FOREIGN KEY (repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tool_invocations (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    tool TEXT NOT NULL,
    args_hash TEXT NOT NULL,
    args_canonical TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    is_error INTEGER NOT NULL DEFAULT 0,
    error_code TEXT,
    warning_json TEXT,
    FOREIGN KEY (repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE,
    FOREIGN KEY (actor_id) REFERENCES actors(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_invocations_repo_time
    ON tool_invocations (repo_id, started_at);

CREATE TABLE IF NOT EXISTS approvals (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    agt_run_id TEXT,
    agt_command_id TEXT,
    args_hash TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('allow', 'deny')),
    approver TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('agt-cli', 'gnome', '02-cli')),
    created_at TEXT NOT NULL,
    FOREIGN KEY (repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_approvals_hash ON approvals (repo_id, args_hash);

CREATE TABLE IF NOT EXISTS evidence_packages (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    agt_run_id TEXT NOT NULL,
    task_type TEXT NOT NULL CHECK (task_type IN ('repair', 'feature', 'refactor', 'ask', 'plan')),
    interaction TEXT NOT NULL CHECK (interaction IN ('agent', 'ask', 'plan')),
    outcome TEXT NOT NULL,
    contract_digest TEXT NOT NULL DEFAULT '',
    verification_digest TEXT NOT NULL DEFAULT '',
    package_digest TEXT NOT NULL,
    package_json TEXT NOT NULL,
    head_commit TEXT,
    created_at TEXT NOT NULL,
    UNIQUE (repo_id, agt_run_id),
    FOREIGN KEY (repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS memory_index (
    memory_id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    status TEXT NOT NULL,
    confidence REAL NOT NULL,
    author_kind TEXT NOT NULL DEFAULT 'unknown',
    updated_at TEXT NOT NULL,
    jsonl_offset INTEGER,
    FOREIGN KEY (repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_memory_status ON memory_index (repo_id, status);

ALTER TABLE completed_index_state ADD COLUMN phase TEXT NOT NULL DEFAULT 'complete';
"#;

/// A repository observation is not proof of a successful index. Legacy rows
/// deliberately acquire no completion marker until their next full sync.
pub const SCHEMA_V4_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS completed_index_state (
    repo_id TEXT PRIMARY KEY,
    head_commit TEXT,
    fingerprint TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);
"#;

/// Schema written by runtime versions that only knew the file/symbol catalog.
pub const SCHEMA_V1_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS repositories (
    repo_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    root_path TEXT NOT NULL,
    head_commit TEXT,
    branch TEXT,
    indexed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id TEXT NOT NULL,
    relative_path TEXT NOT NULL UNIQUE,
    file_hash TEXT NOT NULL,
    git_blob_id TEXT,
    size_bytes INTEGER NOT NULL,
    language TEXT NOT NULL,
    kind TEXT NOT NULL,
    is_test INTEGER NOT NULL DEFAULT 0,
    is_doc INTEGER NOT NULL DEFAULT 0,
    indexed_at TEXT NOT NULL,
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_files_repo ON files (repo_id);
CREATE INDEX IF NOT EXISTS idx_files_path ON files (relative_path);
CREATE INDEX IF NOT EXISTS idx_files_kind ON files (kind);
CREATE INDEX IF NOT EXISTS idx_files_language ON files (language);

CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
    relative_path,
    content,
    tokenize = 'porter unicode61'
);

CREATE TABLE IF NOT EXISTS symbols (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    file_path TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    signature TEXT,
    doc_comment TEXT,
    fingerprint TEXT NOT NULL,
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols (name);
CREATE INDEX IF NOT EXISTS idx_symbols_repo ON symbols (repo_id);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols (file_path);

CREATE TABLE IF NOT EXISTS symbol_references (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id TEXT NOT NULL,
    source_file TEXT NOT NULL,
    source_symbol_name TEXT,
    target_name TEXT NOT NULL,
    target_symbol_id TEXT,
    kind TEXT NOT NULL,
    line_number INTEGER NOT NULL,
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_refs_target ON symbol_references (target_name);
CREATE INDEX IF NOT EXISTS idx_refs_source ON symbol_references (source_file);
CREATE INDEX IF NOT EXISTS idx_refs_kind ON symbol_references (kind);
CREATE INDEX IF NOT EXISTS idx_refs_repo ON symbol_references (repo_id);
"#;

/// Additive tables. Applied with CREATE IF NOT EXISTS so a v1 database keeps its rows.
pub const SCHEMA_V2_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS datasets (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(repo_id, name),
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS datapoints (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    dataset_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    kind TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    content TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    commit_id TEXT,
    path TEXT NOT NULL DEFAULT '',
    start_byte INTEGER,
    end_byte INTEGER,
    symbol_fingerprint TEXT,
    confidence REAL NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE,
    FOREIGN KEY(dataset_id) REFERENCES datasets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_datapoints_lookup
    ON datapoints (dataset_id, kind, path, version);

CREATE TABLE IF NOT EXISTS graph_nodes (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    dataset_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    datapoint_id TEXT,
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE,
    FOREIGN KEY(dataset_id) REFERENCES datasets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_graph_nodes_dataset ON graph_nodes (dataset_id, kind);

CREATE TABLE IF NOT EXISTS graph_edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id TEXT NOT NULL,
    dataset_id TEXT NOT NULL,
    src_id TEXT NOT NULL,
    dst_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT,
    UNIQUE(dataset_id, src_id, dst_id, kind),
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE,
    FOREIGN KEY(dataset_id) REFERENCES datasets(id) ON DELETE CASCADE,
    FOREIGN KEY(src_id) REFERENCES graph_nodes(id) ON DELETE CASCADE,
    FOREIGN KEY(dst_id) REFERENCES graph_nodes(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_graph_edges_src ON graph_edges (dataset_id, src_id, kind);
CREATE INDEX IF NOT EXISTS idx_graph_edges_dst ON graph_edges (dataset_id, dst_id);

CREATE TABLE IF NOT EXISTS node_sets (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    name TEXT NOT NULL,
    UNIQUE(repo_id, name),
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS node_set_members (
    node_set_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    PRIMARY KEY(node_set_id, node_id),
    FOREIGN KEY(node_set_id) REFERENCES node_sets(id) ON DELETE CASCADE,
    FOREIGN KEY(node_id) REFERENCES graph_nodes(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    dataset_id TEXT NOT NULL,
    label TEXT,
    ephemeral INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    FOREIGN KEY(repo_id) REFERENCES repositories(repo_id) ON DELETE CASCADE,
    FOREIGN KEY(dataset_id) REFERENCES datasets(id) ON DELETE CASCADE
);
"#;
