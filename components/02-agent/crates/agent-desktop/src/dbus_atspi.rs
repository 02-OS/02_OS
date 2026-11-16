//! Minimal local AT-SPI2 / D-Bus client over `unix:path=` sockets.
//!
//! Avoids external toolkit crates while letting `SessionTree` talk to a live
//! `org.a11y.Bus` registry when available and fail closed otherwise.

use agent_core::{AgentError, Result, SecretPattern};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const MAX_ATSPI_DEPTH: usize = 4;
const MAX_ATSPI_CHILDREN: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct AtSpiEntry {
    pub id: String,
    pub title: String,
    pub role: String,
    pub focused: bool,
}

/// Extract the first valid local `unix:path=` socket from a potentially
/// semicolon-separated `DBUS_SESSION_BUS_ADDRESS` string.
pub(crate) fn parse_local_unix_socket(address: Option<&str>) -> Result<String> {
    let Some(address) = address.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(AgentError::General(
            "desktop automation found no session bus".into(),
        ));
    };

    let mut saw_abstract = false;
    for transport in address.split(';') {
        let transport = transport.trim();
        if transport.is_empty() {
            continue;
        }
        let Some(after_unix) = transport.strip_prefix("unix:") else {
            continue;
        };
        for param in after_unix.split(',') {
            let param = param.trim();
            if let Some(path) = param.strip_prefix("path=") {
                if path.contains("..") || path.contains('\0') {
                    return Err(AgentError::Security("session bus path is not local".into()));
                }
                if !path.is_empty() {
                    return Ok(path.to_string());
                }
            } else if param.starts_with("abstract=") {
                saw_abstract = true;
            }
        }
    }

    if saw_abstract {
        return Err(AgentError::General(
            "desktop automation does not use an abstract session bus".into(),
        ));
    }
    Err(AgentError::General(
        "session bus address is not a local socket".into(),
    ))
}

fn connect_bus(socket_path: &str) -> Result<UnixStream> {
    let stream = UnixStream::connect(socket_path)
        .map_err(|err| AgentError::General(format!("session bus is not reachable: {err}")))?;
    let timeout = Some(Duration::from_millis(250));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    Ok(stream)
}

fn write_line_read_crlf(stream: &mut UnixStream, line: &str) -> Result<String> {
    stream
        .write_all(line.as_bytes())
        .map_err(|_| unavailable_err())?;
    let mut buf = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    while buf.len() < 1024 {
        match stream.read(&mut byte) {
            Ok(0) => return Err(unavailable_err()),
            Ok(_) => {
                buf.push(byte[0]);
                if buf.ends_with(b"\r\n") {
                    break;
                }
            }
            Err(_) => return Err(unavailable_err()),
        }
    }
    String::from_utf8(buf).map_err(|_| unavailable_err())
}

fn dbus_handshake(stream: &mut UnixStream) -> Result<()> {
    // D-Bus external SASL auth requires one NUL byte followed by AUTH EXTERNAL <uid-hex>\r\n.
    let uid = unsafe { libc::geteuid() }.to_string();
    let uid_hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
    let auth_cmd = format!("\0AUTH EXTERNAL {uid_hex}\r\n");
    let resp = write_line_read_crlf(stream, &auth_cmd)?;
    if !resp.starts_with("OK ") {
        return Err(unavailable_err());
    }
    stream
        .write_all(b"BEGIN\r\n")
        .map_err(|_| unavailable_err())?;
    Ok(())
}

fn encode_dbus_string(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out.push(0);
}

fn pad_to(out: &mut Vec<u8>, align: usize) {
    while !out.len().is_multiple_of(align) {
        out.push(0);
    }
}

fn write_dbus_header_field(fields: &mut Vec<u8>, code: u8, sig: &str, val: &str) {
    pad_to(fields, 8);
    fields.push(code);
    fields.push(sig.len() as u8);
    fields.extend_from_slice(sig.as_bytes());
    fields.push(0);
    pad_to(fields, 4);
    encode_dbus_string(fields, val);
}

fn build_method_call(serial: u32, dest: &str, path: &str, iface: &str, member: &str) -> Vec<u8> {
    let mut fields = Vec::new();
    write_dbus_header_field(&mut fields, 1, "o", path);
    write_dbus_header_field(&mut fields, 2, "s", iface);
    write_dbus_header_field(&mut fields, 3, "s", member);
    write_dbus_header_field(&mut fields, 6, "s", dest);

    let mut msg = vec![
        b'l', // little-endian
        1,    // METHOD_CALL
        0,    // flags
        1,    // protocol version
    ];
    msg.extend_from_slice(&0u32.to_le_bytes()); // empty body length
    msg.extend_from_slice(&serial.to_le_bytes());
    msg.extend_from_slice(&(fields.len() as u32).to_le_bytes());
    msg.extend_from_slice(&fields);
    pad_to(&mut msg, 8);
    msg
}

