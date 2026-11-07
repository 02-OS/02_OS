use agent_core::{
    config::RepoConfig,
    types::{EvidenceItem, EvidenceMemory, MemoryKind, MemoryStatus, RepoId, RepoInfo},
};
use agent_index::{IndexDatabase, RepoScanner};
use agent_memory::MemoryVerifier;
use chrono::Utc;
use std::fs;
use tempfile::tempdir;

fn memory(file: &str, symbol: &str, fingerprint: Option<String>) -> EvidenceMemory {
    EvidenceMemory {
        id: "mem".into(),
        claim: "audit stays local".into(),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: file.into(),
            symbols: vec![symbol.into()],
            commit: "abc".into(),
            fingerprint,
        }],
        valid_at: "abc".into(),
        confidence: 1.0,
        status: MemoryStatus::Fresh,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn verifier_requires_the_symbol_in_the_cited_file() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/a.rs"), "pub fn audit() {}\n").unwrap();
    fs::write(root.join("src/b.rs"), "pub fn auditmarker() {}\n").unwrap();
    let repo_id = RepoId::from_path(root);
    let mut db = IndexDatabase::open_in_memory().unwrap();
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "verify".into(),
        root_path: root.to_path_buf(),
        head_commit: Some("abc".into()),
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
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

    let wrong = MemoryVerifier::verify(
        &memory("src/b.rs", "audit", None),
        root,
        &repo_id,
        Some("abc"),
        &db,
    )
    .unwrap();
    assert_eq!(wrong.status, MemoryStatus::Stale);
    assert_eq!(wrong.confidence, 0.0);

    let right = MemoryVerifier::verify(
        &memory("src/a.rs", "audit", None),
        root,
        &repo_id,
        Some("abc"),
        &db,
    )
    .unwrap();
    assert_eq!(right.status, MemoryStatus::Fresh);

    let recorded = symbols
        .iter()
        .find(|symbol| symbol.name == "audit")
        .unwrap()
        .fingerprint
        .clone();
    let degraded = MemoryVerifier::verify(
        &memory("src/a.rs", "audit", Some(format!("{recorded}-other"))),
        root,
        &repo_id,
        Some("abc"),
        &db,
    )
    .unwrap();
    assert_eq!(degraded.status, MemoryStatus::Degraded);
    assert_eq!(degraded.confidence, 0.70);
}
