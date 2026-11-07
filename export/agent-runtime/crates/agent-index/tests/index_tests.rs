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
}
