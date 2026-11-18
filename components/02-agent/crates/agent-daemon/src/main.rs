use agent_core::Result;
use agent_daemon::{
    decide_rpc, publish_daemon_status, resolve_mcp_repo_path, status_rpc, DaemonSocket, RepoWatcher,
};
use agent_mcp::McpServer;
use agent_skills::{AgentId, SessionPolicy, SharedContextScope, Skill};
use clap::Parser;
use serde_json::json;
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MAX_CONCURRENT_CLIENTS: usize = 32;
const MAX_RPC_LINE_BYTES: usize = 1024 * 1024;
const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(30);

struct ClientSlotGuard(Arc<AtomicUsize>);

impl Drop for ClientSlotGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn parse_session_policy(val: &serde_json::Value) -> Option<SessionPolicy> {
    let agent_str = val
        .get("agent_id")
        .and_then(|v| v.as_str())
        .unwrap_or("mcp-client");
    let agent_id = AgentId::parse(agent_str).ok()?;
    let skill_val = val.get("skill").unwrap_or(val);
    let name = skill_val
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let version = skill_val
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.1.0")
        .to_string();
    let tools = skill_val
        .get("tools")
        .and_then(|v| v.as_array())?
        .iter()
        .filter_map(|t| t.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let scope = match val
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("private")
    {
        "private" => SharedContextScope::Private,
        s if s.starts_with("repo:") => SharedContextScope::repository(&s[5..]).ok()?,
        s if s.starts_with("session:") => SharedContextScope::session(&s[8..]).ok()?,
        _ => SharedContextScope::Private,
    };
    Some(SessionPolicy::new(
        agent_id,
        Skill {
            name,
            version,
            tools,
        },
        scope,
    ))
}

fn normalize_watch_path(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if let Ok(repo) = agent_git::GitRepo::open(&path) {
        let root = repo.root_path();
        return std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    }
    std::fs::canonicalize(&path).unwrap_or(path)
}

#[derive(Parser, Debug)]
#[command(
    name = "02-agentd",
    author = "02_OS Systems Engineering Team",
    version,
    about = "02 Agent Runtime background user daemon service"
)]
struct DaemonArgs {
    #[arg(long, help = "Custom Unix domain socket path")]
    socket: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = 15,
        help = "Background repository sync interval in seconds"
    )]
    interval: u64,
}

static RUNNING: AtomicBool = AtomicBool::new(true);

extern "C" fn sig_handler(_: libc::c_int) {
    RUNNING.store(false, Ordering::SeqCst);
}

