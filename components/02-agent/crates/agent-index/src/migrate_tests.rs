use crate::db::IndexDatabase;
use crate::schema::{CURRENT_SCHEMA_VERSION, SCHEMA_V1_SQL, SCHEMA_V2_SQL, SCHEMA_V4_SQL};
use agent_core::error::AgentError;
use rusqlite::Connection;
use std::fs;

#[test]
fn v1_database_upgrades_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(SCHEMA_V1_SQL).unwrap();
    conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
        .unwrap();
    conn.execute(
        "INSERT INTO repositories (repo_id, name, root_path, head_commit, branch, indexed_at)
         VALUES ('repo', 'demo', '/tmp/demo', NULL, 'main', '2020-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files (
            repo_id, relative_path, file_hash, git_blob_id, size_bytes, language, kind,
            is_test, is_doc, indexed_at
         ) VALUES ('repo', 'src/main.rs', 'abc', NULL, 4, 'rust', 'source', 0, 0, '2020-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    drop(conn);

    let db = IndexDatabase::open(&db_path).unwrap();
    assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    let files = db.list_files(&agent_core::RepoId::new("repo")).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].relative_path, "src/main.rs");
    assert_eq!(files[0].language, agent_core::Language::Rust);
    assert!(db
        .completed_index(&agent_core::RepoId::new("repo"))
        .unwrap()
        .is_none());

    let marker: i64 = {
        let conn = Connection::open(&db_path).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'graph_nodes'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(marker, 1);
    let _ = fs::metadata(&db_path).unwrap();
}

