//! Loopback page for the local status file.
//!
//! The listener is `127.0.0.1` only. The page is built from `status.json`
//! and does not open another socket.

use agent_core::{paths::StoragePaths, AgentError, Result};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const LOOPBACK_ADDR: &str = "127.0.0.1:8732";

pub fn execute() -> Result<()> {
    let listener = bind_dashboard(LOOPBACK_ADDR)
        .map_err(|err| AgentError::General(format!("dashboard listen failed: {err}")))?;
    let bound = listener
        .local_addr()
        .map_err(|err| AgentError::General(err.to_string()))?;
    println!("02 dashboard http://{bound}");
    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                if let Err(err) = handle_client(&mut stream) {
                    eprintln!("dashboard request failed: {err}");
                }
            }
            Err(err) => eprintln!("dashboard connection failed: {err}"),
        }
    }
    Ok(())
}

pub fn bind_dashboard(addr: &str) -> std::io::Result<TcpListener> {
    let parsed: SocketAddr = addr
        .parse()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    if parsed.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "dashboard listens on 127.0.0.1 only",
        ));
    }
    let listener = TcpListener::bind(parsed)?;
    let bound = listener.local_addr()?;
    if bound.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "dashboard listener is not loopback",
        ));
    }
    Ok(listener)
}

fn handle_client(stream: &mut TcpStream) -> std::io::Result<()> {
    let request = read_request(stream)?;
    let line = request.lines().next().unwrap_or("");
    if line.starts_with("GET / ") || line.starts_with("GET / HTTP") || line == "GET /" {
        let page = render_page(&status_path().unwrap_or_else(|_| PathBuf::from("status.json")));
        write_response(stream, 200, &page)
    } else {
        write_response(
            stream,
            404,
            "<!DOCTYPE html><title>02 dashboard</title><p>Not found.</p>",
        )
    }
}

fn status_path() -> Result<PathBuf> {
    Ok(StoragePaths::data_dir()?.join("status.json"))
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut buf = [0u8; 1024];
    let mut collected = Vec::new();
    loop {
        let read = stream.read(&mut buf)?;
        if read == 0 {
            break;
        }
        collected.extend_from_slice(&buf[..read]);
        if collected.windows(4).any(|window| window == b"\r\n\r\n") || collected.len() >= 8192 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&collected).into_owned())
}

fn write_response(stream: &mut TcpStream, status: u16, page: &str) -> std::io::Result<()> {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        page.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(page.as_bytes())?;
    stream.flush()
}

fn render_page(path: &Path) -> String {
    match read_snapshot(path) {
        Ok(snapshot) => page_from(&snapshot, ""),
        Err(message) => page_from(&Snapshot::default(), &message),
    }
}

#[derive(Default)]
struct Snapshot {
    index_phase: String,
    watched_count: String,
    agents: Vec<(String, String)>,
    token_counts: Vec<(String, String, String)>,
    approvals: Vec<(String, String, String)>,
    evidence: Vec<(String, String)>,
}

fn read_snapshot(path: &Path) -> std::result::Result<Snapshot, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "status file is absent".to_string())?;
    if meta.file_type().is_symlink() {
        return Err("status file is a symlink".into());
    }
    if !meta.is_file() {
        return Err("status file is not a regular file".into());
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err("status file is not private".into());
    }
    let text = fs::read_to_string(path).map_err(|_| "status file is unreadable".to_string())?;
    let value: Value =
        serde_json::from_str(&text).map_err(|_| "status file is not json".to_string())?;
    Ok(snapshot_from(&value))
}

fn snapshot_from(value: &Value) -> Snapshot {
    Snapshot {
        index_phase: text_at(value, "index_phase"),
        watched_count: match value.get("watched_count") {
            Some(Value::Number(number)) => number.to_string(),
            Some(Value::String(text)) => text.clone(),
            _ => String::new(),
        },
        agents: rows(value.get("agents"), |item| {
            (text_at(item, "id"), text_at(item, "name"))
        }),
        token_counts: token_rows(value.get("token_counts")),
        approvals: rows(value.get("approvals"), |item| {
            (
                text_at(item, "id"),
                text_at(item, "decision"),
                text_at(item, "args_hash"),
            )
        }),
        evidence: evidence_rows(value.get("evidence")),
    }
}

fn token_rows(value: Option<&Value>) -> Vec<(String, String, String)> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                (
                    first_text(item, &["agent_id", "id"]),
                    first_text(item, &["prompt", "prompt_tokens"]),
                    first_text(item, &["completion", "completion_tokens"]),
                )
            })
            .collect(),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(key, item)| (key.clone(), scalar(item), String::new()))
            .collect(),
        _ => Vec::new(),
    }
}

