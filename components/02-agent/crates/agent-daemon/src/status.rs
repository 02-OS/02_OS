use agent_core::{
    error::AgentError, paths::StoragePaths, phase::phase_label, types::RepoId, Result,
};
use agent_git::GitRepo;
use agent_index::{ApprovalRequest, IndexDatabase, StoredApproval};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Local snapshot the Shell extension reads. It contains no tool arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusApproval {
    pub id: String,
    pub run_id: Option<String>,
    pub command_id: Option<String>,
    pub args_hash: String,
    pub decision: String,
    pub repo_id: String,
    pub repo_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    pub index_phase: String,
    pub watched_count: usize,
    pub approvals: Vec<StatusApproval>,
}

/// Write `status.json` for the session extension. Mode `0600`, replaced atomically.
pub fn write_status_file(path: &Path, report: &StatusReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            StoragePaths::ensure_private_dir(parent)?;
        }
    }
    let body = serde_json::to_vec_pretty(report)
        .map_err(|err| AgentError::General(format!("status json failed: {err}")))?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, body)?;
    let mut permissions = fs::metadata(&temporary)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&temporary, permissions)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn read_phase(status_path: &Path) -> String {
    let Some(parent) = status_path.parent() else {
        return "idle".to_string();
    };
    fs::read_to_string(parent.join("index-phase"))
        .map(|text| phase_label(&text).to_string())
        .unwrap_or_else(|_| "idle".to_string())
}

fn existing_index_db(repo_id: &RepoId) -> Result<Option<IndexDatabase>> {
    let path = StoragePaths::data_dir()?
        .join("repos")
        .join(repo_id.as_str())
        .join("index.sqlite");
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(IndexDatabase::open(&path)?))
}

/// Build the snapshot from the phase file and approval rows already stored.
pub fn publish_status_snapshot(status_path: &Path, watched: &[PathBuf]) -> Result<StatusReport> {
    let mut approvals = Vec::new();
    for path in watched {
        let Ok(repo) = GitRepo::open(path) else {
            continue;
        };
        let Ok(info) = repo.info() else {
            continue;
        };
        let Ok(Some(db)) = existing_index_db(&info.id) else {
            continue;
        };
        let Ok(rows) = db.list_approvals(&info.id) else {
            continue;
        };
        for row in rows {
            approvals.push(view_row(&info.id, path, row));
        }
    }
    let report = StatusReport {
        index_phase: read_phase(status_path),
        watched_count: watched.len(),
        approvals,
    };
    write_status_file(status_path, &report)?;
    Ok(report)
}

fn view_row(repo_id: &RepoId, path: &Path, row: StoredApproval) -> StatusApproval {
    StatusApproval {
        id: row.id,
        run_id: row.run_id,
        command_id: row.command_id,
        args_hash: row.args_hash,
        decision: row.decision,
        repo_id: repo_id.as_str().to_string(),
        repo_path: path.display().to_string(),
    }
}

/// Session file `~/.local/share/02-agent/status.json`.
pub fn publish_daemon_status(watched: &[PathBuf]) -> Result<StatusReport> {
    let directory = StoragePaths::data_dir()?;
    publish_status_snapshot(&directory.join("status.json"), watched)
}

/// A gnome decision may be stored only when the caller repeats the displayed hash.
/// The source is always `gnome`. This function does not recompute the hash.
pub fn approval_from_confirmation(params: &Value) -> Result<ApprovalRequest> {
    let args_hash = text_field(params, "args_hash");
    let confirmed = text_field(params, "confirmed_hash");
    if args_hash.is_empty() || !confirmed.eq_ignore_ascii_case(&args_hash) {
        return Err(AgentError::Security(
            "confirmed hash does not match the displayed args hash".into(),
        ));
    }
    let request = ApprovalRequest {
        id: text_field(params, "id"),
        agt_run_id: optional_text(params, "agt_run_id"),
        agt_command_id: optional_text(params, "agt_command_id"),
        args_hash,
        decision: text_field(params, "decision"),
        approver: text_field(params, "approver"),
        source: "gnome".to_string(),
        invoker: text_field(params, "invoker"),
    };
    if request.id.trim().is_empty() {
        return Err(AgentError::Security(
            "gnome decision requires an approval id".into(),
        ));
    }
    agent_index::admit_approval(&request)?;
    Ok(request)
}

