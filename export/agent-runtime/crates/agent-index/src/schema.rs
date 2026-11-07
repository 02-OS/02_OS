pub const CURRENT_SCHEMA_VERSION: i32 = 2;

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
