use crate::contain::{contains_symlink, write_new_nofollow};
use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    #[serde(default = "default_max_file_size_kb")]
    pub max_file_size_kb: u64,

    #[serde(default = "default_max_context_tokens")]
    pub default_context_tokens: usize,

    #[serde(default = "default_exclude_secrets")]
    pub exclude_secrets: bool,

    #[serde(default = "default_true")]
    pub index_tests: bool,

    #[serde(default = "default_true")]
    pub index_docs: bool,

    #[serde(default)]
    pub custom_ignores: Vec<String>,
}

fn default_max_file_size_kb() -> u64 {
    2048 // 2MB cap to protect memory & CPU
}

fn default_max_context_tokens() -> usize {
    8000
}

fn default_exclude_secrets() -> bool {
    true
}

fn default_true() -> bool {
    true
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            max_file_size_kb: default_max_file_size_kb(),
            default_context_tokens: default_max_context_tokens(),
            exclude_secrets: default_exclude_secrets(),
            index_tests: true,
            index_docs: true,
            custom_ignores: vec![
                "target/".to_string(),
                "node_modules/".to_string(),
                "__pycache__/".to_string(),
                ".venv/".to_string(),
                "dist/".to_string(),
                "build/".to_string(),
                "out/".to_string(),
            ],
        }
    }
}

impl RepoConfig {
    pub const CONFIG_DIR: &'static str = ".02agent";
    pub const CONFIG_FILE: &'static str = "config.toml";

    pub fn load_or_default<P: AsRef<Path>>(repo_root: P) -> Self {
        let repo_root = repo_root.as_ref();
        let relative = Path::new(Self::CONFIG_DIR).join(Self::CONFIG_FILE);
        if contains_symlink(repo_root, &relative) {
            return Self::default();
        }
        let config_path = repo_root.join(&relative);
        match fs::symlink_metadata(&config_path) {
            Ok(meta) if meta.file_type().is_file() => {}
            _ => return Self::default(),
        }
        match fs::read_to_string(&config_path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|_| Self::default()),
            Err(_) => Self::default(),
        }
    }

    /// Write config only when `.02agent/config.toml` does not already exist as a regular file.
    /// Symlinked directories or files are rejected so init cannot overwrite an external target.
    pub fn save_to_repo<P: AsRef<Path>>(&self, repo_root: P) -> Result<()> {
        let repo_root = repo_root.as_ref();
        let dir = repo_root.join(Self::CONFIG_DIR);
        if contains_symlink(repo_root, Path::new(Self::CONFIG_DIR)) {
            return Err(AgentError::Security(
                "refusing to write config through a symlinked .02agent directory".to_string(),
            ));
        }
        match fs::symlink_metadata(&dir) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AgentError::Security(
                    "refusing to write config through a symlinked .02agent directory".to_string(),
                ));
            }
            Ok(meta) if !meta.is_dir() => {
                return Err(AgentError::Config(
                    ".02agent exists and is not a directory".to_string(),
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&dir)?;
            }
            Err(err) => return Err(err.into()),
        }

        let file_path = dir.join(Self::CONFIG_FILE);
        match fs::symlink_metadata(&file_path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AgentError::Security(
                    "refusing to write through a symlinked config.toml".to_string(),
                ));
            }
            Ok(meta) if meta.is_file() => return Ok(()),
            Ok(_) => {
                return Err(AgentError::Config(
                    "config.toml exists and is not a regular file".to_string(),
                ));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }

        let content = toml::to_string_pretty(self).map_err(AgentError::TomlSer)?;
        write_new_nofollow(&file_path, content.as_bytes())
    }
}
