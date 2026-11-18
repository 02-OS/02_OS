mod dbus_atspi;

use agent_core::{
    contain::write_private_atomic, error::AgentError, paths::StoragePaths, Result, SecretPattern,
};
use chrono::Utc;
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Local files that arm or stop desktop automation. Both stay mode `0600`.
pub fn consent_path() -> Result<PathBuf> {
    Ok(StoragePaths::config_dir()?.join("desktop-consent"))
}

pub fn kill_path() -> Result<PathBuf> {
    Ok(StoragePaths::config_dir()?.join("desktop-kill"))
}

fn write_private(path: &Path, body: &str) -> Result<()> {
    write_private_atomic(path, body.as_bytes())
}

/// Explicit consent. The file must contain the word `allow`.
pub fn write_consent(path: &Path) -> Result<()> {
    write_private(path, "allow\n")
}

/// Kill switch. Its presence refuses every desktop tool call.
pub fn write_kill(path: &Path) -> Result<()> {
    write_private(path, "stop\n")
}

/// Refuse the call unless consent is an `0600` regular file (`O_NOFOLLOW | O_NONBLOCK`)
/// saying `allow`, and the kill file is absent.
pub fn authorize(consent: &Path, kill: &Path) -> Result<()> {
    if kill.symlink_metadata().is_ok() {
        return Err(AgentError::Security("desktop automation is stopped".into()));
    }
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(consent)
    {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(AgentError::Security(
                "desktop automation requires consent".into(),
            ));
        }
        Err(err) if err.raw_os_error() == Some(libc::ELOOP) => {
            return Err(AgentError::Security(
                "desktop consent file must not be a symlink".into(),
            ));
        }
        Err(err) => return Err(err.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(AgentError::Security(
            "desktop consent file must be a regular file".into(),
        ));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(AgentError::Security(
            "desktop consent file must be mode 0600".into(),
        ));
    }
    let mut text = String::new();
    file.take(1024).read_to_string(&mut text)?;
    let allowed = text.split_whitespace().any(|word| word == "allow");
    if !allowed {
        return Err(AgentError::Security(
            "desktop automation requires consent".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopWindow {
    pub id: String,
    pub title: String,
    pub focused: bool,
}

/// Accessibility tree used by desktop tools.
pub trait AccessibilityTree {
    fn list_windows(&self) -> Result<Vec<DesktopWindow>>;
    fn focused_window(&self) -> Result<Option<DesktopWindow>>;
    fn click(&mut self, id: &str) -> Result<()>;
    fn type_text(&mut self, id: &str, text: &str) -> Result<()>;
    fn key_press(&mut self, id: &str, key: &str) -> Result<()>;
    fn scroll(&mut self, id: &str, direction: &str, amount: i64) -> Result<()>;
    fn inspect_element(&self, id: &str) -> Result<Value>;
}

/// In-memory tree for tests and for any caller that supplies its own windows.
#[derive(Debug, Default)]
pub struct FakeTree {
    pub windows: Vec<DesktopWindow>,
    pub clicks: Vec<String>,
    pub typed: Vec<(String, String)>,
}

impl AccessibilityTree for FakeTree {
    fn list_windows(&self) -> Result<Vec<DesktopWindow>> {
        Ok(self.windows.clone())
    }

    fn focused_window(&self) -> Result<Option<DesktopWindow>> {
        Ok(self.windows.iter().find(|window| window.focused).cloned())
    }

    fn click(&mut self, id: &str) -> Result<()> {
        if !self.windows.iter().any(|window| window.id == id) {
            return Err(AgentError::General(format!("unknown window {id}")));
        }
        self.clicks.push(id.to_string());
        Ok(())
    }

    fn type_text(&mut self, id: &str, text: &str) -> Result<()> {
        if !self.windows.iter().any(|window| window.id == id) {
            return Err(AgentError::General(format!("unknown window {id}")));
        }
        self.typed.push((id.to_string(), text.to_string()));
        Ok(())
    }

    fn key_press(&mut self, id: &str, key: &str) -> Result<()> {
        if !self.windows.iter().any(|window| window.id == id) {
            return Err(AgentError::General(format!("unknown window {id}")));
        }
        self.clicks.push(format!("{id}:key:{key}"));
        Ok(())
    }

    fn scroll(&mut self, id: &str, direction: &str, amount: i64) -> Result<()> {
        if !self.windows.iter().any(|window| window.id == id) {
            return Err(AgentError::General(format!("unknown window {id}")));
        }
        self.clicks
            .push(format!("{id}:scroll:{direction}:{amount}"));
        Ok(())
    }

    fn inspect_element(&self, id: &str) -> Result<Value> {
        let win = self
            .windows
            .iter()
            .find(|window| window.id == id)
            .ok_or_else(|| AgentError::General(format!("unknown window {id}")))?;
        Ok(json!({
            "id": win.id,
            "title": SecretPattern::redact(&win.title),
            "focused": win.focused
        }))
    }
}

/// Probe a local session bus socket (`;`-delimited multi-transport aware).
pub fn probe_session_bus(address: Option<&str>) -> Result<()> {
    dbus_atspi::probe_bus_or_unavailable(address)
}

pub struct SessionTree;

impl AccessibilityTree for SessionTree {
    fn list_windows(&self) -> Result<Vec<DesktopWindow>> {
        let entries = dbus_atspi::session_list_windows(
            std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref(),
        )?;
        Ok(entries
            .into_iter()
            .map(|entry| DesktopWindow {
                id: entry.id,
                title: entry.title,
                focused: entry.focused,
            })
            .collect())
    }

    fn focused_window(&self) -> Result<Option<DesktopWindow>> {
        let windows = self.list_windows()?;
        Ok(windows.into_iter().find(|window| window.focused))
    }

    fn click(&mut self, _id: &str) -> Result<()> {
        probe_session_bus(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref())
    }

    fn type_text(&mut self, _id: &str, _text: &str) -> Result<()> {
        probe_session_bus(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref())
    }

    fn key_press(&mut self, _id: &str, _key: &str) -> Result<()> {
        probe_session_bus(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref())
    }

    fn scroll(&mut self, _id: &str, _direction: &str, _amount: i64) -> Result<()> {
        probe_session_bus(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref())
    }

    fn inspect_element(&self, id: &str) -> Result<Value> {
        dbus_atspi::session_inspect_element(
            std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref(),
            id,
        )
    }
}

pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "desktop_list_windows",
            "description": "List windows from the local accessibility tree. Requires consent and stops when the kill file exists.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "desktop_focused_window",
            "description": "Return the focused local window, if the accessibility tree reports one.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "desktop_click",
            "description": "Activate a window by the id the accessibility tree reported.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
                "additionalProperties": false
            }
        },
        {
            "name": "desktop_type",
            "description": "Type text into a window id from the accessibility tree. The text is not echoed back or logged.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "text": { "type": "string" }
                },
                "required": ["id", "text"],
                "additionalProperties": false
            }
        },
        {
            "name": "desktop_key_press",
            "description": "Send a single validated key symbol to a window id from the accessibility tree.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "key": { "type": "string" }
                },
                "required": ["id", "key"],
                "additionalProperties": false
            }
        },
        {
            "name": "desktop_scroll",
            "description": "Scroll within a window id by a bounded direction and step count.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "direction": { "type": "string", "enum": ["up", "down", "left", "right"] },
                    "amount": { "type": "integer", "minimum": 1, "maximum": 100 }
                },
                "required": ["id", "direction"],
                "additionalProperties": false
            }
        },
        {
            "name": "desktop_element_inspect",
            "description": "Inspect redacted metadata for an accessibility element id.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
                "additionalProperties": false
            }
        }
    ])
}

