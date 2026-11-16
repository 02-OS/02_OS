use agent_core::{
    config::RepoConfig,
    types::{EvidenceItem, EvidenceMemory, MemoryKind, MemoryStatus, RepoId},
};
use agent_git::GitRepo;
use agent_index::{IndexDatabase, RepoScanner};
use agent_memory::{index_pipeline, MemoryStore, RepoHandle, StalenessEngine, TaskRegistry};
use chrono::Utc;
use git2::Repository;
use std::fs;
use std::os::unix::fs::symlink;
use tempfile::tempdir;

fn commit_paths(repo: &Repository, message: &str, paths: &[&str]) {
    let mut index = repo.index().unwrap();
    for path in paths {
        index.add_path(std::path::Path::new(path)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(head) => vec![head.peel_to_commit().unwrap()],
        Err(_) => Vec::new(),
    };
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap();
}

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

fn index_symbols(root: &std::path::Path, repo_id: &RepoId, db: &mut IndexDatabase) {
    let scanned = RepoScanner::new(root, RepoConfig::default()).scan();
    db.save_scanned_files(repo_id, &scanned).unwrap();
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
    db.save_symbols_and_references(repo_id, &symbols, &references)
        .unwrap();
}

fn evidence_memory(
    root: &std::path::Path,
    id: &str,
    file: &str,
    symbol: &str,
    fingerprint: Option<String>,
) -> EvidenceMemory {
    EvidenceMemory {
        id: id.into(),
        claim: format!("{symbol} in {file}"),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: file.into(),
            symbols: vec![symbol.into()],
            commit: "abc".into(),
            fingerprint: fingerprint.or_else(|| {
                agent_memory::MemoryVerifier::capture_evidence(
                    root,
                    file,
                    vec![symbol.into()],
                    "abc",
                )
                .ok()
                .and_then(|captured| captured.fingerprint)
            }),
        }],
        valid_at: "abc".into(),
        confidence: 1.0,
        status: MemoryStatus::Fresh,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn symlink_outside_the_repo_does_not_leak_into_changed_symbols() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/tracked.rs"), "pub fn local_symbol() {}\n").unwrap();
    commit_paths(&repo, "init", &["src/tracked.rs"]);

    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    db.update_repo_info(&agent_core::types::RepoInfo {
        id: repo_id.clone(),
        name: "symlink".into(),
        root_path: root.to_path_buf(),
        head_commit: None,
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    index_symbols(root, &repo_id, &mut db);

    let outside = tempdir().unwrap();
    let external = outside.path().join("external.rs");
    fs::write(&external, "pub fn PRIVATE_EXTERNAL_SYMBOL_MARKER() {}\n").unwrap();
    fs::remove_file(root.join("src/tracked.rs")).unwrap();
    symlink(&external, root.join("src/tracked.rs")).unwrap();

    let store = MemoryStore::new(root.join("memories.jsonl"));
    let git_repo = GitRepo::open(root).unwrap();
    let report = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert!(report
        .modified_files
        .iter()
        .any(|path| path == "src/tracked.rs"));
    assert!(report
        .changed_symbols
        .iter()
        .all(|name| { !name.contains("PRIVATE_EXTERNAL_SYMBOL_MARKER") }));
}

#[test]
fn oversized_diff_files_are_not_parsed_into_changed_symbols() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/big.rs"), "pub fn small() {}\n").unwrap();
    commit_paths(&repo, "init", &["src/big.rs"]);

    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    db.update_repo_info(&agent_core::types::RepoInfo {
        id: repo_id.clone(),
        name: "oversize".into(),
        root_path: root.to_path_buf(),
        head_commit: None,
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    index_symbols(root, &repo_id, &mut db);

    fs::create_dir_all(root.join(".02agent")).unwrap();
    fs::write(root.join(".02agent/config.toml"), "max_file_size_kb = 1\n").unwrap();
    let body = format!(
        "pub fn OVERSIZED_PRIVATE_MARKER() {{\n{}\n}}\n",
        "    let padding = 1;\n".repeat(80)
    );
    assert!(body.len() > 1024);
    fs::write(root.join("src/big.rs"), body).unwrap();

    let store = MemoryStore::new(root.join("memories.jsonl"));
    let git_repo = GitRepo::open(root).unwrap();
    let report = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert!(report
        .modified_files
        .iter()
        .any(|path| path == "src/big.rs"));
    assert!(report
        .changed_symbols
        .iter()
        .all(|name| !name.contains("OVERSIZED_PRIVATE_MARKER")));
}

#[test]
fn same_symbol_name_in_another_file_stays_fresh() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/a.rs"),
        "pub fn same_name() {\n    let value = 1;\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/b.rs"),
        "pub fn same_name() {\n    let value = 1;\n}\n",
    )
    .unwrap();
    commit_paths(&repo, "init", &["src/a.rs", "src/b.rs"]);

    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    db.update_repo_info(&agent_core::types::RepoInfo {
        id: repo_id.clone(),
        name: "same-name".into(),
        root_path: root.to_path_buf(),
        head_commit: None,
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    index_symbols(root, &repo_id, &mut db);

    let store = MemoryStore::new(root.join("memories.jsonl"));
    store
        .save(&evidence_memory(
            root,
            "mem_a",
            "src/a.rs",
            "same_name",
            None,
        ))
        .unwrap();
    store
        .save(&evidence_memory(
            root,
            "mem_b",
            "src/b.rs",
            "same_name",
            None,
        ))
        .unwrap();

    fs::write(
        root.join("src/a.rs"),
        "pub fn same_name() {\n    let value = 2;\n}\n",
    )
    .unwrap();

    let git_repo = GitRepo::open(root).unwrap();
    let report = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert!(report
        .changed_symbols
        .iter()
        .any(|name| name == "same_name"));
    assert!(report.modified_files.iter().any(|path| path == "src/a.rs"));
    assert!(!report.modified_files.iter().any(|path| path == "src/b.rs"));
    assert_eq!(
        store.get("mem_a").unwrap().unwrap().status,
        MemoryStatus::Degraded
    );
    assert_eq!(
        store.get("mem_b").unwrap().unwrap().status,
        MemoryStatus::Fresh
    );
    assert_eq!(store.get("mem_b").unwrap().unwrap().confidence, 1.0);
}

#[test]
fn fingerprint_mismatch_stays_degraded_after_a_clean_reindex() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let source = root.join("src/auth.rs");
    fs::write(&source, "pub fn rotate_refresh_token() -> bool { true }\n").unwrap();
    commit_paths(&repo, "init", &["src/auth.rs"]);

    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    index_pipeline(&mut handle, true, &TaskRegistry::new()).unwrap();
    let recorded = handle
        .db
        .list_symbols(handle.info.id.as_str())
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "rotate_refresh_token")
        .unwrap()
        .fingerprint;
    let repo_id = handle.info.id.clone();
    let store = MemoryStore::new(root.join("memories.jsonl"));
    store
        .save(&evidence_memory(
            root,
            "mem_rotate",
            "src/auth.rs",
            "rotate_refresh_token",
            Some(recorded),
        ))
        .unwrap();

    let git_repo = GitRepo::open(root).unwrap();
    let matched = StalenessEngine::evaluate(root, &repo_id, &git_repo, &handle.db, &store).unwrap();
    assert!(matched.changed_symbols.is_empty());
    assert_eq!(matched.fresh_count, 1);
    assert_eq!(
        store.get("mem_rotate").unwrap().unwrap().status,
        MemoryStatus::Fresh
    );
    drop(handle);

    fs::write(&source, "pub fn rotate_refresh_token() -> bool { false }\n").unwrap();
    commit_paths(&repo, "change body", &["src/auth.rs"]);
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    index_pipeline(&mut handle, true, &TaskRegistry::new()).unwrap();

    let git_repo = GitRepo::open(root).unwrap();
    let report = StalenessEngine::evaluate(root, &repo_id, &git_repo, &handle.db, &store).unwrap();
    assert!(
        report.changed_symbols.is_empty(),
        "clean tree must not degrade through the diff path: {:?}",
        report.changed_symbols
    );
    let stored = store.get("mem_rotate").unwrap().unwrap();
    assert_eq!(stored.status, MemoryStatus::Degraded);
    assert_eq!(stored.confidence, 0.70);
}

