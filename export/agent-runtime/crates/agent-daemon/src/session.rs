use serde_json::Value;
use std::path::{Path, PathBuf};

/// Resolve the repository a daemon MCP request should operate on.
///
/// Order: tool argument `repo_path` or `path`, legacy `params.repo_path`,
/// the workspace captured from MCP `initialize`, the sole watched repository,
/// then the process default directory.
pub fn resolve_mcp_repo_path(
    req: &Value,
    session_repo: &mut Option<PathBuf>,
    watched: &[PathBuf],
    default_dir: &Path,
) -> PathBuf {
    if let Some(path) = argument_path(req).or_else(|| param_repo_path(req)) {
        *session_repo = Some(path.clone());
        return path;
    }

    if req["method"].as_str() == Some("initialize") {
        if let Some(path) = workspace_from_initialize(&req["params"]) {
            *session_repo = Some(path.clone());
            return path;
        }
    }

    if let Some(path) = session_repo.clone() {
        return path;
    }

    if watched.len() == 1 {
        let path = watched[0].clone();
        *session_repo = Some(path.clone());
        return path;
    }

    default_dir.to_path_buf()
}

fn argument_path(req: &Value) -> Option<PathBuf> {
    let args = &req["params"]["arguments"];
    let raw = args["repo_path"]
        .as_str()
        .or_else(|| args["path"].as_str())?;
    if raw.is_empty() {
        None
    } else {
        Some(PathBuf::from(raw))
    }
}

fn param_repo_path(req: &Value) -> Option<PathBuf> {
    let raw = req["params"]["repo_path"].as_str()?;
    if raw.is_empty() {
        None
    } else {
        Some(PathBuf::from(raw))
    }
}

fn workspace_from_initialize(params: &Value) -> Option<PathBuf> {
    if let Some(uri) = params.get("rootUri").and_then(|v| v.as_str()) {
        if let Some(path) = file_uri_to_path(uri) {
            return Some(path);
        }
    }
    if let Some(folders) = params.get("workspaceFolders").and_then(|v| v.as_array()) {
        for folder in folders {
            if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                if let Some(path) = file_uri_to_path(uri) {
                    return Some(path);
                }
            }
        }
    }
    if let Some(root) = params.get("rootPath").and_then(|v| v.as_str()) {
        if !root.is_empty() {
            return Some(PathBuf::from(root));
        }
    }
    None
}

pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path_part = rest.strip_prefix("localhost").unwrap_or(rest);
    if path_part.is_empty() {
        return None;
    }
    let decoded = percent_decode(path_part);
    if decoded.is_empty() {
        None
    } else {
        Some(PathBuf::from(decoded))
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    index += 3;
                    continue;
                }
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
