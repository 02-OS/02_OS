use agent_core::{paths::StoragePaths, AgentError, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

pub enum AgentTarget {
    Codex,
    Claude,
    OpenCode,
    Cursor,
    Gemini,
    Zed,
    Generic,
}

impl AgentTarget {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "codex" => Self::Codex,
            "claude" => Self::Claude,
            "opencode" => Self::OpenCode,
            "cursor" => Self::Cursor,
            "gemini" | "gemini-cli" => Self::Gemini,
            "zed" => Self::Zed,
            _ => Self::Generic,
        }
    }

    pub fn default_config_path(&self) -> Option<PathBuf> {
        let home = StoragePaths::home_dir().ok()?;
        match self {
            Self::Codex => Some(home.join(".config/codex/mcp.json")),
            Self::Claude => Some(home.join(".config/Claude/claude_desktop_config.json")),
            Self::OpenCode => Some(home.join(".config/opencode/mcp.json")),
            Self::Cursor => Some(home.join(".cursor/mcp.json")),
            Self::Gemini => Some(home.join(".gemini/antigravity-cli/mcp_config.json")),
            Self::Zed => Some(home.join(".config/zed/settings.json")),
            Self::Generic => None,
        }
    }
}

pub struct AgentConnector;

impl AgentConnector {
    /// Generate MCP configuration snippet for target agent.
    pub fn generate_config(target: &AgentTarget) -> serde_json::Value {
        match target {
            AgentTarget::Zed => json!({
                "context_servers": {
                    "02": {
                        "source": "custom",
                        "command": "02",
                        "args": ["mcp"],
                        "env": {}
                    }
                }
            }),
            _ => json!({
                "mcpServers": {
                    "02": {
                        "command": "02",
                        "args": ["mcp"]
                    }
                }
            }),
        }
    }

    /// Connect and write or print the MCP configuration for a coding agent.
    pub fn connect(target_name: &str, write_to_file: bool) -> Result<String> {
        let target = AgentTarget::parse(target_name);
        let config = Self::generate_config(&target);
        let config_str = serde_json::to_string_pretty(&config)
            .map_err(|e| AgentError::General(format!("JSON serialization error: {}", e)))?;

        if write_to_file {
            if let Some(path) = target.default_config_path() {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }

                let final_json = if path.exists() {
                    let existing_text = fs::read_to_string(&path)?;
                    Self::merge_config_text(&target, &existing_text)?
                } else {
                    config_str.clone()
                };

                fs::write(&path, &final_json)?;
                let written: Value = serde_json::from_str(&final_json).map_err(|err| {
                    AgentError::Config(format!("written MCP config is not JSON: {err}"))
                })?;
                let key = if matches!(target, AgentTarget::Zed) {
                    "context_servers"
                } else {
                    "mcpServers"
                };
                if written
                    .get(key)
                    .and_then(|servers| servers.get("02"))
                    .is_none()
                {
                    return Err(AgentError::Config(format!(
                        "written MCP config is missing the 02 entry under {key}"
                    )));
                }
                return Ok(format!(
                    "Successfully configured '{}' MCP server in {}",
                    target_name,
                    path.display()
                ));
            }
        }

        Ok(config_str)
    }

    /// Merge the 02 server entry into an existing MCP JSON document.
    pub fn merge_config_text(target: &AgentTarget, existing_text: &str) -> Result<String> {
        let mut existing: Value = serde_json::from_str(existing_text)
            .map_err(|err| AgentError::Config(format!("invalid MCP config JSON: {err}")))?;
        let root = existing.as_object_mut().ok_or_else(|| {
            AgentError::Config("MCP config root must be a JSON object".to_string())
        })?;
        if matches!(target, AgentTarget::Zed) {
            insert_server(
                root,
                "context_servers",
                json!({
                    "source": "custom",
                    "command": "02",
                    "args": ["mcp"],
                    "env": {}
                }),
            )?;
        } else {
            insert_server(
                root,
                "mcpServers",
                json!({ "command": "02", "args": ["mcp"] }),
            )?;
        }
        let written = serde_json::to_string_pretty(&existing)
            .map_err(|err| AgentError::Config(format!("MCP config serialization failed: {err}")))?;
        let parsed: Value = serde_json::from_str(&written)
            .map_err(|err| AgentError::Config(format!("written MCP config is not JSON: {err}")))?;
        let key = if matches!(target, AgentTarget::Zed) {
            "context_servers"
        } else {
            "mcpServers"
        };
        if parsed
            .get(key)
            .and_then(|servers| servers.get("02"))
            .is_none()
        {
            return Err(AgentError::Config(format!(
                "written MCP config is missing the 02 entry under {key}"
            )));
        }
        Ok(written)
    }
}

fn insert_server(root: &mut serde_json::Map<String, Value>, key: &str, entry: Value) -> Result<()> {
    let servers = root.entry(key).or_insert_with(|| json!({}));
    let Some(map) = servers.as_object_mut() else {
        return Err(AgentError::Config(format!("{key} must be a JSON object")));
    };
    map.insert("02".to_string(), entry);
    Ok(())
}