fn main() -> Result<()> {
    let args = DaemonArgs::parse();

    // Security & isolation check: user service, NOT root
    if unsafe { libc::getuid() } == 0 {
        eprintln!("[!] WARNING: 02-agentd is designed to run as a user service, not root.");
        eprintln!(
            "    Indices and sockets should reside in user session scope ($XDG_RUNTIME_DIR)."
        );
    }

    let socket_path = match args.socket {
        Some(s) => s,
        None => DaemonSocket::socket_path()?,
    };

    let listener = DaemonSocket::bind(&socket_path)?;
    println!(
        "02-agentd: 02 Agent Runtime Daemon v{}",
        env!("CARGO_PKG_VERSION")
    );
    println!("Listening on Unix domain socket: {}", socket_path.display());

    // Register POSIX signal handlers
    unsafe {
        libc::signal(libc::SIGINT, sig_handler as *const () as usize);
        libc::signal(libc::SIGTERM, sig_handler as *const () as usize);
    }

    // Spawn background repo watcher thread
    let watcher = Arc::new(RepoWatcher::new()?);
    let watcher_bg = watcher.clone();
    let interval_dur = Duration::from_secs(args.interval);

    thread::spawn(move || {
        while RUNNING.load(Ordering::Relaxed) {
            let errors = watcher_bg.sync_all();
            if errors > 0 {
                eprintln!("02-agentd: {errors} repository sync(s) failed");
            }
            if let Ok(watched) = watcher_bg.list() {
                if let Err(err) = publish_daemon_status(&watched) {
                    eprintln!("02-agentd: status snapshot: {err}");
                }
            }
            thread::sleep(interval_dur);
        }
    });

    // Accept incoming socket connections
    listener.set_nonblocking(true)?;
    let raw_fd = listener.as_raw_fd();

    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let active_clients = Arc::new(AtomicUsize::new(0));

    while RUNNING.load(Ordering::Relaxed) {
        let mut pfd = libc::pollfd {
            fd: raw_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // Event-driven kernel poll with 1000ms timeout; wakes immediately on socket activity or signal
        let ret = unsafe { libc::poll(&mut pfd, 1, 1000) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if ret == 0 {
            continue;
        }

        if (pfd.revents & libc::POLLIN) != 0 {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if !DaemonSocket::accepts_peer(&stream) {
                        drop(stream);
                        continue;
                    }
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(SOCKET_IO_TIMEOUT));
                    let _ = stream.set_write_timeout(Some(SOCKET_IO_TIMEOUT));

                    if active_clients.load(Ordering::SeqCst) >= MAX_CONCURRENT_CLIENTS {
                        let busy = json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": { "code": -32000, "message": "Daemon busy: maximum concurrent clients reached" }
                        });
                        let _ = writeln!(stream, "{busy}");
                        drop(stream);
                        continue;
                    }
                    active_clients.fetch_add(1, Ordering::SeqCst);
                    let slot_guard = ClientSlotGuard(Arc::clone(&active_clients));
                    let peer_is_mcp = DaemonSocket::peer_is_mcp_server(&stream);
                    let watcher_conn = watcher.clone();
                    let default_dir = current_dir.clone();

                    thread::spawn(move || {
                        let _slot = slot_guard;
                        let mut reader = BufReader::new(match stream.try_clone() {
                            Ok(s) => s,
                            Err(_) => return,
                        });
                        let mut writer = stream;
                        let mut line = String::new();
                        let mut session_repo = None;
                        let mut session_policy: Option<SessionPolicy> = None;
                        let mut is_mcp_session = peer_is_mcp;

                        loop {
                            line.clear();
                            let read_res = (&mut reader)
                                .take((MAX_RPC_LINE_BYTES + 1) as u64)
                                .read_line(&mut line);
                            let n = match read_res {
                                Ok(0) => break,
                                Ok(n) => n,
                                Err(_) => break,
                            };
                            if n > MAX_RPC_LINE_BYTES && !line.ends_with('\n') {
                                let err = json!({
                                    "jsonrpc": "2.0",
                                    "id": null,
                                    "error": { "code": -32600, "message": "Request exceeds maximum line size" }
                                });
                                let _ = writeln!(writer, "{err}");
                                break;
                            }

                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }

                            let req_val: serde_json::Value = match serde_json::from_str(trimmed) {
                                Ok(v) => v,
                                Err(e) => {
                                    let err = json!({
                                        "jsonrpc": "2.0",
                                        "id": null,
                                        "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                                    });
                                    let _ = writeln!(writer, "{}", err);
                                    continue;
                                }
                            };

                            if req_val.is_array() {
                                is_mcp_session = true;
                                let watched = watcher_conn.list().unwrap_or_default();
                                let repo_path = session_repo
                                    .clone()
                                    .or_else(|| (watched.len() == 1).then(|| watched[0].clone()))
                                    .unwrap_or_else(|| default_dir.clone());
                                let mut allowed_roots = watched;
                                if let Some(ref sr) = session_repo {
                                    if !allowed_roots.contains(sr) {
                                        allowed_roots.push(sr.clone());
                                    }
                                }
                                if !allowed_roots.contains(&default_dir) {
                                    allowed_roots.push(default_dir.clone());
                                }
                                let mut server =
                                    McpServer::with_allowed_roots(repo_path, allowed_roots);
                                if let Some(ref policy) = session_policy {
                                    server = server.with_policy(policy.clone());
                                }
                                if let Some(resp) = server.handle_json(req_val) {
                                    let _ = writeln!(writer, "{resp}");
                                    let _ = writer.flush();
                                }
                                continue;
                            }

                            if !req_val.is_object() {
                                let err = json!({
                                    "jsonrpc": "2.0",
                                    "id": null,
                                    "error": { "code": -32600, "message": "Invalid Request" }
                                });
                                let _ = writeln!(writer, "{err}");
                                continue;
                            }

                            let has_id = req_val
                                .as_object()
                                .map(|obj| obj.contains_key("id"))
                                .unwrap_or(false);
                            let method = req_val["method"].as_str().unwrap_or("").to_string();
                            let jsonrpc_ok =
                                req_val.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0");
                            let params_ok = match req_val.get("params") {
                                None => true,
                                Some(params) if params.is_array() || params.is_object() => true,
                                Some(_) => false,
                            };

                            // No id member: notification. Never write a response,
                            // including unknown methods and bad versions.
                            if !has_id {
                                if jsonrpc_ok && params_ok && !method.is_empty() {
                                    match method.as_str() {
                                        "daemon/watch" => {
                                            let path_str =
                                                req_val["params"]["path"].as_str().unwrap_or("");
                                            if let Err(err) =
                                                watcher_conn.add(normalize_watch_path(path_str))
                                            {
                                                eprintln!(
                                                    "02-agentd: daemon/watch {path_str}: {err}"
                                                );
                                            }
                                        }
                                        "daemon/unwatch" => {
                                            let path_str =
                                                req_val["params"]["path"].as_str().unwrap_or("");
                                            if let Err(err) =
                                                watcher_conn.remove(&normalize_watch_path(path_str))
                                            {
                                                eprintln!(
                                                    "02-agentd: daemon/unwatch {path_str}: {err}"
                                                );
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                continue;
                            }

                            let id_value = req_val.get("id").cloned().unwrap_or(json!(null));
                            let id_ok =
                                id_value.is_string() || id_value.is_number() || id_value.is_null();
                            let id = if id_ok { id_value } else { json!(null) };

                            if !jsonrpc_ok || !id_ok || !params_ok || method.is_empty() {
                                let err = json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "error": { "code": -32600, "message": "Invalid Request" }
                                });
                                let _ = writeln!(writer, "{err}");
                                continue;
                            }

                            // Daemon management extension methods
                            let response = match method.as_str() {
                                "daemon/watch" => {
                                    let path_str = req_val["params"]["path"].as_str().unwrap_or("");
                                    let path = normalize_watch_path(path_str);
                                    Some(watch_result_response(id, "added", watcher_conn.add(path)))
                                }
                                "daemon/unwatch" => {
                                    let path_str = req_val["params"]["path"].as_str().unwrap_or("");
                                    let path = normalize_watch_path(path_str);
                                    Some(watch_result_response(
                                        id,
                                        "removed",
                                        watcher_conn.remove(&path),
                                    ))
                                }
                                "daemon/list_watched" => match watcher_conn.list() {
                                    Ok(list) => Some(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": { "repositories": list }
                                    })),
                                    Err(err) => Some(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "error": { "code": -32000, "message": err.to_string() }
                                    })),
                                },
                                "daemon/status" => match watcher_conn.list() {
                                    Ok(list) => Some(status_rpc(id, &list)),
                                    Err(err) => Some(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "error": { "code": -32000, "message": err.to_string() }
                                    })),
                                },
                                "daemon/decide" => {
                                    if is_mcp_session {
                                        Some(json!({
                                            "jsonrpc": "2.0",
                                            "id": id,
                                            "error": { "code": -32000, "message": "security policy violation: MCP session cannot invoke daemon/decide" }
                                        }))
                                    } else {
                                        match watcher_conn.list() {
                                            Ok(list) => {
                                                Some(decide_rpc(id, &req_val["params"], &list))
                                            }
                                            Err(err) => Some(json!({
                                                "jsonrpc": "2.0",
                                                "id": id,
                                                "error": { "code": -32000, "message": err.to_string() }
                                            })),
                                        }
                                    }
                                }
                                _ => {
                                    if method != "ping" {
                                        is_mcp_session = true;
                                    }
                                    if method == "initialize" {
                                        if let Some(policy_val) = req_val
                                            .get("params")
                                            .and_then(|p| p.get("policy"))
                                            .or_else(|| {
                                                req_val
                                                    .get("params")
                                                    .and_then(|p| p.get("capabilities"))
                                                    .and_then(|c| c.get("policy"))
                                            })
                                        {
                                            if let Some(parsed_policy) =
                                                parse_session_policy(policy_val)
                                            {
                                                session_policy = Some(parsed_policy);
                                            }
                                        }
                                    }
                                    // Standard MCP JSON-RPC delegation
                                    let watched = match watcher_conn.list() {
                                        Ok(list) => list,
                                        Err(err) => {
                                            eprintln!("02-agentd: watch list: {err}");
                                            Vec::new()
                                        }
                                    };
                                    let repo_path = resolve_mcp_repo_path(
                                        &req_val,
                                        &mut session_repo,
                                        &watched,
                                        &default_dir,
                                    );
                                    let mut allowed_roots = watched;
                                    if let Some(ref sr) = session_repo {
                                        if !allowed_roots.contains(sr) {
                                            allowed_roots.push(sr.clone());
                                        }
                                    }
                                    if !allowed_roots.contains(&default_dir) {
                                        allowed_roots.push(default_dir.clone());
                                    }

                                    let mut server =
                                        McpServer::with_allowed_roots(repo_path, allowed_roots);
                                    if let Some(ref policy) = session_policy {
                                        server = server.with_policy(policy.clone());
                                    }
                                    server.handle_message(req_val).map(|resp| json!(resp))
                                }
                            };

                            if let Some(resp) = response {
                                let _ = writeln!(writer, "{}", resp);
                                let _ = writer.flush();
                            }
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(_) => break,
            }
        }
    }

    DaemonSocket::cleanup(&socket_path);
    Ok(())
}

fn watch_result_response(
    id: serde_json::Value,
    field: &str,
    result: Result<bool>,
) -> serde_json::Value {
    match result {
        Ok(changed) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { field: changed }
        }),
        Err(err) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32000, "message": err.to_string() }
        }),
    }
}
