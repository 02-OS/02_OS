use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::tools::{apply_policy_to_package, call_tool_with_policy, list_tools};
use agent_context::ContextCompiler;
use agent_core::{
    config::RepoConfig, contain::read_regular_text_within, paths::StoragePaths, Result,
    SecretPattern,
};
use agent_git::GitRepo;
use agent_index::IndexDatabase;
use agent_skills::SessionPolicy;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub struct McpServer {
    pub repo_root: PathBuf,
    allowed_roots: Vec<PathBuf>,
    policy: Option<SessionPolicy>,
}

impl McpServer {
    pub fn new(repo_root: PathBuf) -> Self {
        Self {
            allowed_roots: vec![repo_root.clone()],
            repo_root,
            policy: None,
        }
    }

    pub fn with_allowed_roots(repo_root: PathBuf, allowed_roots: Vec<PathBuf>) -> Self {
        let mut roots = allowed_roots;
        if !roots.iter().any(|r| r == &repo_root) {
            roots.push(repo_root.clone());
        }
        Self {
            repo_root,
            allowed_roots: roots,
            policy: None,
        }
    }

    pub fn with_policy(mut self, policy: SessionPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Process either a single JSON-RPC request object or a JSON-RPC 2.0 batch array (#154).
    pub fn handle_json(&self, value: Value) -> Option<Value> {
        match value {
            Value::Array(items) => {
                if items.is_empty() {
                    return serde_json::to_value(invalid_request(Value::Null)).ok();
                }
                let responses: Vec<Value> = items
                    .into_iter()
                    .filter_map(|item| {
                        self.handle_message(item)
                            .and_then(|resp| serde_json::to_value(resp).ok())
                    })
                    .collect();
                if responses.is_empty() {
                    None
                } else {
                    Some(Value::Array(responses))
                }
            }
            other => self
                .handle_message(other)
                .and_then(|resp| serde_json::to_value(resp).ok()),
        }
    }

    /// Validate a JSON-RPC envelope and dispatch it.
    ///
    /// A message with no `id` member is a notification and produces no response,
    /// including unknown methods and bad versions. Parse errors are handled by
    /// `run_stdio` before this is called.
    pub fn handle_message(&self, value: Value) -> Option<JsonRpcResponse> {
        let Some(obj) = value.as_object() else {
            return Some(invalid_request(Value::Null));
        };
        if !obj.contains_key("id") {
            return None;
        }
        let id_val = obj.get("id").cloned().unwrap_or(Value::Null);
        let id_ok = id_val.is_null() || id_val.is_string() || id_val.is_number();
        let response_id = if id_ok { id_val.clone() } else { Value::Null };
        let jsonrpc_ok = obj.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0");
        let method = obj.get("method").and_then(|m| m.as_str());
        let params_ok = match obj.get("params") {
            None => true,
            Some(params) if params.is_array() || params.is_object() => true,
            Some(_) => false,
        };
        if !jsonrpc_ok || !id_ok || !params_ok || method.map(|m| m.is_empty()).unwrap_or(true) {
            return Some(invalid_request(response_id));
        }
        self.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id_val),
            method: method.unwrap().to_string(),
            params: obj.get("params").cloned(),
        })
    }

    /// Process a single incoming JSON-RPC request and return a response if needed.
    pub fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id.clone()?;
        if req.jsonrpc != "2.0" || req.method.is_empty() {
            return Some(invalid_request(id));
        }

        match req.method.as_str() {
            "initialize" => {
                let result = json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {},
                        "resources": {}
                    },
                    "serverInfo": {
                        "name": "02-agent-runtime",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                });
                Some(JsonRpcResponse::success(id, result))
            }

            "notifications/initialized" => None,

            "ping" => Some(JsonRpcResponse::success(id, json!({}))),

            "tools/list" => {
                if let Some(ref policy) = self.policy {
                    if let Err(err) = policy.authorize_token() {
                        return Some(JsonRpcResponse::error(id, -32001, err.to_string()));
                    }
                }
                let mut tools = list_tools();
                if let Some(ref policy) = self.policy {
                    tools.retain(|tool| {
                        policy
                            .skill
                            .tools
                            .iter()
                            .any(|allowed| allowed == &tool.name)
                    });
                }
                Some(JsonRpcResponse::success(id, json!({ "tools": tools })))
            }

            "tools/call" => {
                let params = req.params.unwrap_or(json!({}));
                let tool_name = params["name"].as_str().unwrap_or("");
                let arguments = &params["arguments"];

                let call_result = call_tool_with_policy(
                    &self.repo_root,
                    &self.allowed_roots,
                    self.policy.as_ref(),
                    tool_name,
                    arguments,
                );
                let result_value = serde_json::to_value(&call_result).unwrap_or(json!({
                    "content": [{"type": "text", "text": "Serialization error"}],
                    "isError": true
                }));

                Some(JsonRpcResponse::success(id, result_value))
            }

            "resources/list" => Some(self.handle_resources_list(id)),

            "resources/read" => Some(self.handle_resources_read(id, req.params.as_ref())),

            unknown => Some(JsonRpcResponse::error(
                id,
                -32601,
                format!("Method '{}' not found", unknown),
            )),
        }
    }

    fn handle_resources_list(&self, id: Value) -> JsonRpcResponse {
        if let Some(ref policy) = self.policy {
            if let Err(err) = policy.authorize_token() {
                return JsonRpcResponse::error(id, -32001, err.to_string());
            }
        }
        let mut resources = vec![
            json!({
                "uri": "02://context",
                "name": "Repository Context",
                "mimeType": "text/markdown"
            }),
            json!({
                "uri": "02://status",
                "name": "Repository Status",
                "mimeType": "application/json"
            }),
        ];
        if let Ok(git) = GitRepo::open(&self.repo_root) {
            let repo_id = agent_core::RepoId::from_path(git.root_path());
            if let Some(ref policy) = self.policy {
                if !policy.allows_repository(repo_id.as_str()) {
                    return JsonRpcResponse::error(
                        id,
                        -32001,
                        "context access denied for scope".into(),
                    );
                }
            }
            if let Ok(db_path) = StoragePaths::repo_db_path(&repo_id) {
                if db_path.is_file() {
                    if let Ok(db) = IndexDatabase::open(&db_path) {
                        if let Ok(files) = db.list_files(&repo_id) {
                            for file in files.into_iter().take(200) {
                                if SecretPattern::is_secret(Path::new(&file.relative_path)) {
                                    continue;
                                }
                                resources.push(json!({
                                    "uri": format!("02://repo/{}", file.relative_path),
                                    "name": file.relative_path,
                                    "mimeType": "text/plain"
                                }));
                            }
                        }
                    }
                }
            }
        }
        JsonRpcResponse::success(id, json!({ "resources": resources }))
    }

    fn handle_resources_read(&self, id: Value, params: Option<&Value>) -> JsonRpcResponse {
        if let Some(ref policy) = self.policy {
            if let Err(err) = policy.authorize_token() {
                return JsonRpcResponse::error(id, -32001, err.to_string());
            }
        }
        let Some(uri) = params.and_then(|p| p.get("uri")).and_then(|u| u.as_str()) else {
            return JsonRpcResponse::error(id, -32602, "resources/read requires params.uri".into());
        };
        let git = match GitRepo::open(&self.repo_root) {
            Ok(git) => git,
            Err(err) => return JsonRpcResponse::error(id, -32000, format!("Git error: {err}")),
        };
        let root = git.root_path();
        let info = match git.info() {
            Ok(info) => info,
            Err(err) => {
                return JsonRpcResponse::error(id, -32000, format!("Repo info error: {err}"))
            }
        };
        if let Some(ref policy) = self.policy {
            if !policy.allows_repository(info.id.as_str()) {
                return JsonRpcResponse::error(
                    id,
                    -32001,
                    "context access denied for scope".into(),
                );
            }
        }

        if uri == "02://context" {
            return match ContextCompiler::compile(root, "repository overview", 8000) {
                Ok(mut package) => {
                    apply_policy_to_package(&info.id, self.policy.as_ref(), &mut package);
                    JsonRpcResponse::success(
                        id,
                        json!({
                            "contents": [{
                                "uri": uri,
                                "mimeType": "text/markdown",
                                "text": package.to_markdown()
                            }]
                        }),
                    )
                }
                Err(err) => JsonRpcResponse::error(id, -32000, err.to_string()),
            };
        }
        if uri == "02://status" {
            let db_path = match StoragePaths::repo_db_path(&info.id) {
                Ok(p) => p,
                Err(err) => return JsonRpcResponse::error(id, -32000, err.to_string()),
            };
            let stats = IndexDatabase::open(&db_path).and_then(|db| db.get_stats(&info.id));
            let body = match stats {
                Ok(stats) => json!({
                    "repository": info.name,
                    "repo_id": info.id.as_str(),
                    "branch": info.branch,
                    "head_commit": info.head_commit,
                    "is_clean": info.is_clean,
                    "total_indexed_files": stats.total_files,
                    "total_symbols": stats.total_symbols,
                    "total_references": stats.total_references,
                }),
                Err(err) => return JsonRpcResponse::error(id, -32000, err.to_string()),
            };
            return JsonRpcResponse::success(
                id,
                json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": body.to_string()
                    }]
                }),
            );
        }
        if let Some(rel) = uri.strip_prefix("02://repo/") {
            if rel.is_empty() {
                return JsonRpcResponse::error(id, -32602, "resource path is empty".into());
            }
            let max_bytes = RepoConfig::load_or_default(root).max_file_size_kb * 1024;
            return match read_regular_text_within(root, Path::new(rel), max_bytes) {
                Ok(text) => {
                    let redacted = match SecretPattern::redact_cow(&text) {
                        Cow::Borrowed(s) => s.to_string(),
                        Cow::Owned(s) => s,
                    };
                    JsonRpcResponse::success(
                        id,
                        json!({
                            "contents": [{
                                "uri": uri,
                                "mimeType": "text/plain",
                                "text": redacted
                            }]
                        }),
                    )
                }
                Err(err) => JsonRpcResponse::error(id, -32002, err.to_string()),
            };
        }

        JsonRpcResponse::error(id, -32602, format!("unknown resource uri: {uri}"))
    }

    /// Run stdio loop processing JSON-RPC messages line by line (supports batches).
    pub fn run_stdio<R: BufRead, W: Write>(&self, reader: R, mut writer: W) -> Result<()> {
        for line in reader.lines() {
            let line_content = match line {
                Ok(l) => l,
                Err(_) => break,
            };

            let trimmed = line_content.trim();
            if trimmed.is_empty() {
                continue;
            }

            let value: Value = match serde_json::from_str(trimmed) {
                Ok(value) => value,
                Err(err) => {
                    let err_resp =
                        JsonRpcResponse::error(json!(null), -32700, format!("Parse error: {err}"));
                    let _ = writeln!(writer, "{}", serde_json::to_string(&err_resp)?);
                    let _ = writer.flush();
                    continue;
                }
            };

            if let Some(resp) = self.handle_json(value) {
                let resp_str = serde_json::to_string(&resp)?;
                writeln!(writer, "{}", resp_str)?;
                writer.flush()?;
            }
        }

        Ok(())
    }
}

fn invalid_request(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::error(id, -32600, "Invalid Request".to_string())
}
