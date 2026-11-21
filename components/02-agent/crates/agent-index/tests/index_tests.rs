use agent_core::{
    config::RepoConfig,
    types::{RepoId, Symbol, SymbolKind},
    Provenance, RepoInfo, SourceKind,
};
use agent_index::{gc_repos, IndexDatabase, NewDataPoint, RepoScanner};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_scanner_and_sqlite_fts5() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create a mock repository structure
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();

    fs::write(
        root.join("src/auth.rs"),
        "pub fn rotate_refresh_token() { /* token rotation logic */ }",
    )
    .unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "#[test] fn test_rotate() { rotate_refresh_token(); }",
    )
    .unwrap();
    fs::write(
        root.join("docs/architecture.md"),
        "# Authentication Architecture\nRefresh tokens are single use.",
    )
    .unwrap();
    fs::write(root.join(".env"), "DATABASE_URL=postgres://secret").unwrap();

    let config = RepoConfig::default();
    let scanner = RepoScanner::new(root, config);
    let scanned = scanner.scan();

    // Verify .env was excluded
    assert!(!scanned.iter().any(|f| f.file.relative_path == ".env"));

    // Verify files were found and classified correctly
    let auth_file = scanned
        .iter()
        .find(|f| f.file.relative_path == "src/auth.rs")
        .expect("src/auth.rs should be found");
    assert_eq!(auth_file.file.kind, agent_core::types::FileKind::Source);
    assert_eq!(auth_file.file.language, agent_core::types::Language::Rust);

    let test_file = scanned
        .iter()
        .find(|f| f.file.relative_path == "tests/auth_test.rs")
        .expect("tests/auth_test.rs should be found");
    assert!(test_file.file.is_test);

    // Save to database
    let mut db = IndexDatabase::open_in_memory().unwrap();
    let repo_id = RepoId::from_path(root);
    let repo_info = RepoInfo {
        id: repo_id.clone(),
        name: "test_repo".into(),
        root_path: root.to_path_buf(),
        head_commit: Some("c0ffee".into()),
        branch: Some("main".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    };
    db.update_repo_info(&repo_info).unwrap();
    let saved_count = db.save_scanned_files(&repo_id, &scanned).unwrap();
    assert_eq!(saved_count, 3);

    // Test stats
    let stats = db.get_stats(&repo_id).unwrap();
    assert_eq!(stats.total_files, 3);
    assert_eq!(stats.source_files, 1);
    assert_eq!(stats.test_files, 1);
    assert_eq!(stats.doc_files, 1);

    // Test FTS5 search
    let results = db.search_content("refresh token", 10).unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|r| r.relative_path.contains("auth")));

    let listed = db.list_files(&repo_id).unwrap();
    let listed_auth = listed
        .iter()
        .find(|f| f.relative_path == "src/auth.rs")
        .unwrap();
    assert_eq!(
        listed_auth.language,
        agent_core::types::Language::Rust,
        "language names stored in sqlite must round-trip"
    );
}

#[test]
fn test_find_tests_ignores_unrelated_calls_inside_tests() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("src/parse.rs"),
        "pub fn parse_header() -> bool { true }\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/parse_test.rs"),
        "#[test]\nfn test_parse_header() {\n    assert!(parse_header());\n    let _ = String::new();\n}\n",
    )
    .unwrap();

    let scanner = RepoScanner::new(root, RepoConfig::default());
    let scanned = scanner.scan();
    let mut db = IndexDatabase::open_in_memory().unwrap();
    let repo_id = RepoId::from_path(root);
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "parse_repo".into(),
        root_path: root.to_path_buf(),
        head_commit: None,
        branch: None,
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    db.save_scanned_files(&repo_id, &scanned).unwrap();

    let mut symbols = Vec::new();
    let mut references = Vec::new();
    for item in &scanned {
        if let Some(text) = &item.content {
            if let Ok(extracted) = agent_parser::CodeExtractor::extract(
                &item.file.relative_path,
                text,
                item.file.language,
            ) {
                symbols.extend(extracted.symbols);
                references.extend(extracted.references);
            }
        }
    }
    db.save_symbols_and_references(&repo_id, &symbols, &references)
        .unwrap();

    let in_file = db.find_symbols_by_file(&repo_id, "src/parse.rs").unwrap();
    assert!(in_file.iter().any(|s| s.name == "parse_header"));
    assert!(db.find_symbols_by_name(&repo_id, "").unwrap().is_empty());

    let tests = db.find_tests_for_symbol(&repo_id, "parse_header").unwrap();
    assert!(tests.iter().any(|t| t.source_file == "tests/parse_test.rs"));
    assert!(
        tests.iter().all(|t| {
            t.target_name != "assert" && t.target_name != "new" && t.target_name != "String::new"
        }),
        "unexpected tests: {:?}",
        tests
            .iter()
            .map(|t| (
                t.kind,
                t.source_symbol_name.clone(),
                t.target_name.clone(),
                t.line_number
            ))
            .collect::<Vec<_>>()
    );

    let broad = db.find_tests_for_symbol(&repo_id, "parse").unwrap();
    assert!(broad
        .iter()
        .any(|t| t.source_symbol_name.as_deref() == Some("test_parse_header")));
    assert!(broad
        .iter()
        .all(|t| t.target_name != "assert" && t.target_name != "new"));
}