fn evidence_rows(value: Option<&Value>) -> Vec<(String, String)> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => (String::new(), text.clone()),
                _ => (text_at(item, "id"), first_text(item, &["summary", "text"])),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn rows<T>(value: Option<&Value>, map: impl Fn(&Value) -> T) -> Vec<T> {
    match value {
        Some(Value::Array(items)) => items.iter().map(map).collect(),
        _ => Vec::new(),
    }
}

fn text_at(value: &Value, key: &str) -> String {
    value.get(key).map(scalar).unwrap_or_default()
}

fn first_text(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).map(scalar))
        .unwrap_or_default()
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => String::new(),
    }
}

fn page_from(snapshot: &Snapshot, notice: &str) -> String {
    let mut body = String::from(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>02 dashboard</title></head><body>",
    );
    body.push_str("<h1>02 dashboard</h1>");
    body.push_str("<p>Local status. This page stays on this machine.</p>");
    if !notice.is_empty() {
        body.push_str("<p>");
        body.push_str(&escape(notice));
        body.push_str("</p>");
    } else {
        body.push_str("<p>Phase ");
        body.push_str(&escape(&snapshot.index_phase));
        body.push_str(". Watched repositories ");
        body.push_str(&escape(&snapshot.watched_count));
        body.push_str(".</p>");
    }
    body.push_str("<h2>Agents</h2><ul>");
    for (id, name) in &snapshot.agents {
        body.push_str("<li>");
        body.push_str(&escape(id));
        body.push(' ');
        body.push_str(&escape(name));
        body.push_str("</li>");
    }
    body.push_str("</ul><h2>Token counts</h2><ul>");
    for (agent, prompt, completion) in &snapshot.token_counts {
        body.push_str("<li>");
        body.push_str(&escape(agent));
        body.push_str(" prompt ");
        body.push_str(&escape(prompt));
        body.push_str(" completion ");
        body.push_str(&escape(completion));
        body.push_str("</li>");
    }
    body.push_str("</ul><h2>Approvals</h2><ul>");
    for (id, decision, hash) in &snapshot.approvals {
        body.push_str("<li>");
        body.push_str(&escape(id));
        body.push(' ');
        body.push_str(&escape(decision));
        body.push(' ');
        body.push_str(&escape(hash));
        body.push_str("</li>");
    }
    body.push_str("</ul><h2>Evidence</h2><ul>");
    for (id, summary) in &snapshot.evidence {
        body.push_str("<li>");
        body.push_str(&escape(id));
        body.push(' ');
        body.push_str(&escape(summary));
        body.push_str("</li>");
    }
    body.push_str("</ul></body></html>");
    body
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::thread;

    #[test]
    fn dashboard_binary_listener_is_loopback() {
        let rejected = bind_dashboard("0.0.0.0:9").expect_err("public bind");
        assert!(rejected.to_string().contains("127.0.0.1"));
        assert!(bind_dashboard("[::1]:0").is_err());
        assert!(bind_dashboard("127.0.0.2:0").is_err());
        assert_eq!(LOOPBACK_ADDR, "127.0.0.1:8732");
        let listener = bind_dashboard("127.0.0.1:0").expect("loopback");
        let bound = listener.local_addr().expect("addr");
        assert_eq!(bound.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn dashboard_page_reads_the_private_status_file() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("status.json");
        fs::write(
            &path,
            r#"{"index_phase":"idle","watched_count":1,"agents":[{"id":"local","name":"02"}],"token_counts":[{"agent_id":"local","prompt":3,"completion":1}],"approvals":[{"id":"a1","decision":"allow","args_hash":"abcd"}],"evidence":[{"id":"e1","summary":"<fact>"}]}"#,
        )
        .expect("write");
        let mut permissions = fs::metadata(&path).expect("meta").permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&path, permissions).expect("mode");

        let page = render_page(&path);
        assert!(page.contains("<h2>Agents</h2>"));
        assert!(page.contains("<h2>Token counts</h2>"));
        assert!(page.contains("<h2>Approvals</h2>"));
        assert!(page.contains("<h2>Evidence</h2>"));
        assert!(page.contains("local"));
        assert!(page.contains("prompt 3"));
        assert!(page.contains("allow"));
        assert!(page.contains("&lt;fact&gt;"));
        assert!(!page.contains("<fact>"));

        let listener = bind_dashboard("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let served = page.clone();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            write_response(&mut stream, 200, &served).expect("write");
        });
        let mut client = TcpStream::connect(addr).expect("connect");
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .expect("request");
        let mut body = String::new();
        client.read_to_string(&mut body).expect("read");
        assert!(body.contains("Token counts"));
        assert!(body.contains("abcd"));
        handle.join().expect("server");

        permissions = fs::metadata(&path).expect("meta").permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&path, permissions).expect("mode");
        let loose = render_page(&path);
        assert!(loose.contains("status file is not private"));
        assert!(loose.contains("<h2>Agents</h2>"));
    }
}
