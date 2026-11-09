use agent_core::{paths::StoragePaths, AgentError, Result};
use serde_json::{json, Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static CONFIG_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

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
            "claude" | "claude-code" | "claudecode" => Self::Claude,
            "opencode" => Self::OpenCode,
            "cursor" => Self::Cursor,
            "gemini" | "gemini-cli" => Self::Gemini,
            "zed" => Self::Zed,
            _ => Self::Generic,
        }
    }

    pub fn config_path(&self, home: &Path) -> Option<PathBuf> {
        self.config_path_in(home, &home.join(".config"))
    }

    fn config_path_in(&self, home: &Path, config_home: &Path) -> Option<PathBuf> {
        match self {
            Self::Codex => Some(home.join(".codex/config.toml")),
            Self::Claude => Some(home.join(".claude.json")),
            Self::OpenCode => Some(config_home.join("opencode/opencode.json")),
            Self::Cursor => Some(home.join(".cursor/mcp.json")),
            Self::Gemini => Some(home.join(".gemini/settings.json")),
            Self::Zed => Some(config_home.join("zed/settings.json")),
            Self::Generic => None,
        }
    }

    pub fn default_config_path(&self) -> Option<PathBuf> {
        let home = StoragePaths::home_dir().ok()?;
        self.config_path_in(&home, &xdg_config_home(&home))
    }
}

pub struct AgentConnector;

impl AgentConnector {
    /// Logical MCP snippet. Codex is represented as JSON here; the file format is TOML.
    pub fn generate_config(target: &AgentTarget) -> Value {
        Self::generate_config_for(target, "02")
    }

    fn generate_config_for(target: &AgentTarget, command: &str) -> Value {
        match target {
            AgentTarget::Zed => json!({
                "context_servers": {
                    "02": {
                        "source": "custom",
                        "command": command,
                        "args": ["mcp"],
                        "env": {}
                    }
                }
            }),
            AgentTarget::OpenCode => json!({
                "mcp": {
                    "02": {
                        "type": "local",
                        "command": [command, "mcp"]
                    }
                }
            }),
            AgentTarget::Codex => json!({
                "mcp_servers": {
                    "02": {
                        "command": command,
                        "args": ["mcp"]
                    }
                }
            }),
            _ => json!({
                "mcpServers": {
                    "02": {
                        "command": command,
                        "args": ["mcp"]
                    }
                }
            }),
        }
    }

    /// Client-native text (TOML for Codex, JSON otherwise).
    pub fn render_config(target: &AgentTarget) -> Result<String> {
        Self::render_config_for(target, "02")
    }

    fn render_config_for(target: &AgentTarget, command: &str) -> Result<String> {
        let text = if matches!(target, AgentTarget::Codex) {
            let mut doc = toml::Table::new();
            upsert_codex(&mut doc, command)?;
            toml::to_string_pretty(&doc)?
        } else {
            serde_json::to_string_pretty(&Self::generate_config_for(target, command))
                .map_err(|err| AgentError::General(format!("JSON serialization error: {err}")))?
        };
        require_valid_02(target, &text)?;
        Ok(text)
    }

    /// Parse a client config into JSON. Codex input is TOML.
    pub fn parse_config_document(target: &AgentTarget, text: &str) -> Result<Value> {
        if matches!(target, AgentTarget::Codex) {
            let doc = parse_codex(text)?;
            return Ok(serde_json::to_value(doc)?);
        }
        serde_json::from_str(text)
            .map_err(|err| AgentError::Config(format!("invalid MCP config JSON: {err}")))
    }

    /// Connect and write or print the MCP configuration for a coding agent.
    pub fn connect(target_name: &str, write_to_file: bool) -> Result<String> {
        let home = StoragePaths::home_dir()?;
        Self::connect_with_config_home(&home, &xdg_config_home(&home), target_name, write_to_file)
    }

    /// Same as `connect`, but resolve config paths under `home` (tests use a temp home).
    pub fn connect_at(home: &Path, target_name: &str, write_to_file: bool) -> Result<String> {
        Self::connect_with_config_home(home, &home.join(".config"), target_name, write_to_file)
    }