fn sample_symbol(name: &str, qualified: &str) -> Symbol {
    Symbol {
        id: format!("sym-{name}"),
        name: name.to_string(),
        qualified_name: qualified.to_string(),
        kind: SymbolKind::Function,
        file_path: "src/parse.rs".to_string(),
        start_line: 1,
        end_line: 1,
        signature: None,
        doc_comment: None,
        fingerprint: "fp".to_string(),
    }
}

#[test]
fn test_symbol_queries_escape_like_wildcards() {
    let mut db = IndexDatabase::open_in_memory().unwrap();
    let repo_id = RepoId::new("like-repo");
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "like".into(),
        root_path: PathBuf::from("/tmp/like-repo"),
        head_commit: None,
        branch: None,
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    db.save_symbols_and_references(
        &repo_id,
        &[
            sample_symbol("parse_header", "src/parse.rs::parse_header"),
            sample_symbol("parseXheader", "src/parse.rs::parseXheader"),
            sample_symbol("100%", "src/parse.rs::100%"),
            sample_symbol("plain", "src/parse.rs::plain"),
        ],
        &[],
    )
    .unwrap();

    let found = db.find_symbols_by_name(&repo_id, "parse_header").unwrap();
    assert!(found.iter().any(|symbol| symbol.name == "parse_header"));
    assert!(found.iter().all(|symbol| symbol.name != "parseXheader"));

    let searched = db.search_symbols(&repo_id, "%", 20).unwrap();
    assert!(searched.iter().any(|symbol| symbol.name == "100%"));
    assert!(searched.iter().all(|symbol| symbol.name == "100%"));
}

#[test]
fn test_gc_drops_orphan_directories_and_old_versions() {
    let data = tempdir().unwrap();
    let missing = data.path().join("missing-root");
    let orphan = data.path().join("repos").join("orphan");
    fs::create_dir_all(&orphan).unwrap();
    let mut orphan_db = IndexDatabase::open(orphan.join("index.sqlite")).unwrap();
    orphan_db
        .update_repo_info(&RepoInfo {
            id: RepoId::new("orphan"),
            name: "orphan".into(),
            root_path: missing,
            head_commit: None,
            branch: None,
            is_clean: true,
            modified_count: 0,
            untracked_count: 0,
        })
        .unwrap();
    drop(orphan_db);

    let live_root = data.path().join("live-src");
    fs::create_dir_all(&live_root).unwrap();
    let live_dir = data.path().join("repos").join("live");
    fs::create_dir_all(&live_dir).unwrap();
    let mut live = IndexDatabase::open(live_dir.join("index.sqlite")).unwrap();
    let repo_id = RepoId::new("live");
    live.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "live".into(),
        root_path: live_root,
        head_commit: None,
        branch: None,
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    let dataset = live.ensure_dataset(&repo_id, "default").unwrap();
    for index in 0..5 {
        live.put_datapoint(NewDataPoint {
            repo_id: repo_id.clone(),
            dataset_id: dataset.id.clone(),
            kind: "pasted_text".into(),
            path: String::new(),
            content: format!("note {index}"),
            provenance: Provenance {
                source_kind: SourceKind::PastedText,
                commit_id: None,
                path: String::new(),
                start_byte: None,
                end_byte: None,
                symbol_fingerprint: None,
            },
            confidence: 1.0,
        })
        .unwrap();
    }
    drop(live);

    let report = gc_repos(data.path()).unwrap();
    assert_eq!(report.orphan_directories, 1);
    assert!(!orphan.exists());
    assert!(live_dir.join("index.sqlite").is_file());
    assert!(report.databases_vacuumed >= 1);
    let live = IndexDatabase::open(live_dir.join("index.sqlite")).unwrap();
    let points = live.list_datapoints(&dataset.id).unwrap();
    assert_eq!(points.len(), 3);
    assert!(points.iter().all(|point| point.kind == "pasted_text"));

    // Non-empty orphan directory without index.sqlite should also be reclaimed.
    let stale_non_empty = data.path().join("repos").join("stale-orphan");
    fs::create_dir_all(&stale_non_empty).unwrap();
    fs::write(stale_non_empty.join("partial.tmp"), b"incomplete").unwrap();
    let report2 = gc_repos(data.path()).unwrap();
    assert_eq!(report2.orphan_directories, 1);
    assert!(!stale_non_empty.exists());
}

