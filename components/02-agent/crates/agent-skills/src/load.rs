//! Read one `SKILL.toml` from inside a skill root.
//!
//! The relative path is `SKILL.toml` or `<name>/SKILL.toml`. Parent
//! components, absolute paths, and symlinks are rejected before the file is read.

use std::ffi::CString;
use std::fs::{self, File};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

use crate::tools::is_known_tool;

const MAX_SKILL_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub version: String,
    pub tools: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("skill path escapes the skill root")]
    PathEscape,
    #[error("skill file is missing")]
    Missing,
    #[error("skill file is not a regular file")]
    NotRegular,
    #[error("skill file is too large")]
    TooLarge,
    #[error("skill document is invalid: {0}")]
    Invalid(String),
    #[error("unknown tool {0}")]
    UnknownTool(String),
    #[error("agent id is invalid")]
    InvalidAgentId,
    #[error("context scope is invalid")]
    InvalidScope,
    #[error("capability token is empty")]
    EmptyToken,
    #[error("capability token is invalid")]
    InvalidToken,
    #[error("capability token has expired")]
    ExpiredToken,
    #[error("tool {0} is not authorized for the active skill")]
    UnauthorizedTool(String),
    #[error("context access denied for scope")]
    ScopeDenied,
}

fn map_open_error(err: std::io::Error) -> SkillError {
    match err.raw_os_error() {
        Some(libc::ENOENT) => SkillError::Missing,
        Some(libc::ELOOP) => SkillError::PathEscape,
        Some(libc::ENOTDIR) => SkillError::NotRegular,
        _ => SkillError::NotRegular,
    }
}

fn open_root_nofollow(root: &Path) -> Result<OwnedFd, SkillError> {
    let raw = CString::new(root.as_os_str().as_bytes()).map_err(|_| SkillError::PathEscape)?;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    // SAFETY: `raw` is NUL-terminated and valid for the syscall duration; returned fd is wrapped immediately.
    let fd = unsafe { libc::open(raw.as_ptr(), flags) };
    if fd < 0 {
        return Err(map_open_error(std::io::Error::last_os_error()));
    }
    // SAFETY: `fd` is a newly opened descriptor owned exclusively by this caller.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let file = File::from(owned);
    let meta = file.metadata().map_err(|_| SkillError::NotRegular)?;
    if !meta.is_dir() {
        return Err(SkillError::NotRegular);
    }
    Ok(file.into())
}

fn open_dir_at_nofollow(parent: &OwnedFd, segment: &str) -> Result<OwnedFd, SkillError> {
    let raw = CString::new(segment).map_err(|_| SkillError::PathEscape)?;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    // SAFETY: `parent` is a valid open directory fd and `raw` is a NUL-terminated single segment.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), raw.as_ptr(), flags) };
    if fd < 0 {
        return Err(map_open_error(std::io::Error::last_os_error()));
    }
    // SAFETY: `fd` is newly opened and owned by the caller.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let file = File::from(owned);
    let meta = file.metadata().map_err(|_| SkillError::NotRegular)?;
    if !meta.is_dir() {
        return Err(SkillError::NotRegular);
    }
    Ok(file.into())
}