#[test]
fn unscoped_commit_nodes_are_split_per_dataset() {
    use agent_core::{RepoId, RepoInfo};
    use rusqlite::params;

    let mut db = IndexDatabase::open_in_memory().unwrap();
    let repo_id = RepoId::new("repo");
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "demo".into(),
        root_path: "/tmp/demo".into(),
        head_commit: None,
        branch: None,
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    let one = db.ensure_dataset(&repo_id, "one").unwrap();
    let two = db.ensure_dataset(&repo_id, "two").unwrap();
    let file_id = format!("file:{}:src/a.rs", two.id);
    db.conn
        .execute(
            "INSERT INTO graph_nodes (id, repo_id, dataset_id, kind, label, datapoint_id)
             VALUES ('commit:abc123', ?1, ?2, 'commit', 'abc123', 'dp-owner')",
            params![repo_id.as_str(), one.id],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO graph_nodes (id, repo_id, dataset_id, kind, label, datapoint_id)
             VALUES (?1, ?2, ?3, 'file', 'src/a.rs', NULL)",
            params![file_id, repo_id.as_str(), two.id],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO graph_edges (repo_id, dataset_id, src_id, dst_id, kind)
             VALUES (?1, ?2, 'commit:abc123', ?3, 'touched-in-commit')",
            params![repo_id.as_str(), two.id, file_id],
        )
        .unwrap();

    crate::graph::migrate_unscoped_commit_nodes(&db.conn).unwrap();

    let old: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM graph_nodes WHERE id = 'commit:abc123'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old, 0);

    let owner_id = crate::graph::commit_node_id(&one.id, "abc123");
    let other_id = crate::graph::commit_node_id(&two.id, "abc123");
    let owner_dp: Option<String> = db
        .conn
        .query_row(
            "SELECT datapoint_id FROM graph_nodes WHERE id = ?1",
            params![owner_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner_dp.as_deref(), Some("dp-owner"));
    let other_dp: Option<String> = db
        .conn
        .query_row(
            "SELECT datapoint_id FROM graph_nodes WHERE id = ?1",
            params![other_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(other_dp.is_none());

    let edge_src: String = db
        .conn
        .query_row(
            "SELECT src_id FROM graph_edges WHERE dataset_id = ?1",
            params![two.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(edge_src, other_id);

    let exported = db
        .export_graph(&two.id, crate::graph::GraphExportFormat::Mermaid)
        .unwrap();
    assert!(exported.contains("abc123"), "{exported}");
    assert!(db
        .upsert_edge(
            repo_id.as_str(),
            &two.id,
            &owner_id,
            &file_id,
            agent_core::EdgeKind::TouchedInCommit,
        )
        .is_err());
}

#[test]
fn architecture_doc_matches_schema() {
    let doc = include_str!("../../../../../docs/agent-runtime-architecture.md");
    assert!(doc.contains("relative_path"));
    assert!(doc.contains("graph_nodes"));
}

#[test]
fn old_reference_rows_survive_source_identity_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(SCHEMA_V1_SQL).unwrap();
    conn.execute("INSERT INTO schema_version (version) VALUES (2)", [])
        .unwrap();
    conn.execute(
        "INSERT INTO repositories (repo_id, name, root_path, indexed_at)
        VALUES ('repo', 'demo', '/tmp/demo', '2020-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO symbol_references (repo_id, source_file, source_symbol_name,
        target_name, kind, line_number) VALUES ('repo', 'methods.rs', 'run', 'only_a', 'calls', 1)",
        [],
    )
    .unwrap();
    drop(conn);
    let db = IndexDatabase::open(&path).unwrap();
    let references = db.list_references("repo").unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].target_name, "only_a");
    assert!(references[0].source_symbol_id.is_none());
    assert_eq!(
        db.files_missing_source_identity(&agent_core::RepoId::new("repo"))
            .unwrap(),
        vec!["methods.rs"]
    );
    drop(db);
    // Migration is idempotent when the upgraded database is reopened.
    let db = IndexDatabase::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(db.list_references("repo").unwrap().len(), 1);
}

fn write_v4_catalog(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(SCHEMA_V1_SQL).unwrap();
    conn.execute_batch(SCHEMA_V2_SQL).unwrap();
    conn.execute_batch("ALTER TABLE symbol_references ADD COLUMN source_symbol_id TEXT;")
        .unwrap();
    conn.execute_batch(SCHEMA_V4_SQL).unwrap();
    conn.execute("INSERT INTO schema_version (version) VALUES (4)", [])
        .unwrap();
    conn.execute(
        "INSERT INTO repositories (repo_id, name, root_path, head_commit, branch, indexed_at)
         VALUES ('repo', 'demo', '/tmp/demo', NULL, 'main', '2020-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files (
            id, repo_id, relative_path, file_hash, git_blob_id, size_bytes, language, kind,
            is_test, is_doc, indexed_at
         ) VALUES (7, 'repo', 'src/main.rs', 'abc', NULL, 4, 'rust', 'source', 0, 0, '2020-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO symbols (
            id, repo_id, name, qualified_name, kind, file_path,
            start_line, end_line, fingerprint
         ) VALUES ('sym1', 'repo', 'main', 'main', 'function', 'src/main.rs', 1, 2, 'fp-old')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files_fts (relative_path, content) VALUES ('src/main.rs', 'unique zebra token')",
        [],
    )
    .unwrap();
}

fn sidecar_path(db_path: &std::path::Path) -> std::path::PathBuf {
    let mut name = db_path.file_name().unwrap().to_os_string();
    name.push(".pre-v5");
    db_path.with_file_name(name)
}

#[test]
fn v4_catalog_upgrades_to_v5_and_keeps_file_ids() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    write_v4_catalog(&db_path);

    let db = IndexDatabase::open(&db_path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 5);
    let user_version: i32 = db
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(user_version, 5);

    let (id, path, raw, redacted): (i64, String, Option<String>, Option<String>) = db
        .conn
        .query_row(
            "SELECT id, relative_path, raw_fingerprint, redacted_fingerprint FROM files",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(id, 7);
    assert_eq!(path, "src/main.rs");
    assert!(raw.is_none());
    assert!(redacted.is_none());

    let (symbol_raw, symbol_redacted): (Option<String>, Option<String>) = db
        .conn
        .query_row(
            "SELECT raw_fingerprint, redacted_fingerprint FROM symbols WHERE id = 'sym1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(symbol_raw.is_none());
    assert_eq!(symbol_redacted.as_deref(), Some("fp-old"));

    let fts: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM files_fts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fts, 0);
    assert!(db.fts_needs_rebuild().unwrap());

    let actors: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'actors'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(actors, 1);
    let phase: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('completed_index_state') WHERE name = 'phase'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(phase, 1);

    db.conn
        .execute(
            "INSERT INTO files (
                repo_id, relative_path, file_hash, size_bytes, language, kind,
                is_test, is_doc, indexed_at
             ) VALUES ('repo', 'src/next.rs', 'def', 1, 'rust', 'source', 0, 0, '2020-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    let next_id: i64 = db
        .conn
        .query_row(
            "SELECT id FROM files WHERE relative_path = 'src/next.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(next_id > 7, "next file id was {next_id}");
    assert!(!sidecar_path(&db_path).exists());
}

#[test]
fn newer_schema_is_refused_before_ddl() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    write_v4_catalog(&db_path);
    let db = IndexDatabase::open(&db_path).unwrap();
    drop(db);

    let conn = Connection::open(&db_path).unwrap();
    conn.execute("UPDATE schema_version SET version = 6", [])
        .unwrap();
    drop(conn);

    match IndexDatabase::open(&db_path) {
        Err(AgentError::SchemaVersion { found, supported }) => {
            assert_eq!(found, 6);
            assert_eq!(supported, 5);
        }
        Err(other) => panic!("expected schema version refusal, got {other}"),
        Ok(_) => panic!("opened a database newer than this binary"),
    }

    let conn = Connection::open(&db_path).unwrap();
    let version: i32 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 6);
    let id: i64 = conn
        .query_row(
            "SELECT id FROM files WHERE relative_path = 'src/main.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(id, 7);
    let actors: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'actors'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(actors, 1);
    assert!(!sidecar_path(&db_path).exists());
}

#[test]
fn v5_reopen_keeps_the_same_file_id() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    write_v4_catalog(&db_path);
    let db = IndexDatabase::open(&db_path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 5);
    drop(db);

    let db = IndexDatabase::open(&db_path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 5);
    let id: i64 = db
        .conn
        .query_row(
            "SELECT id FROM files WHERE relative_path = 'src/main.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(id, 7);
    let fingerprint_columns: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('symbols') WHERE name = 'redacted_fingerprint'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fingerprint_columns, 1);
}

#[test]
fn migrated_fts_stays_empty_until_it_is_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    write_v4_catalog(&db_path);
    let db = IndexDatabase::open(&db_path).unwrap();
    assert!(db.fts_needs_rebuild().unwrap());

    db.conn
        .execute(
            "INSERT INTO files_fts (repo_id, relative_path, content)
             VALUES ('repo', 'src/main.rs', 'unique zebra token')",
            [],
        )
        .unwrap();
    assert!(!db.fts_needs_rebuild().unwrap());
    let hits = db.search_content("zebra", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].relative_path, "src/main.rs");
    assert!(
        hits[0].snippet.contains("zebra"),
        "snippet highlighted the wrong column: {}",
        hits[0].snippet
    );
    assert!(
        hits[0].snippet.contains("<b>"),
        "snippet missing highlight: {}",
        hits[0].snippet
    );
}

#[test]
fn existing_pre_v5_sidecar_is_left_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    write_v4_catalog(&db_path);
    let sidecar = sidecar_path(&db_path);
    fs::write(&sidecar, b"keep-me").unwrap();

    let db = IndexDatabase::open(&db_path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 5);
    drop(db);
    assert_eq!(fs::read(&sidecar).unwrap(), b"keep-me");
}

#[test]
fn current_catalog_opens_while_a_writer_holds_the_reserved_lock() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    let writer = IndexDatabase::open(&db_path).unwrap();
    writer.conn.execute_batch("BEGIN IMMEDIATE;").unwrap();

    match IndexDatabase::open(&db_path) {
        Ok(reader) => {
            assert_eq!(reader.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        }
        Err(other) => panic!("current catalog required a write lock to open: {other}"),
    }

    writer.conn.execute_batch("ROLLBACK;").unwrap();
}