#[test]
fn scanner_fingerprint_matches_scan_fingerprint_and_v5_columns_populate() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        format!(
            "pub fn a() {{}}\npub fn b() {{ let _k = \"ghp_{}\"; }}\n",
            "z".repeat(36)
        ),
    )
    .unwrap();

    let cfg = agent_core::RepoConfig::default();
    let scanner = RepoScanner::new(root, cfg.clone());
    let scanned = scanner.scan();
    assert_eq!(
        scanner.fingerprint(),
        RepoScanner::scan_fingerprint(&scanned, &cfg)
    );

    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "fp-test".into(),
        root_path: root.to_path_buf(),
        head_commit: None,
        branch: None,
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();

    let sym = sample_symbol("a", "src/lib.rs::a");
    db.update_incremental(&repo_id, &scanned, &[], &[sym], &[], None)
        .unwrap();
    let stats = db.get_stats(&repo_id).unwrap();
    assert_eq!(stats.total_files, 1);
    assert_eq!(stats.source_files, 1);
    assert_eq!(stats.total_symbols, 1);

    let deps = db.find_symbol_deps(&repo_id, "a").unwrap();
    assert_eq!(deps.defined_in.len(), 1);
}

#[test]
fn pending_confirmation_nonce_and_v5_telemetry_work() {
    let dir = tempdir().unwrap();
    let mut db = IndexDatabase::open(dir.path().join("index.sqlite")).unwrap();
    let repo_id = RepoId::new("tel");
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "tel".into(),
        root_path: dir.path().to_path_buf(),
        head_commit: None,
        branch: None,
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();

    let args_hash = "ab".repeat(32);
    db.start_pending_invocation(
        &repo_id,
        "agt",
        "agt-worker",
        "run_tests",
        &args_hash,
        "{}",
        "nonce-1234",
        "pending-1",
    )
    .unwrap();
    assert_eq!(db.list_pending_confirmations(&repo_id).unwrap().len(), 1);
    assert!(db
        .verify_and_consume_pending_confirmation("pending-1", &args_hash, "wrong", "allow")
        .is_err());
    let consumed = db
        .verify_and_consume_pending_confirmation("pending-1", &args_hash, "nonce-1234", "allow")
        .unwrap();
    assert_eq!(consumed.tool, "run_tests");
    assert!(db.list_pending_confirmations(&repo_id).unwrap().is_empty());

    db.record_completed_invocation(
        &repo_id,
        "mcp-client",
        "default",
        "repository_context",
        "00",
        "{}",
        false,
        None,
        Some(512),
    )
    .unwrap();
    assert!(!db.list_actors(&repo_id).unwrap().is_empty());
    let tokens = db.list_token_counts(&repo_id).unwrap();
    assert_eq!(tokens.len(), 2);
    assert!(tokens
        .iter()
        .any(|t| t.tool == "repository_context" && t.total_tokens == 512));
}