#[test]
fn missing_tsx_symbol_stays_stale_after_a_clean_reindex() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/Widget.tsx"),
        "export function RemovedComponent() {\n  return 1;\n}\nexport function KeptComponent() {\n  return 1;\n}\n",
    )
    .unwrap();
    commit_paths(&repo, "init", &["src/Widget.tsx"]);

    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    db.update_repo_info(&agent_core::types::RepoInfo {
        id: repo_id.clone(),
        name: "tsx".into(),
        root_path: root.to_path_buf(),
        head_commit: None,
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    index_symbols(root, &repo_id, &mut db);

    let store = MemoryStore::new(root.join("memories.jsonl"));
    store
        .save(&evidence_memory(
            root,
            "mem_removed",
            "src/Widget.tsx",
            "RemovedComponent",
            None,
        ))
        .unwrap();
    store
        .save(&evidence_memory(
            root,
            "mem_kept",
            "src/Widget.tsx",
            "KeptComponent",
            None,
        ))
        .unwrap();

    fs::write(
        root.join("src/Widget.tsx"),
        "export function KeptComponent() {\n  return 1;\n}\n",
    )
    .unwrap();
    commit_paths(&repo, "drop removed", &["src/Widget.tsx"]);
    index_symbols(root, &repo_id, &mut db);

    let git_repo = GitRepo::open(root).unwrap();
    let report = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert!(report.changed_symbols.is_empty());
    assert_eq!(
        store.get("mem_removed").unwrap().unwrap().status,
        MemoryStatus::Stale
    );
    assert_eq!(store.get("mem_removed").unwrap().unwrap().confidence, 0.0);
    assert_eq!(
        store.get("mem_kept").unwrap().unwrap().status,
        MemoryStatus::Fresh
    );
}

#[test]
fn same_named_methods_in_one_file_do_not_collide_in_staleness() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/pair.rs"),
        "pub struct A;\npub struct B;\nimpl A { pub fn build() -> i32 { 1 } }\nimpl B { pub fn build() -> i32 { 2 } }\n",
    )
    .unwrap();
    commit_paths(&repo, "init", &["src/pair.rs"]);

    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    db.update_repo_info(&agent_core::types::RepoInfo {
        id: repo_id.clone(),
        name: "pair".into(),
        root_path: root.to_path_buf(),
        head_commit: None,
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    index_symbols(root, &repo_id, &mut db);

    // Edit only B::build while leaving A::build untouched.
    fs::write(
        root.join("src/pair.rs"),
        "pub struct A;\npub struct B;\nimpl A { pub fn build() -> i32 { 1 } }\nimpl B { pub fn build() -> i32 { 99 } }\n",
    )
    .unwrap();
    let store = MemoryStore::new(root.join("memories.jsonl"));
    let git_repo = GitRepo::open(root).unwrap();
    let report = StalenessEngine::evaluate(root, &repo_id, &git_repo, &db, &store).unwrap();
    assert!(report.changed_symbols.contains(&"build".to_string()));
}
