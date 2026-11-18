use agent_core::{paths::StoragePaths, AgentError, Result};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static BOUND_INODES: OnceLock<Mutex<HashMap<PathBuf, (u64, u64)>>> = OnceLock::new();

fn bound_inodes() -> &'static Mutex<HashMap<PathBuf, (u64, u64)>> {
    BOUND_INODES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct DaemonSocket;

impl DaemonSocket {
    /// Determine appropriate Unix domain socket path for user session.
    /// Requires `XDG_RUNTIME_DIR` to be non-empty and absolute (`#144`).
    pub fn socket_path() -> Result<PathBuf> {
        if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
            let trimmed = runtime_dir.trim();
            let runtime_path = Path::new(trimmed);
            if !trimmed.is_empty() && runtime_path.is_absolute() {
                return Ok(runtime_path.join("02agent.sock"));
            }
        }

        let base = StoragePaths::data_dir()?;
        StoragePaths::ensure_private_dir(&base)?;
        Ok(base.join("02agent.sock"))
    }

    pub fn current_uid() -> u32 {
        unsafe { libc::getuid() }
    }

    /// `(uid, pid)` of the process on the other end of `stream`.
    pub fn peer_cred(stream: &UnixStream) -> Result<(u32, i32)> {
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
        Ok((cred.uid, cred.pid))
    }

    /// UID of the process on the other end of `stream`.
    pub fn peer_uid(stream: &UnixStream) -> Result<u32> {
        Ok(Self::peer_cred(stream)?.0)
    }

    /// Accept the stream only when it belongs to the daemon's user.
    pub fn accepts_peer(stream: &UnixStream) -> bool {
        match Self::peer_uid(stream) {
            Ok(uid) => uid == Self::current_uid(),
            Err(_) => false,
        }
    }

    /// True when the connected peer process is an `02 mcp` stdio server (`#132`).
    pub fn peer_is_mcp_server(stream: &UnixStream) -> bool {
        let Ok((_, pid)) = Self::peer_cred(stream) else {
            return true;
        };
        if pid <= 0 {
            return false;
        }
        let Ok(cmdline) = fs::read(format!("/proc/{pid}/cmdline")) else {
            return false;
        };
        let args: Vec<String> = cmdline
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        args.iter().any(|arg| arg == "mcp")
            && args
                .first()
                .map(|bin| bin.ends_with("/02") || bin == "02")
                .unwrap_or(false)
    }

    /// Bind to Unix domain socket, replacing only a stale socket we own.
    pub fn bind(path: &PathBuf) -> Result<UnixListener> {
        match fs::symlink_metadata(path) {
            Ok(meta) => {
                let kind = meta.file_type();
                // lstat: a symlink is not a socket, even if its target is one.
                if kind.is_symlink() || !kind.is_socket() {
                    let label = if kind.is_symlink() {
                        "symlink"
                    } else if kind.is_dir() {
                        "directory"
                    } else if kind.is_file() {
                        "regular file"
                    } else {
                        "non-socket"
                    };
                    return Err(AgentError::General(format!(
                        "Refusing to replace {} at {} (not a socket)",
                        label,
                        path.display()
                    )));
                }
                if meta.uid() != Self::current_uid() {
                    return Err(AgentError::General(format!(
                        "Refusing to unlink socket {} owned by uid {}",
                        path.display(),
                        meta.uid()
                    )));
                }
                if UnixStream::connect(path).is_ok() {
                    return Err(AgentError::General(format!(
                        "Another 02-agentd daemon is already listening on {}",
                        path.display()
                    )));
                }
                fs::remove_file(path).map_err(|err| {
                    AgentError::General(format!(
                        "Failed to unlink stale socket {}: {err}",
                        path.display()
                    ))
                })?;
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
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

        if let Ok(meta) = fs::symlink_metadata(path) {
            if let Ok(mut guard) = bound_inodes().lock() {
                guard.insert(path.clone(), (meta.dev(), meta.ino()));
            }
        }

        Ok(listener)
    }

    /// Clean up socket file on shutdown only if `(st_dev, st_ino)` matches our bound socket (`#144`).
    pub fn cleanup(path: &PathBuf) {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return;
        };
        let kind = meta.file_type();
        if kind.is_symlink() || !kind.is_socket() || meta.uid() != Self::current_uid() {
            return;
        }
        if let Ok(mut guard) = bound_inodes().lock() {
            if let Some(&(expected_dev, expected_ino)) = guard.get(path) {
                if meta.dev() != expected_dev || meta.ino() != expected_ino {
                    return;
                }
                guard.remove(path);
            }
        }
        let _ = fs::remove_file(path);
    }
}