fn append_audit_log(consent: &Path, tool: &str, target_id: Option<&str>, ok: bool) {
    let Some(dir) = consent.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return;
    };
    let log_path = dir.join("desktop-audit.jsonl");
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&log_path)
    else {
        return;
    };
    if let Ok(meta) = file.metadata() {
        if !meta.file_type().is_file() {
            return;
        }
    }
    let entry = json!({
        "ts": Utc::now().to_rfc3339(),
        "tool": tool,
        "id": target_id,
        "ok": ok
    });
    let _ = writeln!(file, "{entry}");
}

/// One desktop tool call. Consent and the kill switch are checked before the tree is touched.
pub fn call_tool<T: AccessibilityTree>(
    consent: &Path,
    kill: &Path,
    tree: &mut T,
    name: &str,
    arguments: &Value,
) -> Result<Value> {
    authorize(consent, kill)?;
    let target_id = arguments.get("id").and_then(Value::as_str);
    let outcome = match name {
        "desktop_list_windows" => {
            let windows = tree.list_windows()?;
            Ok(json!({ "windows": windows.iter().map(window_json).collect::<Vec<_>>() }))
        }
        "desktop_focused_window" => match tree.focused_window()? {
            Some(window) => Ok(window_json(&window)),
            None => Ok(json!({ "window": null })),
        },
        "desktop_click" => {
            let id = required_str(arguments, "id")?;
            tree.click(id)?;
            Ok(json!({ "clicked": id }))
        }
        "desktop_type" => {
            let id = required_str(arguments, "id")?;
            let text = required_str(arguments, "text")?;
            tree.type_text(id, text)?;
            Ok(json!({ "typed": true }))
        }
        "desktop_key_press" => {
            let id = required_str(arguments, "id")?;
            let key = required_str(arguments, "key")?;
            if key.len() > 64
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
            {
                return Err(AgentError::General("invalid key symbol".into()));
            }
            tree.key_press(id, key)?;
            Ok(json!({ "key_pressed": key }))
        }
        "desktop_scroll" => {
            let id = required_str(arguments, "id")?;
            let direction = required_str(arguments, "direction")?;
            if !matches!(direction, "up" | "down" | "left" | "right") {
                return Err(AgentError::General(
                    "scroll direction must be up, down, left, or right".into(),
                ));
            }
            let amount = arguments.get("amount").and_then(Value::as_i64).unwrap_or(1);
            if !(1..=100).contains(&amount) {
                return Err(AgentError::General(
                    "scroll amount must be in 1..=100".into(),
                ));
            }
            tree.scroll(id, direction, amount)?;
            Ok(json!({ "scrolled": direction, "amount": amount }))
        }
        "desktop_element_inspect" => {
            let id = required_str(arguments, "id")?;
            tree.inspect_element(id)
        }
        _ => Err(AgentError::General(format!("unknown desktop tool {name}"))),
    };
    append_audit_log(consent, name, target_id, outcome.is_ok());
    outcome
}

