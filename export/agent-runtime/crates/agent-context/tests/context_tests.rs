use agent_context::ContextCompiler;
use agent_core::{
    config::RepoConfig,
    types::{EvidenceItem, EvidenceMemory, MemoryKind, MemoryStatus, RepoId, RepoInfo},
};
use agent_index::{IndexDatabase, RepoScanner};
use agent_memory::MemoryStore;
use chrono::Utc;
use git2::Repository;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_context_compiler_full_pipeline() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();

    let sig = git2::Signature::now("Engineer", "dev@02os.org").unwrap();

    // 1. Create source, test, and doc files
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();

    let token_file = root.join("src/token.rs");
    fs::write(
        &token_file,
        "pub fn rotate_refresh_token() -> bool {\n    // Single-use token rotation logic\n    true\n}\n",
    )
    .unwrap();

    let session_file = root.join("src/session.rs");
    fs::write(
        &session_file,
        "use crate::token::rotate_refresh_token;\npub fn validate_session() {\n    rotate_refresh_token();\n}\n",
    )
    .unwrap();

    let test_file = root.join("tests/token_test.rs");
    fs::write(
        &test_file,
        "#[test]\nfn test_token_rotation() {\n    assert!(rotate_refresh_token());\n}\n",
    )
    .unwrap();

    let doc_file = root.join("docs/auth.md");
    fs::write(
        &doc_file,
        "# Authentication Protocol\nRefresh tokens are single-use.\n",
    )
    .unwrap();

    // 2. Commit files
    let mut index = repo.index().unwrap();
    index
        .add_path(std::path::Path::new("src/token.rs"))
        .unwrap();
    index
        .add_path(std::path::Path::new("src/session.rs"))
        .unwrap();
    index
        .add_path(std::path::Path::new("tests/token_test.rs"))
        .unwrap();
    index
        .add_path(std::path::Path::new("docs/auth.md"))
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let commit_id = repo
        .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .unwrap();

    // 3. Index repo into SQLite
    let repo_id = RepoId::from_path(root);
    let db_path = agent_core::paths::StoragePaths::repo_db_path(&repo_id).unwrap();
    let mut db = IndexDatabase::open(&db_path).unwrap();

    let repo_info = RepoInfo {
        id: repo_id.clone(),
        name: "test_context_repo".to_string(),
        root_path: root.to_path_buf(),
        head_commit: Some(commit_id.to_string()),
        branch: Some("master".to_string()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    };
    db.update_repo_info(&repo_info).unwrap();

    let scanner = RepoScanner::new(root, RepoConfig::default());
    let scanned = scanner.scan();
    db.save_scanned_files(&repo_id, &scanned).unwrap();

    let mut symbols = Vec::new();
    let mut references = Vec::new();
    for s in &scanned {
        if let Some(ref text) = s.content {
            if let Ok(ext) =
                agent_parser::CodeExtractor::extract(&s.file.relative_path, text, s.file.language)
            {
                symbols.extend(ext.symbols);
                references.extend(ext.references);
            }
        }
    }
    db.save_symbols_and_references(&repo_id, &symbols, &references)
        .unwrap();

    // 4. Save evidence memory
    let store = MemoryStore::for_repo(&repo_id).unwrap();
    let mem = EvidenceMemory {
        id: "mem_auth_refresh".to_string(),
        claim: "Refresh tokens are strictly single-use".to_string(),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: "src/token.rs".to_string(),
            symbols: vec!["rotate_refresh_token".to_string()],
            commit: commit_id.to_string(),
            fingerprint: None,
        }],
        valid_at: commit_id.to_string(),
        confidence: 1.0,
        status: MemoryStatus::Fresh,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    store.save(&mem).unwrap();

    // 5. Run ContextCompiler for task
    let package = ContextCompiler::compile(root, "fix refresh-token rotation race", 4000).unwrap();

    assert_eq!(package.task, "fix refresh-token rotation race");
    assert!(package.staleness.contains("fresh"));

    // Check relevant implementation
    assert!(package
        .relevant_files
        .iter()
        .any(|f| f.relative_path == "src/token.rs"));

    // Check relevant symbols
    assert!(package
        .relevant_symbols
        .iter()
        .any(|s| s.symbol.name == "rotate_refresh_token"));

    // Check relevant tests
    assert!(package
        .relevant_tests
        .iter()
        .any(|t| t.test_file == "tests/token_test.rs"));

    // Check dependencies
    assert!(package
        .dependencies
        .iter()
        .any(|d| d.caller == "validate_session" && d.callee == "rotate_refresh_token"));

    // Check memories
    assert!(package
        .verified_memories
        .iter()
        .any(|m| m.id == "mem_auth_refresh"));

    // Check budget
    assert!(package.budget.returned_tokens <= 4000);
    assert!(package.budget.candidate_tokens > 0);

    // Check markdown generation
    let md = package.to_markdown();
    assert!(md.contains("# Repository Evidence: fix refresh-token rotation race"));
    assert!(md.contains("rotate_refresh_token"));
    assert!(md.contains("validate_session"));
    assert!(md.contains("Refresh tokens are strictly single-use"));
}

