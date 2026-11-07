use agent_core::Result;
use agent_daemon::{resolve_mcp_repo_path, DaemonSocket, RepoWatcher};
use agent_mcp::McpServer;
use clap::Parser;
use serde_json::json;
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
            thread::sleep(interval_dur);
        }
    });

    // Accept incoming socket connections
    listener.set_nonblocking(true)?;
    let raw_fd = listener.as_raw_fd();

    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

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
                Ok((stream, _)) => {
                    if !DaemonSocket::accepts_peer(&stream) {
                        drop(stream);
                        continue;
                    }
                    let watcher_conn = watcher.clone();
                    let default_dir = current_dir.clone();

                    thread::spawn(move || {
                        let mut reader = BufReader::new(match stream.try_clone() {
                            Ok(s) => s,
                            Err(_) => return,
                        });
                        let mut writer = stream;
                        let mut line = String::new();
                        let mut session_repo = None;

                        while let Ok(n) = reader.read_line(&mut line) {
                            if n == 0 {
                                break;
                            }

                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                line.clear();
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
                                    line.clear();
                                    continue;
                                }
                            };

                            let method = req_val["method"].as_str().unwrap_or("");
                            let id = req_val.get("id").cloned().unwrap_or(json!(null));

                            // Daemon management extension methods
                            let response = match method {
                                "daemon/watch" => {
                                    let path_str = req_val["params"]["path"].as_str().unwrap_or("");
                                    let path = PathBuf::from(path_str);
                                    let added = watcher_conn.add(path).unwrap_or(false);
                                    Some(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": { "added": added }
                                    }))
                                }
                                "daemon/unwatch" => {
                                    let path_str = req_val["params"]["path"].as_str().unwrap_or("");
                                    let path = PathBuf::from(path_str);
                                    let removed = watcher_conn.remove(&path).unwrap_or(false);
                                    Some(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": { "removed": removed }
                                    }))
                                }
                                "daemon/list_watched" => {
                                    let list = watcher_conn.list();
                                    Some(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": { "repositories": list }
                                    }))
                                }
                                _ => {
                                    // Standard MCP JSON-RPC delegation
                                    let watched = watcher_conn.list();
                                    let repo_path = resolve_mcp_repo_path(
                                        &req_val,
                                        &mut session_repo,
                                        &watched,
                                        &default_dir,
                                    );

                                    let server = McpServer::new(repo_path);
                                    if let Ok(rpc_req) = serde_json::from_value(req_val) {
                                        server.handle_request(rpc_req).map(|resp| json!(resp))
                                    } else {
                                        None
                                    }
                                }
                            };

                            if let Some(resp) = response {
                                let _ = writeln!(writer, "{}", resp);
                                let _ = writer.flush();
                            }

                            line.clear();
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