    fn connect_with_config_home(
        home: &Path,
        config_home: &Path,
        target_name: &str,
        write_to_file: bool,
    ) -> Result<String> {
        let target = AgentTarget::parse(target_name);
        if matches!(target, AgentTarget::Generic) {
            return Err(AgentError::Config(format!(
                "unsupported connect target '{target_name}'"
            )));
        }
        // GUI clients may have no shell PATH. Persist the executable that
        // actually ran this command; printed snippets remain portable.
        let executable = if write_to_file {
            Some(std::env::current_exe()?)
        } else {
            None
        };
        let command = match executable.as_ref() {
            Some(path) => path.to_str().ok_or_else(|| {
                AgentError::Config("MCP executable path is not valid UTF-8".into())
            })?,
            None => "02",
        };
        let rendered = Self::render_config_for(&target, command)?;
        if write_to_file {
            if let Some(path) = target.config_path_in(home, config_home) {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)?;
                    }
                }
                let final_text = if path.exists() {
                    let existing_text = fs::read_to_string(&path)?;
                    Self::merge_config_text_for(&target, &existing_text, command)?
                } else {
                    rendered
                };
                require_valid_02(&target, &final_text)?;
                write_config_atomic(&path, final_text.as_bytes())?;
                let reread = fs::read_to_string(&path)?;
                require_valid_02(&target, &reread).map_err(|_| {
                    AgentError::Config(format!(
                        "written MCP config at {} does not contain a client-valid 02 entry",
                        path.display()
                    ))
                })?;
                return Ok(format!(
                    "Successfully configured '{}' MCP server in {}",
                    target_name,
                    path.display()
                ));
            }
        }
        Ok(rendered)
    }

    /// Merge the 02 server entry into an existing client config.
    pub fn merge_config_text(target: &AgentTarget, existing_text: &str) -> Result<String> {
        Self::merge_config_text_for(target, existing_text, "02")
    }

    fn merge_config_text_for(
        target: &AgentTarget,
        existing_text: &str,
        command: &str,
    ) -> Result<String> {
        let written = if matches!(target, AgentTarget::Codex) {
            let mut doc = if existing_text.trim().is_empty() {
                toml::Table::new()
            } else {
                parse_codex(existing_text)?
            };
            upsert_codex(&mut doc, command)?;
            toml::to_string_pretty(&doc)?
        } else {
            merge_json(target, existing_text, command)?
        };
        require_valid_02(target, &written)?;
        Ok(written)
    }
}

fn xdg_config_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
}

fn merge_json(target: &AgentTarget, existing_text: &str, command: &str) -> Result<String> {
    let mut existing: Value = if existing_text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(existing_text)
            .map_err(|err| AgentError::Config(format!("invalid MCP config JSON: {err}")))?
    };
    let root = existing
        .as_object_mut()
        .ok_or_else(|| AgentError::Config("MCP config root must be a JSON object".to_string()))?;
    match target {
        AgentTarget::Zed => upsert_json_entry(
            root,
            "context_servers",
            json!({
                "source": "custom",
                "command": command,
                "args": ["mcp"],
                "env": {}
            }),
        )?,
        AgentTarget::OpenCode => upsert_json_entry(
            root,
            "mcp",
            json!({
                "type": "local",
                "command": [command, "mcp"]
            }),
        )?,
        AgentTarget::Codex => {
            return Err(AgentError::Config(
                "Codex config is TOML, not JSON".to_string(),
            ));
        }
        _ => upsert_json_entry(
            root,
            "mcpServers",
            json!({ "command": command, "args": ["mcp"] }),
        )?,
    }
    serde_json::to_string_pretty(&existing)
        .map_err(|err| AgentError::Config(format!("MCP config serialization failed: {err}")))
}