fn window_json(window: &DesktopWindow) -> Value {
    json!({
        "id": window.id,
        "title": SecretPattern::redact(&window.title),
        "focused": window.focused
    })
}

fn required_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AgentError::General(format!("desktop tool requires {key}")))
}

/// JSON-RPC 2.0 `initialize`, `tools/list`, and `tools/call`. Validates envelope strictly.
pub fn handle_message<T: AccessibilityTree>(
    consent: &Path,
    kill: &Path,
    tree: &mut T,
    message: &Value,
) -> Option<Value> {
    let Some(obj) = message.as_object() else {
        return Some(json!({
            "jsonrpc": "2.0",
            "id": Value::Null,
            "error": { "code": -32600, "message": "Invalid Request" }
        }));
    };
    let has_id = obj.contains_key("id");
    let id = obj.get("id").cloned().unwrap_or(Value::Null);
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32600, "message": "Invalid Request: missing or bad jsonrpc version" }
        }));
    }
    let Some(method) = obj
        .get("method")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
    else {
        return Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32600, "message": "Invalid Request: missing method" }
        }));
    };
    if !has_id {
        return None;
    }
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "02-desktop", "version": "0.1.0" }
        })),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let params = obj.get("params").cloned().unwrap_or(json!({}));
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            match call_tool(consent, kill, tree, name, &arguments) {
                Ok(value) => Ok(json!({
                    "content": [{ "type": "text", "text": value.to_string() }],
                    "isError": false
                })),
                Err(err) => Ok(json!({
                    "content": [{ "type": "text", "text": err.to_string() }],
                    "isError": true
                })),
            }
        }
        _ => Err(AgentError::General(format!("unknown method {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(err) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": err.to_string() }
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    fn paths(dir: &Path) -> (PathBuf, PathBuf) {
        (dir.join("desktop-consent"), dir.join("desktop-kill"))
    }

    fn allow(dir: &Path) -> (PathBuf, PathBuf) {
        let (consent, kill) = paths(dir);
        write_consent(&consent).unwrap();
        (consent, kill)
    }

    #[test]
    fn calls_are_denied_without_consent_or_when_the_kill_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let (consent, kill) = paths(dir.path());
        let mut tree = FakeTree::default();
        let missing = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_list_windows",
            &json!({}),
        );
        assert!(missing
            .unwrap_err()
            .to_string()
            .contains("requires consent"));

        write_consent(&consent).unwrap();
        let mode = fs::metadata(&consent).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::write(&consent, "later\n").unwrap();
        fs::set_permissions(&consent, fs::Permissions::from_mode(0o600)).unwrap();
        let word = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_list_windows",
            &json!({}),
        );
        assert!(word.unwrap_err().to_string().contains("requires consent"));

        write_consent(&consent).unwrap();
        fs::set_permissions(&consent, fs::Permissions::from_mode(0o644)).unwrap();
        let loose = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_list_windows",
            &json!({}),
        );
        assert!(loose.unwrap_err().to_string().contains("0600"));

        write_consent(&consent).unwrap();
        write_kill(&kill).unwrap();
        let stopped = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_list_windows",
            &json!({}),
        );
        assert!(stopped.unwrap_err().to_string().contains("stopped"));
        assert!(tree.clicks.is_empty());
    }

    #[test]
    fn a_consented_fake_tree_lists_clicks_and_does_not_echo_typed_text() {
        let dir = tempfile::tempdir().unwrap();
        let (consent, kill) = allow(dir.path());
        let mut tree = FakeTree {
            windows: vec![DesktopWindow {
                id: "win-1".into(),
                title: "Editor".into(),
                focused: true,
            }],
            clicks: Vec::new(),
            typed: Vec::new(),
        };
        let listed = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_list_windows",
            &json!({}),
        )
        .unwrap();
        assert_eq!(listed["windows"][0]["title"], "Editor");
        call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_click",
            &json!({"id": "win-1"}),
        )
        .unwrap();
        let typed = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_type",
            &json!({"id": "win-1", "text": "secret-token"}),
        )
        .unwrap();
        assert_eq!(typed, json!({ "typed": true }));
        assert_eq!(tree.clicks, vec!["win-1".to_string()]);
        assert_eq!(tree.typed[0].1, "secret-token");
        assert!(!typed.to_string().contains("secret-token"));
    }

    #[test]
    fn session_probe_errors_without_a_bus_and_refuses_a_reachable_bus() {
        let missing = probe_session_bus(None).unwrap_err();
        assert!(missing.to_string().contains("no session bus"));

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("bus");
        let _listener = UnixListener::bind(&socket).unwrap();
        let address = format!("unix:path={}", socket.display());
        let refused = probe_session_bus(Some(&address)).unwrap_err();
        assert!(refused.to_string().contains("AT-SPI tree is not available"));
    }

    #[test]
    fn tool_call_without_consent_is_an_error_result_and_list_is_local() {
        let dir = tempfile::tempdir().unwrap();
        let (consent, kill) = paths(dir.path());
        let mut tree = FakeTree::default();
        let listed = handle_message(
            &consent,
            &kill,
            &mut tree,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        )
        .unwrap();
        assert!(listed["result"]["tools"][0]["name"]
            .as_str()
            .unwrap()
            .starts_with("desktop_"));
        let called = handle_message(
            &consent,
            &kill,
            &mut tree,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "desktop_click", "arguments": { "id": "win-1" } }
            }),
        )
        .unwrap();
        assert_eq!(called["result"]["isError"], true);
        assert!(called["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("requires consent"));

        let bad_rpc = handle_message(
            &consent,
            &kill,
            &mut tree,
            &json!({"id": 3, "method": "tools/list"}),
        )
        .unwrap();
        assert_eq!(bad_rpc["error"]["code"], -32600);
    }

    #[test]
    fn desktop_multi_transport_new_tools_audit_log_and_directory_reject() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("bus");
        let _listener = UnixListener::bind(&socket).unwrap();
        let multi = format!(
            "unix:abstract=/tmp/fake-abs;unix:path={},guid=1",
            socket.display()
        );
        let err = probe_session_bus(Some(&multi)).unwrap_err();
        assert!(err.to_string().contains("AT-SPI tree is not available"));

        let fake_dir_consent = dir.path().join("consent-dir");
        fs::create_dir(&fake_dir_consent).unwrap();
        fs::set_permissions(&fake_dir_consent, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(authorize(&fake_dir_consent, &dir.path().join("kill")).is_err());

        let (consent, kill) = allow(dir.path());
        let mut tree = FakeTree {
            windows: vec![DesktopWindow {
                id: "w1".into(),
                title: "Term".into(),
                focused: true,
            }],
            clicks: Vec::new(),
            typed: Vec::new(),
        };
        call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_type",
            &json!({"id": "w1", "text": "top-secret-password"}),
        )
        .unwrap();
        let key_res = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_key_press",
            &json!({"id": "w1", "key": "Return"}),
        )
        .unwrap();
        assert_eq!(key_res["key_pressed"], "Return");
        let scroll_res = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_scroll",
            &json!({"id": "w1", "direction": "down", "amount": 3}),
        )
        .unwrap();
        assert_eq!(scroll_res["scrolled"], "down");
        let insp = call_tool(
            &consent,
            &kill,
            &mut tree,
            "desktop_element_inspect",
            &json!({"id": "w1"}),
        )
        .unwrap();
        assert_eq!(insp["id"], "w1");
        let audit = fs::read_to_string(dir.path().join("desktop-audit.jsonl")).unwrap();
        assert!(audit.contains("desktop_key_press"));
        assert!(!audit.contains("top-secret-password"));
    }
}
