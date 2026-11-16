use crate::error::{AgentError, Result};
use crate::paths::StoragePaths;
use crate::types::SecretPattern;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Read cryptographically random bytes from the OS CSPRNG and encode as lowercase hex.
pub fn random_hex(byte_len: usize) -> Result<String> {
    let mut bytes = vec![0u8; byte_len];
    let mut rng = File::open("/dev/urandom")?;
    rng.read_exact(&mut bytes)?;
    Ok(hex::encode(bytes))
}

/// Reject absolute paths, `..`, and NUL so a caller-supplied path cannot leave `root`.
pub fn reject_escaping_relative(relative: &Path) -> Result<()> {
    if relative.as_os_str().is_empty() {
        return Err(AgentError::Security("evidence path is empty".to_string()));
    }
    if relative.is_absolute() {
        return Err(AgentError::Security(
            "absolute paths are outside the repository".to_string(),
        ));
    }
    let text = relative.to_string_lossy();
    if text.contains('\0') {
        return Err(AgentError::Security("path contains a NUL byte".to_string()));
    }
    for component in relative.components() {
        match component {
            Component::Normal(name) if !name.is_empty() => {}
            Component::CurDir => {}
            _ => {
                return Err(AgentError::Security(
                    "path escapes the repository".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn push_normal(cursor: &mut PathBuf, component: Component<'_>) -> Result<()> {
    match component {
        Component::Normal(name) if !name.is_empty() => {
            cursor.push(name);
            Ok(())
        }
        _ => Err(AgentError::Security(
            "path escapes the repository".to_string(),
        )),
    }
}

/// Walk `relative` under `root`. Every existing component must be a real directory or the final
/// regular file. Symlinks are rejected instead of followed.
pub fn resolve_nofollow(root: &Path, relative: &Path) -> Result<PathBuf> {
    reject_escaping_relative(relative)?;
    let mut cursor = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        if matches!(component, Component::CurDir) {
            continue;
        }
        push_normal(&mut cursor, *component)?;
        match fs::symlink_metadata(&cursor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AgentError::Security(format!(
                    "refusing to follow symlink {}",
                    cursor.display()
                )));
            }
            Ok(meta) => {
                let last = index + 1 == components.len();
                if !last && !meta.is_dir() {
                    return Err(AgentError::Security(format!(
                        "path component is not a directory: {}",
                        cursor.display()
                    )));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(AgentError::Security(format!(
                    "path not found: {}",
                    cursor.display()
                )));
            }
            Err(err) => return Err(err.into()),
        }
    }
    if cursor == root {
        return Err(AgentError::Security("evidence path is empty".to_string()));
    }
    Ok(cursor)
}

fn open_nofollow(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    options.open(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound || err.raw_os_error() == Some(libc::ELOOP) {
            AgentError::Security(format!("refusing to open {}", path.display()))
        } else {
            AgentError::Io(err)
        }
    })
}

fn canonical_inside_canon(root_canon: &Path, path: &Path) -> Result<()> {
    let path_canon = fs::canonicalize(path)?;
    if path_canon.starts_with(root_canon) {
        Ok(())
    } else {
        Err(AgentError::Security(
            "canonical path escapes the repository".to_string(),
        ))
    }
}

struct ContainedFile {
    file: File,
}

fn open_contained(root: &Path, relative: &Path, max_bytes: u64) -> Result<ContainedFile> {
    let root_canon = fs::canonicalize(root)?;
    open_contained_canon(root, &root_canon, relative, max_bytes)
}

fn open_contained_canon(
    root: &Path,
    root_canon: &Path,
    relative: &Path,
    max_bytes: u64,
) -> Result<ContainedFile> {
    if SecretPattern::is_secret(relative) {
        return Err(AgentError::Security(
            "secret filename is not evidence".to_string(),
        ));
    }
    let path = resolve_nofollow(root, relative)?;
    let meta = fs::symlink_metadata(&path)?;
    if !meta.file_type().is_file() {
        return Err(AgentError::Security(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    if meta.len() > max_bytes {
        return Err(AgentError::Security(
            "file exceeds the configured size limit".to_string(),
        ));
    }
    let dev = meta.dev();
    let ino = meta.ino();
    let file = open_nofollow(&path)?;
    let opened = file.metadata()?;
    if opened.dev() != dev || opened.ino() != ino || !opened.file_type().is_file() {
        return Err(AgentError::Security(
            "file identity changed while opening".to_string(),
        ));
    }
    canonical_inside_canon(root_canon, &path)?;
    Ok(ContainedFile { file })
}

/// Open a regular file inside `root` without following symlinks.
pub fn validate_evidence_file(root: &Path, relative: &Path, max_bytes: u64) -> Result<()> {
    let _opened = open_contained(root, relative, max_bytes)?;
    Ok(())
}

fn read_contained_bytes(opened: ContainedFile, max_bytes: u64) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    opened
        .file
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > max_bytes {
        return Err(AgentError::Security(
            "file exceeds the configured size limit".to_string(),
        ));
    }
    Ok(buf)
}

pub fn read_regular_bytes_within(root: &Path, relative: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let opened = open_contained(root, relative, max_bytes)?;
    read_contained_bytes(opened, max_bytes)
}

pub fn read_regular_text_within(root: &Path, relative: &Path, max_bytes: u64) -> Result<String> {
    let bytes = read_regular_bytes_within(root, relative, max_bytes)?;
    String::from_utf8(bytes)
        .map_err(|_| AgentError::Security("file is not valid UTF-8".to_string()))
}

/// Read `absolute` when it is a regular file lexically inside `root`.
pub fn read_indexed_bytes(root: &Path, absolute: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let relative = absolute
        .strip_prefix(root)
        .map_err(|_| AgentError::Security("indexed path is outside the repository".to_string()))?;
    read_regular_bytes_within(root, relative, max_bytes)
}

/// Read `absolute` using a pre-canonicalized repository root to avoid repeated `canonicalize(root)`.
pub fn read_indexed_bytes_canon(
    root: &Path,
    root_canon: &Path,
    absolute: &Path,
    max_bytes: u64,
) -> Result<Vec<u8>> {
    let relative = absolute
        .strip_prefix(root)
        .map_err(|_| AgentError::Security("indexed path is outside the repository".to_string()))?;
    let opened = open_contained_canon(root, root_canon, relative, max_bytes)?;
    read_contained_bytes(opened, max_bytes)
}

/// Stream SHA-256 of a contained regular file without allocating its entire content buffer.
pub fn hash_indexed_file_canon(
    root: &Path,
    root_canon: &Path,
    absolute: &Path,
    max_bytes: u64,
) -> Result<String> {
    let relative = absolute
        .strip_prefix(root)
        .map_err(|_| AgentError::Security("indexed path is outside the repository".to_string()))?;
    let mut opened = open_contained_canon(root, root_canon, relative, max_bytes)?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut chunk = [0u8; 32 * 1024];
    loop {
        let read = opened.file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(AgentError::Security(
                "file exceeds the configured size limit".to_string(),
            ));
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Atomically create/replace `path` with mode `0o600` using a randomized `O_NOFOLLOW` temp file.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            StoragePaths::ensure_private_dir(parent)?;
        }
    }
    for legacy_tmp in [
        path.with_extension("tmp"),
        PathBuf::from(format!("{}.tmp", path.display())),
    ] {
        if let Ok(meta) = fs::symlink_metadata(&legacy_tmp) {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(AgentError::Io(std::io::Error::from_raw_os_error(
                    libc::EISDIR,
                )));
            }
        }
    }
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let pid = std::process::id();
    let mut attempts = 0usize;
    let (temp_path, mut file) = loop {
        let seq = TEMP_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
        let nonce = random_hex(8)?;
        let candidate = dir.join(format!(".{stem}.{pid}.{seq}.{nonce}.tmp"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && attempts < 16 => {
                attempts += 1;
                continue;
            }
            Err(err) if err.raw_os_error() == Some(libc::ELOOP) => {
                return Err(AgentError::Security(format!(
                    "refusing to write through symlink {}",
                    candidate.display()
                )));
            }
            Err(err) => return Err(AgentError::Io(err)),
        }
    };
    let write_res = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);
    if let Err(err) = write_res {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }
    if let Err(err) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(AgentError::Io(err));
    }
    if let Ok(parent_dir) = File::open(dir) {
        let _ = parent_dir.sync_all();
    }
    Ok(())
}

/// Create a new regular file. An existing symlink is not followed or replaced.
pub fn write_new_nofollow(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|err| {
        if err.raw_os_error() == Some(libc::ELOOP)
            || err.kind() == std::io::ErrorKind::AlreadyExists
        {
            AgentError::Security(format!("refusing to write through {}", path.display()))
        } else {
            AgentError::Io(err)
        }
    })?;
    file.write_all(bytes)?;
    Ok(())
}

/// True when any existing component of `relative` under `root` is a symlink.
pub fn contains_symlink(root: &Path, relative: &Path) -> bool {
    if reject_escaping_relative(relative).is_err() {
        return true;
    }
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        if matches!(component, Component::CurDir) {
            continue;
        }
        let Component::Normal(name) = component else {
            return true;
        };
        cursor.push(name);
        match fs::symlink_metadata(&cursor) {
            Ok(meta) if meta.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}
