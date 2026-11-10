use agent_core::{error::AgentError, paths::StoragePaths, Result};
use serde_json::{json, Value};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// Local files that arm or stop desktop automation. Both stay mode `0600`.
pub fn consent_path() -> Result<PathBuf> {
    Ok(StoragePaths::config_dir()?.join("desktop-consent"))
}

pub fn kill_path() -> Result<PathBuf> {
    Ok(StoragePaths::config_dir()?.join("desktop-kill"))
}

fn write_private(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            StoragePaths::ensure_private_dir(parent)?;
        }
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, body)?;
    let mut permissions = fs::metadata(&temporary)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&temporary, permissions)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

/// Explicit consent. The file must contain the word `allow`.
pub fn write_consent(path: &Path) -> Result<()> {
    write_private(path, "allow\n")
}

/// Kill switch. Its presence refuses every desktop tool call.
pub fn write_kill(path: &Path) -> Result<()> {
    write_private(path, "stop\n")
}

/// Refuse the call unless consent is private and says `allow`, and the kill file is absent.
pub fn authorize(consent: &Path, kill: &Path) -> Result<()> {
    if kill.symlink_metadata().is_ok() {
        return Err(AgentError::Security("desktop automation is stopped".into()));
    }
    let metadata = match consent.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(AgentError::Security(
                "desktop automation requires consent".into(),
            ));
        }
        Err(err) => return Err(err.into()),
    };
    if metadata.file_type().is_symlink() {
        return Err(AgentError::Security(
            "desktop consent file must not be a symlink".into(),
        ));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(AgentError::Security(
            "desktop consent file must be mode 0600".into(),
        ));
    }
    let text = fs::read_to_string(consent)?;
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

/// Accessibility tree used by desktop tools. The live process does not implement this.
pub trait AccessibilityTree {
    fn list_windows(&self) -> Result<Vec<DesktopWindow>>;
    fn focused_window(&self) -> Result<Option<DesktopWindow>>;
    fn click(&mut self, id: &str) -> Result<()>;
    fn type_text(&mut self, id: &str, text: &str) -> Result<()>;
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
}

/// Fail closed. A session bus is probed and then refused: this binary links no AT-SPI toolkit,
/// sends no input events, and opens no network connection.
pub fn probe_session_bus(address: Option<&str>) -> Result<()> {
    let Some(address) = address.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(AgentError::General(
            "desktop automation found no session bus".into(),
        ));
    };
    let path = address.split(',').find_map(|part| {
        let part = part.trim();
        part.strip_prefix("unix:path=")
            .or_else(|| part.strip_prefix("unix:abstract="))
    });
    let Some(path) = path else {
        return Err(AgentError::General(
            "session bus address is not a local socket".into(),
        ));
    };
    if path.contains("..") || path.contains('\0') {
        return Err(AgentError::Security("session bus path is not local".into()));
    }
    if address.contains("unix:abstract=") {
        return Err(AgentError::General(
            "desktop automation does not use an abstract session bus".into(),
        ));
    }
    UnixStream::connect(path)
        .map_err(|err| AgentError::General(format!("session bus is not reachable: {err}")))?;
    Err(AgentError::General(
        "AT-SPI tree is not available to this process".into(),
    ))
}

pub struct SessionTree;

impl AccessibilityTree for SessionTree {
    fn list_windows(&self) -> Result<Vec<DesktopWindow>> {
        probe_session_bus(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref())?;
        Ok(Vec::new())
    }

    fn focused_window(&self) -> Result<Option<DesktopWindow>> {
        probe_session_bus(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref())?;
        Ok(None)
    }

    fn click(&mut self, _id: &str) -> Result<()> {
        probe_session_bus(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref())?;
        Ok(())
    }

    fn type_text(&mut self, _id: &str, _text: &str) -> Result<()> {
        probe_session_bus(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref())?;
        Ok(())
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
            "description": "Type text into a window id from the accessibility tree. The text is not echoed back.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "text": { "type": "string" }
                },
                "required": ["id", "text"],
                "additionalProperties": false
            }
        }
    ])
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
    match name {
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
        _ => Err(AgentError::General(format!("unknown desktop tool {name}"))),
    }
}

fn window_json(window: &DesktopWindow) -> Value {
    json!({
        "id": window.id,
        "title": window.title,
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

/// JSON-RPC tools/list and tools/call. Notifications and unknown methods get no side effects.
pub fn handle_message<T: AccessibilityTree>(
    consent: &Path,
    kill: &Path,
    tree: &mut T,
    message: &Value,
) -> Option<Value> {
    message.get("id")?;
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "02-desktop", "version": "0.1.0" }
        })),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let params = message.get("params").cloned().unwrap_or(json!({}));
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
    }
}
