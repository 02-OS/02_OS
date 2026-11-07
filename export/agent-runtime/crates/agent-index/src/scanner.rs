use agent_core::{
    config::RepoConfig,
    contain::read_indexed_bytes,
    types::{FileKind, IndexedFile, Language, SecretPattern},
};
use chrono::Utc;
use ignore::WalkBuilder;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

/// Directory ignore patterns (`build/`) match a path component, not a string prefix.
pub fn custom_ignored(rel: &Path, patterns: &[String]) -> bool {
    if rel.as_os_str().is_empty() {
        return false;
    }
    let components: Vec<&str> = rel
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    if components.is_empty() {
        return false;
    }
    let joined = components.join("/");
    for pattern in patterns {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(name) = trimmed.strip_suffix('/') {
            if name.is_empty() {
                continue;
            }
            if components.contains(&name) {
                return true;
            }
        } else if joined == trimmed || joined.starts_with(&format!("{trimmed}/")) {
            return true;
        }
    }
    false
}

pub struct RepoScanner {
    root: PathBuf,
    config: RepoConfig,
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub file: IndexedFile,
    pub absolute_path: PathBuf,
    pub content: Option<String>,
}

impl RepoScanner {
    pub fn new<P: AsRef<Path>>(root: P, config: RepoConfig) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            config,
        }
    }

    /// Walk repository and gather indexable files respecting gitignore, 02agentignore, and security rules.
    pub fn scan(&self) -> Vec<ScannedFile> {
        let mut builder = WalkBuilder::new(&self.root);
        builder.hidden(false); // allow inspecting dotfiles like .gitignore
        builder.git_ignore(true);
        builder.git_global(true);
        builder.git_exclude(true);
        builder.follow_links(false);
        let ignore_root = self.root.clone();
        let ignore_patterns = self.config.custom_ignores.clone();
        builder.filter_entry(move |entry| match entry.path().strip_prefix(&ignore_root) {
            Ok(rel) if rel.as_os_str().is_empty() => true,
            Ok(rel) => !custom_ignored(rel, &ignore_patterns),
            Err(_) => false,
        });

        // Add custom .02agentignore if it exists
        let agent_ignore = self.root.join(".02agentignore");
        if agent_ignore.exists() {
            builder.add_custom_ignore_filename(".02agentignore");
        }

        let max_bytes = self.config.max_file_size_kb * 1024;
        let mut scanned = Vec::new();

        for result in builder.build() {
            let entry = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            let file_type = match entry.file_type() {
                Some(file_type) => file_type,
                None => continue,
            };
            // Never follow a symlink. The real file is indexed under its own name.
            if file_type.is_symlink() || !file_type.is_file() {
                continue;
            }

            // Exclude anything inside .git
            if path.components().any(|c| c.as_os_str() == ".git") {
                continue;
            }

            // Exclude secrets by filename before opening the body.
            if self.config.exclude_secrets && SecretPattern::is_secret(path) {
                continue;
            }

            // Relative path from repo root
            let rel = match path.strip_prefix(&self.root) {
                Ok(rel) => rel,
                Err(_) => continue,
            };
            let rel_path = match rel.to_str() {
                Some(text) => text.to_string(),
                None => continue,
            };
            if custom_ignored(rel, &self.config.custom_ignores) {
                continue;
            }

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let language = Language::from_extension(ext);
            let kind = Self::classify_kind(&rel_path, language);
            if (kind == FileKind::Test && !self.config.index_tests)
                || (kind == FileKind::Documentation && !self.config.index_docs)
            {
                continue;
            }

            let bytes = match read_indexed_bytes(&self.root, path, max_bytes) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            let size = bytes.len() as u64;
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let hash = hex::encode(hasher.finalize());
            let content = std::str::from_utf8(&bytes).ok().map(SecretPattern::redact);
            let is_test = kind == FileKind::Test;
            let is_doc = kind == FileKind::Documentation;

            let file = IndexedFile {
                relative_path: rel_path,
                file_hash: hash,
                git_blob_id: None, // Filled via Git if available
                size_bytes: size,
                language,
                kind,
                is_test,
                is_doc,
                indexed_at: Utc::now(),
            };

            scanned.push(ScannedFile {
                file,
                absolute_path: path.to_path_buf(),
                content,
            });
        }

        scanned
    }

    fn classify_kind(rel_path: &str, language: Language) -> FileKind {
        let lower = rel_path.to_lowercase();
        let file_name = Path::new(rel_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Tests
        if lower.contains("/tests/")
            || lower.contains("/test/")
            || lower.contains("/spec/")
            || lower.contains("/specs/")
            || file_name.starts_with("test_")
            || file_name.ends_with("_test.rs")
            || file_name.ends_with("_test.py")
            || file_name.ends_with("_test.go")
            || file_name.ends_with(".test.ts")
            || file_name.ends_with(".test.js")
            || file_name.ends_with(".spec.ts")
            || file_name.ends_with(".spec.js")
        {
            return FileKind::Test;
        }

        // Documentation
        if lower.starts_with("docs/")
            || lower.contains("/docs/")
            || file_name.ends_with(".md")
            || file_name.ends_with(".markdown")
            || file_name.ends_with(".rst")
            || file_name.ends_with(".adoc")
            || file_name == "readme"
            || file_name.starts_with("readme.")
            || file_name == "license"
            || file_name.starts_with("license.")
        {
            return FileKind::Documentation;
        }

        // Package manifests
        let manifests = [
            "cargo.toml",
            "package.json",
            "requirements.txt",
            "pyproject.toml",
            "go.mod",
            "pom.xml",
            "cmakelists.txt",
            "packages.x86_64",
            "pubspec.yaml",
            "gemfile",
        ];
        if manifests.contains(&file_name.as_str()) {
            return FileKind::Manifest;
        }

        // Configuration
        if file_name.ends_with(".conf")
            || file_name.ends_with(".ini")
            || file_name.ends_with(".desktop")
            || file_name.ends_with(".service")
            || file_name.ends_with(".socket")
            || file_name == ".editorconfig"
            || file_name == ".gitignore"
            || (matches!(language, Language::Toml | Language::Yaml | Language::Json)
                && !manifests.contains(&file_name.as_str()))
        {
            return FileKind::Configuration;
        }

        // Source
        if matches!(
            language,
            Language::Rust
                | Language::Python
                | Language::TypeScript
                | Language::JavaScript
                | Language::C
                | Language::Cpp
                | Language::Bash
                | Language::Dart
                | Language::Go
                | Language::Sql
        ) {
            return FileKind::Source;
        }

        FileKind::Unknown
    }
}
