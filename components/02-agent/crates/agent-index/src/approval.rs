use agent_core::{AgentError, Result};

/// One stored approval row. The hash is the digest agt already computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredApproval {
    pub id: String,
    pub run_id: Option<String>,
    pub command_id: Option<String>,
    pub args_hash: String,
    pub decision: String,
}

/// A decision recorded for an agt command. This runtime does not recompute
/// `args_hash`. agt is the process that checks the hash.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub id: String,
    pub agt_run_id: Option<String>,
    pub agt_command_id: Option<String>,
    pub args_hash: String,
    pub decision: String,
    pub approver: String,
    pub source: String,
    /// Actor that invoked the tool. An actor cannot allow its own call.
    pub invoker: String,
}

/// Refuse a client that approves itself. MCP is not an approval source.
pub fn admit_approval(request: &ApprovalRequest) -> Result<()> {
    if request.invoker.trim().is_empty() || request.approver.trim().is_empty() {
        return Err(AgentError::Security(
            "approval requires an invoker and an approver".into(),
        ));
    }
    if request.invoker == request.approver || request.source == "mcp" || request.approver == "mcp" {
        return Err(AgentError::Security(
            "mcp client cannot approve its own call".into(),
        ));
    }
    if !matches!(request.source.as_str(), "agt-cli" | "gnome" | "02-cli") {
        return Err(AgentError::Security(
            "approval source must be agt, the shell, or the 02 cli".into(),
        ));
    }
    if !matches!(request.decision.as_str(), "allow" | "deny") {
        return Err(AgentError::Security(
            "approval decision must be allow or deny".into(),
        ));
    }
    let hash = request.args_hash.trim();
    let shaped = hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !shaped {
        return Err(AgentError::Security(
            "approval args hash must be the digest agt computed".into(),
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
            head_commit: None,
            branch: None,
            is_clean: true,
            modified_count: 0,
            untracked_count: 0,
        })
        .unwrap();
        (db, id)
    }

    fn sample(invoker: &str, approver: &str, source: &str) -> ApprovalRequest {
        ApprovalRequest {
            id: format!("{invoker}-{approver}"),
            agt_run_id: Some("run-1".into()),
            agt_command_id: Some("cmd-1".into()),
            args_hash: "ab".repeat(32),
            decision: "allow".into(),
            approver: approver.into(),
            source: source.into(),
            invoker: invoker.into(),
        }
    }

    #[test]
    fn mcp_self_approval_is_not_stored() {
        let (db, repo) = open();
        let err = db
            .record_approval(&repo, &sample("mcp", "mcp", "mcp"))
            .unwrap_err();
        assert!(err.to_string().contains("cannot approve"));
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM approvals", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        db.record_approval(&repo, &sample("agent", "person", "agt-cli"))
            .unwrap();
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM approvals", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