fn text_field(params: &Value, key: &str) -> String {
    params
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn optional_text(params: &Value, key: &str) -> Option<String> {
    let text = text_field(params, key);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Record a confirmed gnome decision against an indexed repository.
pub fn apply_gnome_decision(params: &Value) -> Result<String> {
    let request = approval_from_confirmation(params)?;
    let repo_path = text_field(params, "repo_path");
    if repo_path.is_empty() {
        return Err(AgentError::General(
            "gnome decision requires repo_path".into(),
        ));
    }
    let info = GitRepo::open(&repo_path)?.info()?;
    let Some(db) = existing_index_db(&info.id)? else {
        return Err(AgentError::RepositoryNotInitialized);
    };
    let id = request.id.clone();
    db.record_approval(&info.id, &request)?;
    Ok(id)
}

pub fn status_rpc(id: Value, watched: &[PathBuf]) -> Value {
    match publish_daemon_status(watched) {
        Ok(report) => json!({ "jsonrpc": "2.0", "id": id, "result": report }),
        Err(err) => rpc_error(id, err),
    }
}

pub fn decide_rpc(id: Value, params: &Value, watched: &[PathBuf]) -> Value {
    match apply_gnome_decision(params) {
        Ok(recorded) => {
            let _ = publish_daemon_status(watched);
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "recorded": recorded, "source": "gnome" }
            })
        }
        Err(err) => rpc_error(id, err),
    }
}

fn rpc_error(id: Value, err: AgentError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32000, "message": err.to_string() }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::RepoInfo;
    use std::os::unix::fs::PermissionsExt;

    fn sample_params(confirmed: &str) -> Value {
        json!({
            "id": "gnome-1",
            "agt_run_id": "run-1",
            "agt_command_id": "cmd-1",
            "args_hash": "ab".repeat(32),
            "confirmed_hash": confirmed,
            "decision": "deny",
            "approver": "person",
            "invoker": "agent",
            "source": "mcp"
        })
    }

    #[test]
    fn status_file_is_private_and_reports_the_phase_line() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("index-phase"), "indexing\n").unwrap();
        let path = directory.path().join("status.json");
        let report = StatusReport {
            index_phase: "indexing".into(),
            watched_count: 2,
            approvals: Vec::new(),
        };
        write_status_file(&path, &report).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let stored: StatusReport = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored, report);
        assert_eq!(read_phase(&path), "indexing");
    }

    #[test]
    fn gnome_decision_requires_the_displayed_hash_and_forces_gnome_source() {
        let rejected = approval_from_confirmation(&sample_params(&"ff".repeat(32))).unwrap_err();
        assert!(rejected.to_string().contains("displayed args hash"));

        let request = approval_from_confirmation(&sample_params(&"AB".repeat(32))).unwrap();
        assert_eq!(request.source, "gnome");
        assert_eq!(request.decision, "deny");
        assert_eq!(request.invoker, "agent");

        let mut db = IndexDatabase::open_in_memory().unwrap();
        let repo = RepoId::new("repo");
        db.update_repo_info(&RepoInfo {
            id: repo.clone(),
            name: "demo".into(),
            root_path: "/tmp/demo".into(),
            head_commit: None,
            branch: None,
            is_clean: true,
            modified_count: 0,
            untracked_count: 0,
        })
        .unwrap();
        db.record_approval(&repo, &request).unwrap();
        let rows = db.list_approvals(&repo).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].decision, "deny");
        assert_eq!(rows[0].args_hash, "ab".repeat(32));
        assert_eq!(rows[0].command_id.as_deref(), Some("cmd-1"));
    }

    #[test]
    fn mcp_approver_is_rejected_even_when_the_hash_matches() {
        let mut params = sample_params(&"ab".repeat(32));
        params["approver"] = json!("mcp");
        let err = approval_from_confirmation(&params).unwrap_err();
        assert!(err.to_string().contains("cannot approve"));
    }
}
