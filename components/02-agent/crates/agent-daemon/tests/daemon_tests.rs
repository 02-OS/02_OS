use agent_daemon::{DaemonSocket, RepoWatcher};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use tempfile::tempdir;

#[test]
fn test_socket_bind_and_permissions() {
    let dir = tempdir().unwrap();
    let sock_path = dir.path().join("test_agent.sock");

    let listener = DaemonSocket::bind(&sock_path).unwrap();
    assert!(sock_path.exists());

    // Check permissions are 0600 (user read/write only)
    let meta = std::fs::metadata(&sock_path).unwrap();
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // Test client connect
    let mut client = UnixStream::connect(&sock_path).unwrap();
    let (mut server_stream, _) = listener.accept().unwrap();
    let peer = DaemonSocket::peer_uid(&server_stream).unwrap();
    assert_eq!(peer, DaemonSocket::current_uid());
    assert!(DaemonSocket::accepts_peer(&server_stream));

    // Client writes message
    writeln!(
        client,
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}}"
    )
    .unwrap();
    client.flush().unwrap();

    // Server reads message
    let mut reader = BufReader::new(&mut server_stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("ping"));

    // Server responds
    writeln!(
        server_stream,
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}"
    )
    .unwrap();
    server_stream.flush().unwrap();

    // Client reads response
    let mut client_reader = BufReader::new(client);
    let mut resp = String::new();
    client_reader.read_line(&mut resp).unwrap();
    assert!(resp.contains("\"result\":{}"));

    // Cleanup
    drop(listener);
    DaemonSocket::cleanup(&sock_path);
    assert!(!sock_path.exists());
}

#[test]
fn test_repo_watcher_state() {
    let dir = tempdir().unwrap();
    let dummy_repo1 = dir.path().join("repo1");
    let dummy_repo2 = dir.path().join("repo2");

    let watcher = RepoWatcher::with_state_file(dir.path().join("watched_repos.json")).unwrap();
    assert!(watcher.add(dummy_repo1.clone()).unwrap());
    assert!(watcher.add(dummy_repo2.clone()).unwrap());

    // Duplicate add returns false
    assert!(!watcher.add(dummy_repo1.clone()).unwrap());

    let list = watcher.list().unwrap();
    assert!(list.contains(&dummy_repo1));
    assert!(list.contains(&dummy_repo2));

    // Remove repo1
    assert!(watcher.remove(&dummy_repo1).unwrap());
    let list_after = watcher.list().unwrap();
    assert!(!list_after.contains(&dummy_repo1));
    assert!(list_after.contains(&dummy_repo2));

    // Cleanup
    watcher.remove(&dummy_repo2).unwrap();
}

#[test]
fn test_resolve_mcp_repo_path() {
    use agent_daemon::resolve_mcp_repo_path;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    let default_dir = Path::new("/default");
    let mut session = None;

    let init = json!({
        "method": "initialize",
        "params": { "rootUri": "file:///tmp/my%20repo" }
    });
    let path = resolve_mcp_repo_path(&init, &mut session, &[], default_dir);
    assert_eq!(path, PathBuf::from("/tmp/my repo"));

    let call = json!({
        "method": "tools/call",
        "params": { "name": "repository_status", "arguments": {} }
    });
    let path = resolve_mcp_repo_path(&call, &mut session, &[], default_dir);
    assert_eq!(path, PathBuf::from("/tmp/my repo"));

    let file_call = json!({
        "method": "tools/call",
        "params": {
            "name": "file_outline",
            "arguments": { "path": "src/lib.rs" }
        }
    });
    let path = resolve_mcp_repo_path(&file_call, &mut session, &[], default_dir);
    assert_eq!(path, PathBuf::from("/tmp/my repo"));

    let explicit = json!({
        "method": "tools/call",
        "params": {
            "name": "repository_status",
            "arguments": { "repo_path": "/work/proj" }
        }
    });
    let path = resolve_mcp_repo_path(&explicit, &mut session, &[], default_dir);
    assert_eq!(path, PathBuf::from("/work/proj"));

    session = None;
    let watched = vec![PathBuf::from("/only/repo")];
    let path = resolve_mcp_repo_path(&call, &mut session, &watched, default_dir);
    assert_eq!(path, PathBuf::from("/only/repo"));

    session = None;
    let watched = vec![PathBuf::from("/a"), PathBuf::from("/b")];
    let path = resolve_mcp_repo_path(&call, &mut session, &watched, default_dir);
    assert_eq!(path, default_dir);
}

#[test]
fn concurrent_watch_registrations_persist() {
    let dir = tempdir().unwrap();
    let watcher =
        Arc::new(RepoWatcher::with_state_file(dir.path().join("watched_repos.json")).unwrap());
    let mut handles = Vec::new();
    for thread_id in 0..32 {
        let watcher = Arc::clone(&watcher);
        handles.push(thread::spawn(move || {
            for item in 0..8 {
                let path = PathBuf::from(format!("/virtual/repo-{thread_id}-{item}"));
                assert!(watcher.add(path).unwrap());
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let state = watcher.load_state().unwrap();
    assert_eq!(state.repositories.len(), 256);
    for thread_id in 0..32 {
        for item in 0..8 {
            assert!(state
                .repositories
                .contains(&PathBuf::from(format!("/virtual/repo-{thread_id}-{item}"))));
        }
    }
}

#[test]
fn malformed_watch_state_returns_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("watched_repos.json");
    let torn = "{\"repositories\":[";
    fs::write(&path, torn).unwrap();
    let watcher = RepoWatcher::with_state_file(path.clone()).unwrap();
    assert!(watcher.load_state().is_err());
    assert!(watcher.add(PathBuf::from("/repo")).is_err());
    assert!(watcher.list().is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), torn);

    let as_dir = dir.path().join("not-a-file");
    fs::create_dir(&as_dir).unwrap();
    let watcher = RepoWatcher::with_state_file(as_dir).unwrap();
    assert!(watcher.load_state().is_err());
}

#[test]
fn bind_leaves_non_sockets_untouched_and_replaces_stale_socket() {
    let dir = tempdir().unwrap();

    let file_path = dir.path().join("marker");
    fs::write(&file_path, b"keep-me").unwrap();
    assert!(DaemonSocket::bind(&file_path).is_err());
    assert_eq!(fs::read(&file_path).unwrap(), b"keep-me");

    let dir_path = dir.path().join("a-directory");
    fs::create_dir(&dir_path).unwrap();
    assert!(DaemonSocket::bind(&dir_path).is_err());
    assert!(dir_path.is_dir());

    let target = dir.path().join("symlink-target");
    fs::write(&target, b"safe").unwrap();
    let link = dir.path().join("symlink-sock");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(DaemonSocket::bind(&link).is_err());
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(fs::read(&target).unwrap(), b"safe");

    let live = dir.path().join("live.sock");
    let listener = DaemonSocket::bind(&live).unwrap();
    let live_link = dir.path().join("live-link.sock");
    std::os::unix::fs::symlink(&live, &live_link).unwrap();
    assert!(DaemonSocket::bind(&live_link).is_err());
    assert!(live_link
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    let err = DaemonSocket::bind(&live).unwrap_err();
    assert!(err.to_string().contains("already listening"), "{err}");
    assert!(UnixStream::connect(&live).is_ok());
    drop(listener);

    let stale = dir.path().join("stale.sock");
    let stale_listener = DaemonSocket::bind(&stale).unwrap();
    drop(stale_listener);
    let replaced = DaemonSocket::bind(&stale).unwrap();
    assert!(stale.symlink_metadata().unwrap().file_type().is_socket());
    drop(replaced);
}