#[test]
fn pending_approval_binds_to_exact_intended_arguments_without_storing_raw_args() {
    use sha2::{Digest, Sha256};

    let sha = |text: &str| {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        hex::encode(hasher.finalize())
    };

    let dir = tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    let mut db = IndexDatabase::open(&db_path).unwrap();
    let repo_id = RepoId::new("approval-binding");
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "approval-binding".into(),
        root_path: dir.path().to_path_buf(),
        head_commit: None,
        branch: None,
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();

    let secret_a = format!("ghp_{}", "a".repeat(36));
    let secret_b = format!("ghp_{}", "b".repeat(36));
    let payload_a = format!(r#"{{"cmd":"deploy-prod","token":"{secret_a}"}}"#);
    let payload_b = format!(r#"{{"cmd":"deploy-prod","token":"{secret_b}"}}"#);
    let payload_redacted = r#"{"cmd":"deploy-prod","token":"[REDACTED]"}"#;

    let hash_a = sha(&payload_a);
    let hash_b = sha(&payload_b);
    let hash_redacted = sha(payload_redacted);
    assert_ne!(hash_a, hash_b);
    assert_ne!(hash_a, hash_redacted);

    // Malformed args_hash is rejected at pending registration time.
    assert!(db
        .start_pending_invocation(
            &repo_id,
            "agt",
            "agt-worker",
            "deploy",
            "not-a-sha256",
            &payload_a,
            "nonce-bad",
            "pending-bad",
        )
        .is_err());

    db.start_pending_invocation(
        &repo_id,
        "agt",
        "agt-worker",
        "deploy",
        &hash_a,
        &payload_a,
        "nonce-exact-1",
        "pending-exact-1",
    )
    .unwrap();

    // Pending confirmation carries the exact intended args_hash and no raw arguments.
    let pending = db.list_pending_confirmations(&repo_id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].args_hash, hash_a);
    assert!(pending[0].args_canonical.is_empty());

    // Mutated arguments (even differing only in a secret) and redacted-args hash must fail.
    assert!(db
        .verify_and_consume_pending_confirmation(
            "pending-exact-1",
            &hash_b,
            "nonce-exact-1",
            "allow"
        )
        .is_err());
    assert!(db
        .verify_and_consume_pending_confirmation(
            "pending-exact-1",
            &hash_redacted,
            "nonce-exact-1",
            "allow",
        )
        .is_err());
    assert!(db
        .verify_and_consume_pending_confirmation("pending-exact-1", &hash_a, "wrong-nonce", "allow")
        .is_err());
    assert_eq!(db.list_pending_confirmations(&repo_id).unwrap().len(), 1);

    // Only the exact intended args_hash + matching nonce consumes the pending approval.
    let consumed = db
        .verify_and_consume_pending_confirmation(
            "pending-exact-1",
            &hash_a,
            "nonce-exact-1",
            "allow",
        )
        .unwrap();
    assert_eq!(consumed.args_hash, hash_a);
    assert!(consumed.args_canonical.is_empty());
    assert!(db.list_pending_confirmations(&repo_id).unwrap().is_empty());

    // Replaying the same exact hash + nonce is rejected.
    assert!(db
        .verify_and_consume_pending_confirmation(
            "pending-exact-1",
            &hash_a,
            "nonce-exact-1",
            "allow",
        )
        .is_err());

    // Completed telemetry ignores any raw arguments passed by callers.
    let raw_marker = r#"{"query":"HARMLESS_DOTENV_MARKER_ABC123456"}"#;
    let marker_hash = sha(raw_marker);
    db.record_completed_invocation(
        &repo_id,
        "mcp-client",
        "default",
        "repository_search",
        &marker_hash,
        raw_marker,
        false,
        None,
        Some(64),
    )
    .unwrap();
    drop(db);

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let mut stmt = conn
        .prepare("SELECT id, args_hash, args_canonical, finished_at FROM tool_invocations")
        .unwrap();
    let rows: Vec<(String, String, String, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    for (_, stored_hash, stored_canonical, finished_at) in &rows {
        assert!(finished_at.is_some());
        assert!(
            stored_canonical.is_empty(),
            "completed telemetry row must not persist raw args_canonical"
        );
        assert!(stored_hash == &hash_a || stored_hash == &marker_hash);
    }

    // Legacy pending rows containing raw args_canonical are also scrubbed on consumption.
    let legacy_hash = sha(r#"{"cmd":"legacy"}"#);
    let actor_id: String = conn
        .query_row("SELECT id FROM actors LIMIT 1", [], |row| row.get(0))
        .unwrap();
    conn.execute(
        "INSERT INTO tool_invocations (
            id, repo_id, actor_id, tool, args_hash, args_canonical,
            started_at, finished_at, is_error, error_code, warning_json
         ) VALUES (?1, ?2, ?3, 'legacy_tool', ?4, ?5, '2026-11-21T00:00:00Z', NULL, 0, NULL, ?6)",
        rusqlite::params![
            "pending-legacy",
            repo_id.as_str(),
            actor_id,
            legacy_hash,
            r#"{"legacy":"RAW_LEGACY_PAYLOAD"}"#,
            r#"{"confirmation_nonce":"nonce-legacy","status":"pending"}"#,
        ],
    )
    .unwrap();
    drop(stmt);
    drop(conn);

    let db = IndexDatabase::open(&db_path).unwrap();
    let legacy_consumed = db
        .verify_and_consume_pending_confirmation(
            "pending-legacy",
            &legacy_hash,
            "nonce-legacy",
            "allow",
        )
        .unwrap();
    assert_eq!(legacy_consumed.args_hash, legacy_hash);
    assert!(legacy_consumed.args_canonical.is_empty());
    drop(db);

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let scrubbed: String = conn
        .query_row(
            "SELECT args_canonical FROM tool_invocations WHERE id = 'pending-legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(scrubbed.is_empty());
    drop(conn);

    let raw_bytes = fs::read(&db_path).unwrap();
    let raw_lossy = String::from_utf8_lossy(&raw_bytes);
    for forbidden in [
        secret_a.as_str(),
        "deploy-prod",
        "HARMLESS_DOTENV_MARKER_ABC123456",
    ] {
        assert!(
            !raw_lossy.contains(forbidden),
            "raw argument `{forbidden}` leaked into SQLite database"
        );
    }
}
