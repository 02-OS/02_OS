use agent_core::types::{EvidenceItem, EvidenceMemory, MemoryKind, MemoryStatus, RepoId, RepoInfo};
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

    let mut newer = mem.clone();
    newer.claim = "A later writer stored this claim".to_string();
    newer.updated_at = Utc::now();
    store.save(&newer).unwrap();
    let refused = store.save_if_unchanged(&mem, mem.updated_at).unwrap_err();
    assert!(refused.to_string().contains("stale memory write"));
    assert_eq!(
        store.get("mem_auth_17").unwrap().unwrap().claim,
        "A later writer stored this claim"
    );

    let all = store.load_all().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, "mem_auth_17");

    let found = store.get("mem_auth_17").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().claim, "A later writer stored this claim");

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

#[test]
fn memory_store_redacts_new_and_legacy_claims_without_changing_evidence() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("memories.jsonl");
    let store = MemoryStore::new(path.clone());
    let token = format!("ghp_{}", "a".repeat(36));
    let memory = EvidenceMemory {
        id: "keep-id".into(),
        claim: format!("auditprobe uses {token}"),
        kind: MemoryKind::SecurityConstraint,
        evidence: vec![EvidenceItem {
            file: "src/auth.rs".into(),
            symbols: vec!["authenticate".into()],
            commit: "abc".into(),
            fingerprint: Some("recorded-fingerprint".into()),
        }],
        valid_at: "abc".into(),
        confidence: 0.0,
        status: MemoryStatus::Invalidated,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    // Simulate a store written before the redaction boundary existed.
    fs::write(
        &path,
        format!("{}\n", serde_json::to_string(&memory).unwrap()),
    )
    .unwrap();
    let loaded = store.get("keep-id").unwrap().unwrap();
    assert_eq!(loaded.claim, "auditprobe uses [REDACTED]");
    assert_eq!(loaded.id, memory.id);
    assert_eq!(loaded.evidence[0].file, memory.evidence[0].file);
    assert_eq!(
        loaded.evidence[0].fingerprint,
        memory.evidence[0].fingerprint
    );
    assert_eq!(loaded.status, MemoryStatus::Invalidated);
    store.save(&memory).unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains(&token));
    assert!(fs::read_to_string(&path).unwrap().contains("[REDACTED]"));
}

#[test]
fn seed_names_02_os_and_uses_the_runtime_cargo_path_that_exists() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("profile")).unwrap();
    fs::write(dir.path().join("profile/profiledef.sh"), "iso_name=02_OS\n").unwrap();
    fs::create_dir_all(dir.path().join("vendor/agent-runtime")).unwrap();
    fs::write(
        dir.path().join("vendor/agent-runtime/Cargo.toml"),
        "[workspace]\n",
    )
    .unwrap();

    let store = MemoryStore::new(dir.path().join("memories.jsonl"));
    let info = RepoInfo {
        id: RepoId::from_path(dir.path()),
        name: "fixture".to_string(),
        root_path: dir.path().to_path_buf(),
        head_commit: Some("abc123".to_string()),
        branch: Some("master".to_string()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    };
    assert_eq!(store.seed_initial_if_empty(&info).unwrap(), 2);
    let all = store.load_all().unwrap();
    let profile = all
        .iter()
        .find(|memory| memory.id == "mem_archiso_profile")
        .unwrap();
    assert_eq!(
        profile.claim,
        "02_OS is a live image built with mkarchiso from profile/"
    );
    assert!(!profile.claim.contains("Arch Linux"));
    let runtime = all
        .iter()
        .find(|memory| memory.id == "mem_agent_runtime")
        .unwrap();
    assert_eq!(runtime.evidence[0].file, "vendor/agent-runtime/Cargo.toml");

    fs::create_dir_all(dir.path().join("components/02-agent")).unwrap();
    fs::write(
        dir.path().join("components/02-agent/Cargo.toml"),
        "[workspace]\n",
    )
    .unwrap();
    let preferred = MemoryStore::new(dir.path().join("other-memories.jsonl"));
    preferred.seed_initial_if_empty(&info).unwrap();
    let preferred_runtime = preferred
        .load_all()
        .unwrap()
        .into_iter()
        .find(|memory| memory.id == "mem_agent_runtime")
        .unwrap();
    assert_eq!(
        preferred_runtime.evidence[0].file,
        "components/02-agent/Cargo.toml"
    );
    assert!(preferred_runtime.claim.contains("components/02-agent/"));
}