fn read_dbus_reply(stream: &mut UnixStream, want_serial: u32) -> Result<(u8, Vec<u8>)> {
    for _ in 0..8 {
        let mut header = [0u8; 16];
        stream
            .read_exact(&mut header)
            .map_err(|_| unavailable_err())?;
        if header[0] != b'l' {
            return Err(unavailable_err());
        }
        let msg_type = header[1];
        let body_len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let fields_len =
            u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
        if fields_len > 64 * 1024 || body_len > 256 * 1024 {
            return Err(unavailable_err());
        }
        let padded_fields = (fields_len + 7) & !7;
        let mut rest = vec![0u8; padded_fields + body_len];
        stream
            .read_exact(&mut rest)
            .map_err(|_| unavailable_err())?;
        let fields = &rest[..fields_len];
        let body = rest[padded_fields..].to_vec();
        if msg_type == 4 {
            // SIGNAL: keep waiting for our METHOD_RETURN / ERROR.
            continue;
        }
        let mut reply_serial = None;
        let mut pos = 0usize;
        while pos + 4 <= fields.len() {
            pos = (pos + 7) & !7;
            if pos >= fields.len() {
                break;
            }
            let code = fields[pos];
            pos += 1;
            if pos >= fields.len() {
                break;
            }
            let sig_len = fields[pos] as usize;
            pos += 1 + sig_len + 1;
            if pos > fields.len() {
                break;
            }
            let sig_byte = fields[pos - sig_len - 1];
            pos = (pos + 3) & !3;
            match sig_byte {
                b'u' if pos + 4 <= fields.len() => {
                    let v = u32::from_le_bytes([
                        fields[pos],
                        fields[pos + 1],
                        fields[pos + 2],
                        fields[pos + 3],
                    ]);
                    if code == 5 {
                        reply_serial = Some(v);
                    }
                    pos += 4;
                }
                b's' | b'o' if pos + 4 <= fields.len() => {
                    let slen = u32::from_le_bytes([
                        fields[pos],
                        fields[pos + 1],
                        fields[pos + 2],
                        fields[pos + 3],
                    ]) as usize;
                    pos += 4 + slen + 1;
                }
                _ => break,
            }
        }
        if reply_serial == Some(want_serial) {
            return Ok((msg_type, body));
        }
    }
    Err(unavailable_err())
}

fn read_first_dbus_string(body: &[u8]) -> Option<String> {
    if body.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let slice = body.get(4..4 + len)?;
    std::str::from_utf8(slice).ok().map(str::to_string)
}

fn call_simple_string(
    stream: &mut UnixStream,
    serial: &mut u32,
    dest: &str,
    path: &str,
    iface: &str,
    member: &str,
) -> Result<String> {
    let cur = *serial;
    *serial += 1;
    let msg = build_method_call(cur, dest, path, iface, member);
    stream.write_all(&msg).map_err(|_| unavailable_err())?;
    let (msg_type, body) = read_dbus_reply(stream, cur)?;
    if msg_type != 2 {
        return Err(unavailable_err());
    }
    read_first_dbus_string(&body).ok_or_else(unavailable_err)
}

fn open_atspi_bus(session_address: Option<&str>) -> Result<UnixStream> {
    let session_sock = parse_local_unix_socket(session_address)?;
    let mut session = connect_bus(&session_sock)?;
    dbus_handshake(&mut session)?;
    let mut serial = 1u32;
    call_simple_string(
        &mut session,
        &mut serial,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "Hello",
    )?;
    let a11y_addr = call_simple_string(
        &mut session,
        &mut serial,
        "org.a11y.Bus",
        "/org/a11y/bus",
        "org.a11y.Bus",
        "GetAddress",
    )?;
    let a11y_sock = parse_local_unix_socket(Some(&a11y_addr)).map_err(|_| unavailable_err())?;
    let mut a11y = connect_bus(&a11y_sock).map_err(|_| unavailable_err())?;
    dbus_handshake(&mut a11y)?;
    let mut a11y_serial = 1u32;
    call_simple_string(
        &mut a11y,
        &mut a11y_serial,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "Hello",
    )?;
    Ok(a11y)
}

pub(crate) fn probe_bus_or_unavailable(session_address: Option<&str>) -> Result<()> {
    let sock = parse_local_unix_socket(session_address)?;
    let _stream = connect_bus(&sock)?;
    Err(unavailable_err())
}

pub(crate) fn session_list_windows(session_address: Option<&str>) -> Result<Vec<AtSpiEntry>> {
    let mut bus = open_atspi_bus(session_address)?;
    let mut serial = 10u32;
    let name = call_simple_string(
        &mut bus,
        &mut serial,
        "org.a11y.atspi.Registry",
        "/org/a11y/atspi/accessible/root",
        "org.a11y.atspi.Accessible",
        "GetLocalizedRoleName",
    )?;
    let title = SecretPattern::redact(&name);
    let _depth_cap = MAX_ATSPI_DEPTH;
    let _child_cap = MAX_ATSPI_CHILDREN;
    if title.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![AtSpiEntry {
            id: "atspi-root".into(),
            title,
            role: "desktop-frame".into(),
            focused: true,
        }])
    }
}

pub(crate) fn session_inspect_element(session_address: Option<&str>, id: &str) -> Result<Value> {
    let windows = session_list_windows(session_address)?;
    let found = windows
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(unavailable_err)?;
    Ok(json!({
        "id": found.id,
        "title": SecretPattern::redact(&found.title),
        "role": found.role,
        "focused": found.focused,
        "max_depth": MAX_ATSPI_DEPTH
    }))
}

fn unavailable_err() -> AgentError {
    AgentError::General("AT-SPI tree is not available to this process".into())
}
