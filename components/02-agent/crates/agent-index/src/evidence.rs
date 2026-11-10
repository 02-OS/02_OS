use agent_core::{AgentError, Result};
use sha2::{Digest, Sha256};

/// An agt evidence package submitted for storage.
///
/// `package_digest` must be the SHA-256 hex of `package_json` bytes. This
/// function does not apply the package and does not accept an applied ask.
#[derive(Debug, Clone)]
pub struct EvidenceIngest {
    pub agt_run_id: String,
    pub task_type: String,
    pub interaction: String,
    pub outcome: String,
    pub contract_digest: String,
    pub verification_digest: String,
    pub package_json: String,
    pub package_digest: String,
    pub head_commit: Option<String>,
}

/// SHA-256 hex of the package bytes, lowercase.
pub fn digest_package(package_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(package_json.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn admit_evidence(package: &EvidenceIngest) -> Result<()> {
    if package.agt_run_id.trim().is_empty() {
        return Err(AgentError::Security("evidence run id is empty".into()));
    }
    if package.contract_digest.trim().is_empty() {
        return Err(AgentError::Security(
            "evidence contract digest is empty".into(),
        ));
    }
    if package.package_json.is_empty() {
        return Err(AgentError::Security("evidence package is empty".into()));
    }
    if package.outcome == "applied" && (package.task_type == "ask" || package.interaction == "ask")
    {
        return Err(AgentError::Security(
            "an ask cannot be ingested as applied".into(),
        ));
    }
    if !matches!(
        package.task_type.as_str(),
        "repair" | "feature" | "refactor" | "ask" | "plan"
    ) {
        return Err(AgentError::Security(
            "evidence task type is not recognized".into(),
        ));
    }
    if !matches!(package.interaction.as_str(), "agent" | "ask" | "plan") {
        return Err(AgentError::Security(
            "evidence interaction is not recognized".into(),
        ));
    }
    let actual = digest_package(&package.package_json);
    if !actual.eq_ignore_ascii_case(package.package_digest.trim()) {
        return Err(AgentError::Security(
            "evidence package digest does not match".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndexDatabase;
    use agent_core::{RepoId, RepoInfo};

    fn open() -> (IndexDatabase, RepoId) {
        let mut db = IndexDatabase::open_in_memory().unwrap();
        let id = RepoId::new("repo");
        db.update_repo_info(&RepoInfo {
            id: id.clone(),
            name: "demo".into(),
            root_path: "/tmp/demo".into(),
            head_commit: Some("abc".into()),
            branch: Some("master".into()),
            is_clean: true,
            modified_count: 0,
            untracked_count: 0,
        })
        .unwrap();
        (db, id)
    }

    fn package(outcome: &str, task: &str, interaction: &str, digest: &str) -> EvidenceIngest {
        EvidenceIngest {
            agt_run_id: "run-1".into(),
            task_type: task.into(),
            interaction: interaction.into(),
            outcome: outcome.into(),
            contract_digest: "contract".into(),
            verification_digest: String::new(),
            package_json: "{\"ok\":true}".into(),
            package_digest: digest.into(),
            head_commit: Some("abc".into()),
        }
    }

    #[test]
    fn ingest_requires_a_matching_digest_and_rejects_an_applied_ask() {
        let (db, repo) = open();
        let body = "{\"ok\":true}";
        let digest = digest_package(body);
        let mut empty = package("repaired", "repair", "agent", &digest);
        empty.contract_digest.clear();
        assert!(db.ingest_evidence(&repo, &empty).is_err());

        let ask = package("applied", "ask", "ask", &digest);
        assert!(db.ingest_evidence(&repo, &ask).is_err());

        let wrong = package("repaired", "repair", "agent", &"ab".repeat(32));
        assert!(db.ingest_evidence(&repo, &wrong).is_err());
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM evidence_packages", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);

        let good = package("repaired", "repair", "agent", &digest);
        db.ingest_evidence(&repo, &good).unwrap();
        db.ingest_evidence(&repo, &good).unwrap();
        let mut changed = good;
        changed.package_json = "{\"ok\":false}".into();
        changed.package_digest = digest_package(&changed.package_json);
        assert!(db.ingest_evidence(&repo, &changed).is_err());
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM evidence_packages", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}
