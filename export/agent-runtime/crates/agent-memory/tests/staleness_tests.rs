use agent_core::{
    config::RepoConfig,
    types::{EvidenceItem, EvidenceMemory, MemoryKind, MemoryStatus, RepoId},
};
use agent_git::GitRepo;
use agent_index::{IndexDatabase, RepoScanner};
use agent_memory::{MemoryStore, StalenessEngine};
use chrono::Utc;
use git2::Repository;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_staleness_engine_detection() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();

    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();

    // 1. Create initial files
    fs::create_dir_all(root.join("src")).unwrap();
    let auth_file = root.join("src/auth.rs");
    fs::write(
        &auth_file,
        "pub fn rotate_refresh_token() -> bool {\n    true\n}\n",
    )
    .unwrap();

    // Commit file
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("src/auth.rs")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let commit_id = repo
        .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .unwrap();

    // Index repo
    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    let repo_info = agent_core::types::RepoInfo {
        id: repo_id.clone(),
        name: "staleness_test".to_string(),
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

    // Extract and save symbols
    let mut symbols = Vec::new();
    let mut references = Vec::new();
    for s in &scanned {
        if let Some(ref text) = s.content {
            let ext =
                agent_parser::CodeExtractor::extract(&s.file.relative_path, text, s.file.language)
                    .unwrap();
            symbols.extend(ext.symbols);
            references.extend(ext.references);
        }
    }
    db.save_symbols_and_references(&repo_id, &symbols, &references)
        .unwrap();

    // Create memory attached to rotate_refresh_token
    let memory_file = root.join("memories.jsonl");
    let store = MemoryStore::new(memory_file);
    let mem = EvidenceMemory {
        id: "mem_rotate".to_string(),
        claim: "Refresh token rotation is mandatory".to_string(),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: "src/auth.rs".to_string(),
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

    let git_repo = GitRepo::open(root).unwrap();

    // 2. Evaluate when clean: should remain Fresh
    let report_clean = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert_eq!(report_clean.changed_symbols.len(), 0);
    assert_eq!(report_clean.fresh_count, 1);

    // 3. Modify function in working tree
    fs::write(
        &auth_file,
        "pub fn rotate_refresh_token() -> bool {\n    // Modified implementation\n    false\n}\n",
    )
    .unwrap();

    // Evaluate again: symbol modified -> memory degraded
    let report_modified =
        StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert!(report_modified
        .changed_symbols
        .contains(&"rotate_refresh_token".to_string()));
    assert_eq!(report_modified.degraded_count, 1);

    let updated_mem = store.get("mem_rotate").unwrap().unwrap();
    assert_eq!(updated_mem.status, MemoryStatus::Degraded);
    assert_eq!(updated_mem.confidence, 0.70);

    // 4. Delete file: memory should become Stale and the removed symbol is detected
    fs::remove_file(&auth_file).unwrap();
    let report_deleted = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert!(report_deleted
        .changed_symbols
        .contains(&"rotate_refresh_token".to_string()));
    assert_eq!(report_deleted.stale_count, 1);

    let stale_mem = store.get("mem_rotate").unwrap().unwrap();
    assert_eq!(stale_mem.status, MemoryStatus::Stale);
    assert_eq!(stale_mem.confidence, 0.0);
}

#[test]
fn test_invalidated_memory_is_not_revived() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/auth.rs"), "pub fn rotate() { }\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("src/auth.rs")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let commit_id = repo
        .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    let repo_id = RepoId::from_path(root);
    let db = IndexDatabase::open_in_memory().unwrap();
    let store = MemoryStore::new(root.join("memories.jsonl"));
    let mem = EvidenceMemory {
        id: "mem_old_rule".to_string(),
        claim: "This rule was repealed".to_string(),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: "src/auth.rs".to_string(),
            symbols: vec!["rotate".to_string()],
            commit: commit_id.to_string(),
            fingerprint: None,
        }],
        valid_at: commit_id.to_string(),
        confidence: 0.25,
        status: MemoryStatus::Invalidated,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    store.save(&mem).unwrap();

    let git_repo = GitRepo::open(root).unwrap();
    let report = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert!(report.affected_memories.is_empty());
    assert_eq!(report.stale_count, 1);
    assert_eq!(report.fresh_count, 0);

    let stored = store.get("mem_old_rule").unwrap().unwrap();
    assert_eq!(stored.status, MemoryStatus::Invalidated);
    assert_eq!(stored.confidence, 0.25);
}

#[test]
fn deleting_a_function_marks_its_memory_stale_after_reindex() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let source = root.join("src/auth.rs");
    fs::write(
        &source,
        "pub fn rotate_refresh_token() -> bool { true }\npub fn keep_session() {}\n",
    )
    .unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("src/auth.rs")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let commit_id = repo
        .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    db.update_repo_info(&agent_core::types::RepoInfo {
        id: repo_id.clone(),
        name: "staleness_removed".into(),
        root_path: root.to_path_buf(),
        head_commit: Some(commit_id.to_string()),
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    let store = MemoryStore::new(root.join("memories.jsonl"));
    let make = |id: &str, symbol: &str| EvidenceMemory {
        id: id.into(),
        claim: format!("{symbol} exists"),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: "src/auth.rs".into(),
            symbols: vec![symbol.into()],
            commit: commit_id.to_string(),
            fingerprint: None,
        }],
        valid_at: commit_id.to_string(),
        confidence: 1.0,
        status: MemoryStatus::Fresh,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    store
        .save(&make("mem_rotate", "rotate_refresh_token"))
        .unwrap();
    store.save(&make("mem_keep", "keep_session")).unwrap();

    fs::write(&source, "pub fn keep_session() {}\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("src/auth.rs")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let parent = repo.find_commit(commit_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "drop rotate", &tree, &[&parent])
        .unwrap();

    let scanned = RepoScanner::new(root, RepoConfig::default()).scan();
    db.save_scanned_files(&repo_id, &scanned).unwrap();
    let mut symbols = Vec::new();
    let mut references = Vec::new();
    for item in &scanned {
        if let Some(text) = &item.content {
            let extracted = agent_parser::CodeExtractor::extract(
                &item.file.relative_path,
                text,
                item.file.language,
            )
            .unwrap();
            symbols.extend(extracted.symbols);
            references.extend(extracted.references);
        }
    }
    db.save_symbols_and_references(&repo_id, &symbols, &references)
        .unwrap();

    let git_repo = GitRepo::open(root).unwrap();
    let report = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert_eq!(
        store.get("mem_rotate").unwrap().unwrap().status,
        MemoryStatus::Stale
    );
    assert_eq!(
        store.get("mem_keep").unwrap().unwrap().status,
        MemoryStatus::Fresh
    );
    assert_eq!(report.fresh_count, 1);
    assert_eq!(report.stale_count, 1);
}
