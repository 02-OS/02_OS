use agent_core::Result;
use agent_daemon::{DaemonSocket, RepoWatcher};
use agent_git::GitDiscovery;
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

pub fn register_automatic(root: &Path) -> Result<bool> {
    let watcher = RepoWatcher::new()?;
    let root = root.canonicalize()?;
    watcher.register_automatic(root.clone())?;
    Ok(watcher.load_state()?.repositories.contains(&root))
}

pub fn watching(root: &Path) -> Result<bool> {
    Ok(RepoWatcher::new()?
        .load_state()?
        .repositories
        .contains(&root.canonicalize()?))
}

/// Probe the actual protocol, not just a stale socket pathname.
pub fn daemon_available() -> bool {
    let probe = || -> Result<bool> {
        let mut stream = UnixStream::connect(DaemonSocket::socket_path()?)?;
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;
        stream.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n")?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        let value: serde_json::Value = serde_json::from_str(&line)?;
        Ok(value["id"] == 1 && value.get("result").is_some())
    };
    probe().unwrap_or(false)
}

pub fn execute(enable: bool, json_output: bool) -> Result<()> {
    let root = GitDiscovery::find_repository_root(std::env::current_dir()?)?.canonicalize()?;
    // Explicit watch also initializes storage; init honors a previous opt-out.
    if enable {
        crate::commands::init::execute_quiet()?;
    }
    let watcher = RepoWatcher::new()?;
    if enable {
        watcher.add(root.clone())?;
    } else {
        watcher.remove(&root)?;
    }
    let available = daemon_available();
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "success", "root_path": root, "watching": enable,
                "daemon_available": available
            }))?
        );
    } else {
        println!(
            "Background indexing {} for {} (daemon {})",
            if enable { "enabled" } else { "disabled" },
            root.display(),
            if available {
                "available"
            } else {
                "stopped; registration is saved"
            }
        );
    }
    Ok(())
}
