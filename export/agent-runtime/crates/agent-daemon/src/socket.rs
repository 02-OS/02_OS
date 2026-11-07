use agent_core::{paths::StoragePaths, AgentError, Result};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub struct DaemonSocket;

impl DaemonSocket {
    /// Determine appropriate Unix domain socket path for user session.
    pub fn socket_path() -> Result<PathBuf> {
        if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
            let path = PathBuf::from(runtime_dir).join("02agent.sock");
            return Ok(path);
        }

        let base = StoragePaths::data_dir()?;
        StoragePaths::ensure_private_dir(&base)?;
        Ok(base.join("02agent.sock"))
    }

    pub fn current_uid() -> u32 {
        unsafe { libc::getuid() }
    }

    /// UID of the process on the other end of `stream`.
    pub fn peer_uid(stream: &UnixStream) -> Result<u32> {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut libc::ucred as *mut libc::c_void,
                &mut len,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(cred.uid)
    }

    /// Accept the stream only when it belongs to the daemon's user.
    pub fn accepts_peer(stream: &UnixStream) -> bool {
        match Self::peer_uid(stream) {
            Ok(uid) => uid == Self::current_uid(),
            Err(_) => false,
        }
    }

    /// Bind to Unix domain socket, cleaning up stale sockets if present.
    pub fn bind(path: &PathBuf) -> Result<UnixListener> {
        if path.exists() {
            // Check if another daemon instance is actively listening
            if UnixStream::connect(path).is_ok() {
                return Err(AgentError::General(format!(
                    "Another 02-agentd daemon is already listening on {}",
                    path.display()
                )));
            }
            // Stale socket from previous run; safe to unlink
            let _ = fs::remove_file(path);
        }

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                StoragePaths::ensure_private_dir(parent)?;
            }
        }

        let listener = UnixListener::bind(path).map_err(|e| {
            AgentError::General(format!("Failed to bind socket {}: {}", path.display(), e))
        })?;

        // Ensure user-only permissions (0600)
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));

        Ok(listener)
    }

    /// Clean up socket file on shutdown.
    pub fn cleanup(path: &PathBuf) {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
}