#[test]
fn test_context_budget_ignores_files_past_the_return_cap() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    let sig = git2::Signature::now("Engineer", "dev@02os.org").unwrap();

    fs::create_dir_all(root.join("src")).unwrap();
    let mut index = repo.index().unwrap();
    for n in 0..20 {
        let rel = format!("src/widget_{n}.rs");
        let mut body = format!("pub fn widget_{n}() {{\n");
        for line in 0..60 {
            body.push_str(&format!(
                "    let value_{line} = \"widget token budget padding {n}\";\n"
            ));
        }
        body.push_str("}\n");
        fs::write(root.join(&rel), body).unwrap();
        index.add_path(std::path::Path::new(&rel)).unwrap();
    }
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "widgets", &tree, &[])
        .unwrap();

    let repo_id = RepoId::from_path(root);
    let db_path = agent_core::paths::StoragePaths::repo_db_path(&repo_id).unwrap();
    let mut db = IndexDatabase::open(&db_path).unwrap();
    let repo_info = RepoInfo {
        id: repo_id.clone(),
        name: "budget_repo".to_string(),
        root_path: root.to_path_buf(),
        head_commit: Some("abc".into()),
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    };
    db.update_repo_info(&repo_info).unwrap();
    let scanned = RepoScanner::new(root, RepoConfig::default()).scan();
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

    let package = ContextCompiler::compile(root, "widget", 100_000).unwrap();
    assert!(package.budget.candidate_files > 15);
    assert!(package.relevant_files.len() <= 15);
    assert_eq!(package.budget.returned_files, package.relevant_files.len());

    let snippet_tokens: usize = package
        .relevant_files
        .iter()
        .map(|file| {
            file.snippet
                .as_deref()
                .map(|text| text.len().div_ceil(4))
                .unwrap_or(0)
        })
        .sum();
    assert!(
        package.budget.returned_tokens <= snippet_tokens + 2000,
        "returned_tokens {} charged more than retained snippets {}",
        package.budget.returned_tokens,
        snippet_tokens
    );
}

#[test]
fn context_redacts_secrets_and_obeys_a_tight_budget() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    let sig = git2::Signature::now("Engineer", "dev@02os.org").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let token = format!("ghp_{}", "a".repeat(36));
    let mut body = format!("pub fn leaky() {{ let secret = \"{token}\"; }}\n");
    body.push_str(&"x".repeat(20_000));
    body.push('\n');
    fs::write(root.join("src/leaky.rs"), &body).unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_path(std::path::Path::new("src/leaky.rs"))
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "leak", &tree, &[])
        .unwrap();

    let repo_id = RepoId::from_path(root);
    let mut db =
        IndexDatabase::open(agent_core::paths::StoragePaths::repo_db_path(&repo_id).unwrap())
            .unwrap();
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "redact".into(),
        root_path: root.to_path_buf(),
        head_commit: Some("abc".into()),
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    let scanned = RepoScanner::new(root, RepoConfig::default()).scan();
    assert!(scanned.iter().all(|item| {
        item.content
            .as_deref()
            .map(|text| !text.contains(&token))
            .unwrap_or(true)
    }));
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

    let store = MemoryStore::for_repo(&repo_id).unwrap();
    store
        .save(&EvidenceMemory {
            id: "mem_invalid".into(),
            claim: "leaky token was rotated".into(),
            kind: MemoryKind::ArchitecturalFact,
            evidence: vec![EvidenceItem {
                file: "/etc/passwd".into(),
                symbols: vec![],
                commit: "abc".into(),
                fingerprint: None,
            }],
            valid_at: "abc".into(),
            confidence: 0.0,
            status: MemoryStatus::Invalidated,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .unwrap();

    let empty = ContextCompiler::compile(root, "leaky", 0).unwrap();
    assert_eq!(empty.budget.returned_tokens, 0);
    assert!(empty
        .relevant_files
        .iter()
        .all(|file| file.snippet.is_none()));
    assert!(empty.to_markdown().is_empty());

    let package = ContextCompiler::compile(root, "leaky", 200).unwrap();
    assert!(package.budget.returned_tokens <= 200);
    let markdown = package.to_markdown();
    assert!(!markdown.contains(&token));
    assert!(package.relevant_files.iter().all(|file| {
        file.snippet
            .as_deref()
            .map(|text| text.len() < 2000)
            .unwrap_or(true)
    }));
    assert!(!package
        .relevant_files
        .iter()
        .any(|file| file.relative_path == "/etc/passwd"));

    let wide = ContextCompiler::compile(root, "leaky", 4000).unwrap();
    assert!(wide.verified_memories.is_empty());
    assert!(wide
        .diagnostic_memories
        .iter()
        .any(|memory| memory.id == "mem_invalid"));
    assert!(wide.to_markdown().contains("not verified"));
    assert!(!wide.to_markdown().contains(&token));
}
