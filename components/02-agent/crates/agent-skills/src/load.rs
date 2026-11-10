//! Read one `SKILL.toml` from inside a skill root.
//!
//! The relative path is `SKILL.toml` or `<name>/SKILL.toml`. Parent
//! components, absolute paths, and symlinks are rejected before the file is read.

use std::fs;
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
}

pub fn load_skill(root: &Path, relative: &Path) -> Result<Skill, SkillError> {
    let parts = skill_parts(relative)?;
    if !root.is_dir() {
        return Err(SkillError::Missing);
    }
    let root_meta = fs::symlink_metadata(root).map_err(|_| SkillError::Missing)?;
    if root_meta.file_type().is_symlink() {
        return Err(SkillError::PathEscape);
    }
    let mut cursor = root.to_path_buf();
    for part in &parts {
        cursor.push(part);
        let meta = match fs::symlink_metadata(&cursor) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(SkillError::Missing);
            }
            Err(_) => return Err(SkillError::NotRegular),
        };
        if meta.file_type().is_symlink() {
            return Err(SkillError::PathEscape);
        }
    }
    let meta = fs::symlink_metadata(&cursor).map_err(|_| SkillError::Missing)?;
    if !meta.is_file() {
        return Err(SkillError::NotRegular);
    }
    if meta.len() > MAX_SKILL_BYTES {
        return Err(SkillError::TooLarge);
    }
    let text = fs::read_to_string(&cursor)
        .map_err(|_| SkillError::Invalid("unreadable skill file".into()))?;
    parse_document(&text)
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
    }
}