fn upsert_json_entry(root: &mut Map<String, Value>, key: &str, fields: Value) -> Result<()> {
    let servers = root.entry(key).or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err(AgentError::Config(format!("{key} must be a JSON object")));
    };
    let entry = servers.entry("02".to_string()).or_insert_with(|| json!({}));
    let Some(entry) = entry.as_object_mut() else {
        return Err(AgentError::Config(format!(
            "{key}.02 must be a JSON object"
        )));
    };
    let Some(fields) = fields.as_object() else {
        return Err(AgentError::Config(
            "server entry must be an object".to_string(),
        ));
    };
    for (field, value) in fields {
        if field == "env" && entry.contains_key("env") {
            continue;
        }
        entry.insert(field.clone(), value.clone());
    }
    Ok(())
}

fn require_valid_02(target: &AgentTarget, text: &str) -> Result<()> {
    let ok = match target {
        AgentTarget::Codex => {
            let doc = parse_codex(text)?;
            codex_entry_valid(&doc)
        }
        _ => {
            let value: Value = serde_json::from_str(text).map_err(|err| {
                AgentError::Config(format!("written MCP config is not JSON: {err}"))
            })?;
            json_entry_valid(target, &value)
        }
    };
    if !ok {
        return Err(AgentError::Config(
            "written MCP config is missing a client-valid 02 entry".into(),
        ));
    }
    Ok(())
}

fn json_entry_valid(target: &AgentTarget, value: &Value) -> bool {
    match target {
        AgentTarget::Zed => {
            let entry = &value["context_servers"]["02"];
            entry.get("source").and_then(|v| v.as_str()) == Some("custom") && stdio_command(entry)
        }
        AgentTarget::OpenCode => {
            let entry = &value["mcp"]["02"];
            entry.get("type").and_then(|v| v.as_str()) == Some("local")
                && entry
                    .get("command")
                    .and_then(Value::as_array)
                    .is_some_and(|args| {
                        args.len() == 2
                            && valid_command(args[0].as_str())
                            && args[1].as_str() == Some("mcp")
                    })
        }
        AgentTarget::Codex => false,
        _ => stdio_command(&value["mcpServers"]["02"]),
    }
}

fn stdio_command(entry: &Value) -> bool {
    valid_command(entry.get("command").and_then(Value::as_str)) && command_args(entry, &["mcp"])
}

fn valid_command(command: Option<&str>) -> bool {
    command.is_some_and(|command| command == "02" || Path::new(command).is_absolute())
}

fn command_args(entry: &Value, expected: &[&str]) -> bool {
    entry
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.len() == expected.len()
                && arr
                    .iter()
                    .zip(expected)
                    .all(|(value, want)| value.as_str() == Some(*want))
        })
        .unwrap_or(false)
}

fn parse_codex(text: &str) -> Result<toml::Table> {
    toml::from_str(text.strip_prefix('\u{feff}').unwrap_or(text))
        .map_err(|err| AgentError::Config(format!("invalid Codex config.toml: {err}")))
}

fn upsert_codex(doc: &mut toml::Table, command: &str) -> Result<()> {
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| AgentError::Config("mcp_servers must be a TOML table".into()))?;
    let entry = servers
        .entry("02")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| AgentError::Config("mcp_servers.02 must be a TOML table".into()))?;
    entry.insert("command".into(), toml::Value::String(command.into()));
    entry.insert(
        "args".into(),
        toml::Value::Array(vec![toml::Value::String("mcp".into())]),
    );
    Ok(())
}

fn codex_entry_valid(doc: &toml::Table) -> bool {
    let Some(entry) = doc
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .and_then(|servers| servers.get("02"))
        .and_then(toml::Value::as_table)
    else {
        return false;
    };
    valid_command(entry.get("command").and_then(toml::Value::as_str))
        && entry
            .get("args")
            .and_then(toml::Value::as_array)
            .map(|args| args.len() == 1 && args[0].as_str() == Some("mcp"))
            .unwrap_or(false)
}

/// Publish in one rename; failures before rename leave the old file intact.
fn write_config_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mode = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => meta.permissions().mode() & 0o777,
        Ok(_) => {
            return Err(AgentError::Config(format!(
                "refusing to replace non-regular config {}",
                path.display()
            )))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0o600,
        Err(err) => return Err(err.into()),
    };
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let seq = CONFIG_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".02-mcp-config.{}.{}.tmp", std::process::id(), seq));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