fn open_file_at_nofollow(parent: &OwnedFd, filename: &str) -> Result<File, SkillError> {
    let raw = CString::new(filename).map_err(|_| SkillError::PathEscape)?;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    // SAFETY: `parent` is a valid open directory fd and `raw` is a NUL-terminated filename.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), raw.as_ptr(), flags) };
    if fd < 0 {
        return Err(map_open_error(std::io::Error::last_os_error()));
    }
    // SAFETY: `fd` is newly opened and transferred immediately to RAII `File`.
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub fn load_skill(root: &Path, relative: &Path) -> Result<Skill, SkillError> {
    let parts = skill_parts(relative)?;
    let mut dir_fd = open_root_nofollow(root)?;
    for segment in &parts[..parts.len() - 1] {
        dir_fd = open_dir_at_nofollow(&dir_fd, segment)?;
    }
    let filename = &parts[parts.len() - 1];
    let file = open_file_at_nofollow(&dir_fd, filename)?;
    let meta = file.metadata().map_err(|_| SkillError::NotRegular)?;
    if !meta.is_file() {
        return Err(SkillError::NotRegular);
    }
    if meta.len() > MAX_SKILL_BYTES {
        return Err(SkillError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_SKILL_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| SkillError::Invalid("unreadable skill file".into()))?;
    if bytes.len() as u64 > MAX_SKILL_BYTES {
        return Err(SkillError::TooLarge);
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| SkillError::Invalid("skill file is not valid UTF-8".into()))?;
    parse_document(&text)
}

/// Discover valid `SKILL.toml` definitions directly inside `root` or `<name>/SKILL.toml`.
pub fn discover_skills(root: &Path) -> Result<Vec<Skill>, SkillError> {
    match open_root_nofollow(root) {
        Ok(_) => {}
        Err(SkillError::Missing) => return Ok(Vec::new()),
        Err(err) => return Err(err),
    }
    let mut skills = Vec::new();
    match load_skill(root, Path::new("SKILL.toml")) {
        Ok(skill) => skills.push(skill),
        Err(SkillError::Missing) => {}
        Err(err) => return Err(err),
    }
    let entries = fs::read_dir(root).map_err(|_| SkillError::Missing)?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if valid_dir(&name) {
            if let Ok(meta) = fs::symlink_metadata(entry.path()) {
                if !meta.file_type().is_symlink() && meta.is_dir() {
                    subdirs.push(name);
                }
            }
        }
    }
    subdirs.sort();
    for name in subdirs {
        let rel = Path::new(&name).join("SKILL.toml");
        match load_skill(root, &rel) {
            Ok(skill) => skills.push(skill),
            Err(SkillError::Missing) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(skills)
}

fn skill_parts(relative: &Path) -> Result<Vec<String>, SkillError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(SkillError::PathEscape);
    }
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if text.contains('/')
                    || text.contains('\\')
                    || text.contains('\0')
                    || text.starts_with('.')
                {
                    return Err(SkillError::PathEscape);
                }
                parts.push(text.into_owned());
            }
            _ => return Err(SkillError::PathEscape),
        }
    }
    let nested = parts.len() == 2 && parts[1] == "SKILL.toml" && valid_dir(&parts[0]);
    let top = parts.len() == 1 && parts[0] == "SKILL.toml";
    if nested || top {
        Ok(parts)
    } else {
        Err(SkillError::PathEscape)
    }
}

fn valid_dir(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= 64
        && first.is_ascii_alphabetic()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

fn parse_document(text: &str) -> Result<Skill, SkillError> {
    let value: toml::Value =
        toml::from_str(text).map_err(|err| SkillError::Invalid(err.to_string()))?;
    let table = value
        .as_table()
        .ok_or_else(|| SkillError::Invalid("skill document must be a table".into()))?;
    for key in table.keys() {
        if key != "name" && key != "version" && key != "tools" {
            return Err(SkillError::Invalid(format!("unknown field {key}")));
        }
    }
    let name = required_str(table, "name")?;
    let version = required_str(table, "version")?;
    if !valid_dir(name) {
        return Err(SkillError::Invalid("name is invalid".into()));
    }
    if !valid_version(version) {
        return Err(SkillError::Invalid("version is invalid".into()));
    }
    let tools_value = table
        .get("tools")
        .ok_or_else(|| SkillError::Invalid("tools is required".into()))?;
    let tools_array = tools_value
        .as_array()
        .ok_or_else(|| SkillError::Invalid("tools must be an array".into()))?;
    let mut tools = Vec::with_capacity(tools_array.len());
    for entry in tools_array {
        let tool = entry
            .as_str()
            .ok_or_else(|| SkillError::Invalid("tool name must be a string".into()))?;
        if tool.contains('/') || tool.contains("..") || tool.contains('\\') {
            return Err(SkillError::PathEscape);
        }
        if !is_known_tool(tool) {
            return Err(SkillError::UnknownTool(tool.to_string()));
        }
        if tools.iter().any(|existing: &String| existing == tool) {
            return Err(SkillError::Invalid(format!("duplicate tool {tool}")));
        }
        tools.push(tool.to_string());
    }
    Ok(Skill {
        name: name.to_string(),
        version: version.to_string(),
        tools,
    })
}

fn required_str<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    key: &str,
) -> Result<&'a str, SkillError> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| SkillError::Invalid(format!("{key} is required")))
}

