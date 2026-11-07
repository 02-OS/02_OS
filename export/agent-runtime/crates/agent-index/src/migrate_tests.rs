use crate::db::IndexDatabase;
use crate::schema::{CURRENT_SCHEMA_VERSION, SCHEMA_V1_SQL};
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
