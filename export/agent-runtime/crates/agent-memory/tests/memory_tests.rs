use agent_core::types::{EvidenceItem, EvidenceMemory, MemoryKind, MemoryStatus, RepoId};
use agent_index::IndexDatabase;
use agent_memory::{MemoryStore, MemoryVerifier};
use chrono::Utc;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_memory_store_crud() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("memories.jsonl");
    let store = MemoryStore::new(file_path);

    let mem = EvidenceMemory {
        id: "mem_auth_17".to_string(),
        claim: "Refresh tokens are single use and rotated upon successful exchange".to_string(),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: "src/auth/refresh.rs".to_string(),
            symbols: vec!["rotate_refresh_token".to_string()],
            commit: "a812fe3".to_string(),
            fingerprint: None,
        }],
        valid_at: "a812fe3".to_string(),
        confidence: 1.0,
        status: MemoryStatus::Fresh,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    store.save(&mem).unwrap();

    let all = store.load_all().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, "mem_auth_17");

    let found = store.get("mem_auth_17").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().claim, mem.claim);

    let deleted = store.delete("mem_auth_17").unwrap();
    assert!(deleted);
    assert_eq!(store.load_all().unwrap().len(), 0);
}

#[test]
fn test_memory_verifier_staleness() {
    let dir = tempdir().unwrap();
    let repo_id = RepoId::from_path(dir.path());
    let db = IndexDatabase::open_in_memory().unwrap();

    let mem = EvidenceMemory {
        id: "mem_missing_file".to_string(),
        claim: "Non-existent file test".to_string(),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: "does_not_exist.rs".to_string(),
            symbols: vec!["fake_symbol".to_string()],
            commit: "1111111".to_string(),
            fingerprint: None,
        }],
        valid_at: "1111111".to_string(),
        confidence: 1.0,
        status: MemoryStatus::Fresh,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let verified =
        MemoryVerifier::verify(&mem, dir.path(), &repo_id, Some("2222222"), &db).unwrap();
    assert_eq!(verified.status, MemoryStatus::Stale);
    assert_eq!(verified.confidence, 0.0);
}

#[test]
fn test_invalidated_memory_stays_invalidated() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("exists.rs"), "fn main() {}\n").unwrap();
    let stamped = Utc::now() - chrono::Duration::hours(5);
    let mem = EvidenceMemory {
        id: "mem_retired".to_string(),
        claim: "obsolete claim".to_string(),
        kind: MemoryKind::ArchitecturalFact,
        evidence: vec![EvidenceItem {
            file: "exists.rs".to_string(),
            symbols: vec!["main".to_string()],
            commit: "abc".to_string(),
            fingerprint: None,
        }],
        valid_at: "abc".to_string(),
        confidence: 0.2,
        status: MemoryStatus::Invalidated,
        created_at: stamped,
        updated_at: stamped,
    };
    let repo_id = RepoId::from_path(dir.path());
    let db = IndexDatabase::open_in_memory().unwrap();
    let verified = MemoryVerifier::verify(&mem, dir.path(), &repo_id, Some("zzz"), &db).unwrap();
    assert_eq!(verified.status, MemoryStatus::Invalidated);
    assert_eq!(verified.confidence, 0.2);
    assert_eq!(verified.updated_at, stamped);
}

#[test]
fn test_concurrent_saves_keep_every_row() {
    let dir = tempdir().unwrap();
    let store = MemoryStore::new(dir.path().join("memories.jsonl"));
    std::thread::scope(|scope| {
        for index in 0..32 {
            let store = &store;
            scope.spawn(move || {
                let stamped = Utc::now();
                let mem = EvidenceMemory {
                    id: format!("id-{index}"),
                    claim: format!("claim {index}"),
                    kind: MemoryKind::Convention,
                    evidence: Vec::new(),
                    valid_at: "head".to_string(),
                    confidence: 1.0,
                    status: MemoryStatus::Fresh,
                    created_at: stamped,
                    updated_at: stamped,
                };
                store.save(&mem).unwrap();
            });
        }
    });
    let mut ids: Vec<_> = store
        .load_all()
        .unwrap()
        .into_iter()
        .map(|memory| memory.id)
        .collect();
    ids.sort();
    let mut expected: Vec<_> = (0..32).map(|index| format!("id-{index}")).collect();
    expected.sort();
    assert_eq!(ids, expected);
}