fn valid_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 32
        && !text.contains("..")
        && text.bytes().any(|byte| byte.is_ascii_digit())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::symlink;

    fn write_skill(dir: &Path, body: &str) {
        let mut file = fs::File::create(dir.join("SKILL.toml")).expect("create");
        file.write_all(body.as_bytes()).expect("write");
    }

    #[test]
    fn loads_a_skill_inside_the_root() {
        let root = tempfile::tempdir().expect("temp");
        let skill_dir = root.path().join("review");
        fs::create_dir(&skill_dir).expect("dir");
        write_skill(
            &skill_dir,
            "name = \"review\"\nversion = \"0.1.0\"\ntools = [\"repository_search\", \"memory_get\"]\n",
        );
        let skill = load_skill(root.path(), Path::new("review/SKILL.toml")).expect("load");
        assert_eq!(skill.name, "review");
        assert_eq!(skill.version, "0.1.0");
        assert_eq!(skill.tools, vec!["repository_search", "memory_get"]);
    }

    #[test]
    fn rejects_path_escape_unknown_tools_and_symlinks() {
        let root = tempfile::tempdir().expect("temp");
        let err = load_skill(root.path(), Path::new("../SKILL.toml")).unwrap_err();
        assert!(err.to_string().contains("escapes"));
        assert!(load_skill(root.path(), Path::new("/tmp/SKILL.toml")).is_err());
        assert!(load_skill(root.path(), Path::new("review/../../SKILL.toml")).is_err());

        let skill_dir = root.path().join("review");
        fs::create_dir(&skill_dir).expect("dir");
        write_skill(
            &skill_dir,
            "name = \"review\"\nversion = \"0.1.0\"\ntools = [\"shell\"]\n",
        );
        let unknown = load_skill(root.path(), Path::new("review/SKILL.toml")).unwrap_err();
        assert!(unknown.to_string().contains("unknown tool"));

        write_skill(
            &skill_dir,
            "name = \"review\"\nversion = \"0.1.0\"\ntools = [\"../etc/passwd\"]\n",
        );
        assert!(load_skill(root.path(), Path::new("review/SKILL.toml")).is_err());

        write_skill(
            &skill_dir,
            "name = \"review\"\nversion = \"0.1.0\"\ntools = []\nnetwork = true\n",
        );
        let extra = load_skill(root.path(), Path::new("review/SKILL.toml")).unwrap_err();
        assert!(extra.to_string().contains("unknown field"));

        let outside = tempfile::tempdir().expect("outside");
        write_skill(
            outside.path(),
            "name = \"review\"\nversion = \"1\"\ntools = []\n",
        );
        symlink(
            outside.path().join("SKILL.toml"),
            root.path().join("SKILL.toml"),
        )
        .expect("symlink");
        let linked = load_skill(root.path(), Path::new("SKILL.toml")).unwrap_err();
        assert!(linked.to_string().contains("escapes"));

        let link_dir = outside.path().join("linked-dir");
        fs::create_dir(&link_dir).expect("dir");
        write_skill(
            &link_dir,
            "name = \"linked\"\nversion = \"1\"\ntools = []\n",
        );
        symlink(&link_dir, root.path().join("nested")).expect("dir symlink");
        let dir_linked = load_skill(root.path(), Path::new("nested/SKILL.toml")).unwrap_err();
        assert!(dir_linked.to_string().contains("escapes"));
    }

    #[test]
    fn discover_skills_ignores_symlinks_and_loads_valid_entries() {
        let root = tempfile::tempdir().expect("root");
        let a = root.path().join("alpha");
        let b = root.path().join("beta");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        write_skill(
            &a,
            "name = \"alpha\"\nversion = \"0.1.0\"\ntools = [\"memory_list\"]\n",
        );
        write_skill(
            &b,
            "name = \"beta\"\nversion = \"0.2.0\"\ntools = [\"workspace_symbols\"]\n",
        );
        let skills = discover_skills(root.path()).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].name, "beta");
    }
}
