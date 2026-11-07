use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::tools::{call_tool, list_tools};
use agent_core::Result;
use serde_json::json;
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub struct McpServer {
    pub repo_root: PathBuf,
}

impl McpServer {
    pub fn new(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }

    /// Process a single incoming JSON-RPC request and return response if needed.
    pub fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id.clone().unwrap_or(json!(null));

        match req.method.as_str() {
            "initialize" => {
                let result = json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "02-agent-runtime",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                });
                Some(JsonRpcResponse::success(id, result))
            }

            "notifications/initialized" => {
                // Notifications do not receive responses in JSON-RPC
                None
            }

            "ping" => Some(JsonRpcResponse::success(id, json!({}))),

            "tools/list" => {
                let tools = list_tools();
                Some(JsonRpcResponse::success(id, json!({ "tools": tools })))
            }

            "tools/call" => {
                let params = req.params.unwrap_or(json!({}));
                let tool_name = params["name"].as_str().unwrap_or("");
                let arguments = &params["arguments"];

                let call_result = call_tool(&self.repo_root, tool_name, arguments);
                let result_value = serde_json::to_value(&call_result).unwrap_or(json!({
                    "content": [{"type": "text", "text": "Serialization error"}],
                    "isError": true
                }));

                Some(JsonRpcResponse::success(id, result_value))
            }

            unknown => Some(JsonRpcResponse::error(
                id,
                -32601,
                format!("Method '{}' not found", unknown),
            )),
        }
    }

    /// Run stdio loop processing JSON-RPC messages line by line.
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

            let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
                Ok(req) => req,
                Err(e) => {
                    let err_resp =
                        JsonRpcResponse::error(json!(null), -32700, format!("Parse error: {}", e));
                    let _ = writeln!(writer, "{}", serde_json::to_string(&err_resp)?);
                    let _ = writer.flush();
                    continue;
                }
            };

            if let Some(resp) = self.handle_request(request) {
                let resp_str = serde_json::to_string(&resp)?;
                writeln!(writer, "{}", resp_str)?;
                writer.flush()?;
            }
        }

        Ok(())
    }
}
