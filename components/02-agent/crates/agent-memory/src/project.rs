use crate::facts::Fact;
use agent_core::{RepoInfo, Result};
use agent_index::IndexDatabase;

/// Copy decayed facts into `memory_index`.
///
/// The row id is prefixed with `fact:` so it cannot replace an evidence
/// memory. `memories.jsonl` stays the authority. `jsonl_offset` stays null.
pub fn project_facts(db: &IndexDatabase, info: &RepoInfo, facts: &[Fact]) -> Result<usize> {
    db.note_repo_presence(info)?;
    let mut stored = 0;
    for fact in facts {
        if fact.repo_id != info.id.as_str() {
            continue;
        }
        let memory_id = if fact.id.starts_with("fact:") {
            fact.id.clone()
        } else {
            format!("fact:{}", fact.id)
        };
        db.project_fact_row(&info.id, &memory_id, fact.confidence, &fact.decayed_at)?;
        stored += 1;
    }
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use agent_core::{EvidenceMemory, MemoryKind, MemoryStatus, RepoId, RepoInfo};
    use chrono::Utc;

    #[test]
    fn projection_does_not_become_the_memory_record() {
        let directory = tempfile::tempdir().unwrap();
        let db = IndexDatabase::open_in_memory().unwrap();
        let repo_id = RepoId::new("repo-facts");
        let info = RepoInfo {
            id: repo_id.clone(),
            name: "demo".into(),
            root_path: directory.path().to_path_buf(),
            head_commit: Some("abc".into()),
            branch: Some("master".into()),
            is_clean: true,
            modified_count: 0,
            untracked_count: 0,
        };
        let fact = Fact {
            id: "abc123".into(),
            repo_id: repo_id.as_str().to_string(),
            claim: "symbol main is defined in src/main.rs".into(),
            relative_path: "src/main.rs".into(),
            symbol: Some("main".into()),
            commit_id: "abc".into(),
            base_confidence: 0.8,
            confidence: 0.4,
            source: "ast".into(),
            anchored_at: "2026-01-01T00:00:00Z".into(),
            decayed_at: "2026-02-01T00:00:00Z".into(),
        };
        assert_eq!(project_facts(&db, &info, &[fact]).unwrap(), 1);
        assert_eq!(
            db.projected_confidence(&repo_id, "fact:abc123")
                .unwrap()
                .unwrap(),
            0.4
        );

        let path = directory.path().join("memories.jsonl");
        let store = MemoryStore::new(path.clone());
        store
            .save(&EvidenceMemory {
                id: "mem-1".into(),
                claim: "tokens rotate".into(),
                kind: MemoryKind::ArchitecturalFact,
                evidence: Vec::new(),
                valid_at: "abc".into(),
                confidence: 0.9,
                status: MemoryStatus::Fresh,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].claim, "tokens rotate");
        let log = std::fs::read_to_string(path).unwrap();
        assert!(!log.contains("fact:abc123"));
        assert!(!log.contains("symbol main"));
    }
}
