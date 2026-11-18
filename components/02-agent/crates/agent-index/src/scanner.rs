use agent_core::{
    config::RepoConfig,
    contain::{hash_indexed_file_canon, read_indexed_bytes_canon},
    types::{FileKind, IndexedFile, Language, SecretPattern},
};
use chrono::Utc;
use ignore::WalkBuilder;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
enum CompiledIgnore {
    Component(String),
    PathPrefix { exact: String, prefix: String },
}

fn compile_ignores(patterns: &[String]) -> Vec<CompiledIgnore> {
    let mut rules = Vec::new();
    for pattern in patterns {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(name) = trimmed.strip_suffix('/') {
            if name.is_empty() {
                continue;
            }
            let name = name.trim_start_matches("./");
            if !name.contains('/') {
                rules.push(CompiledIgnore::Component(name.to_string()));
            } else {
                rules.push(CompiledIgnore::PathPrefix {
                    exact: name.to_string(),
                    prefix: format!("{name}/"),
                });
            }
        } else {
            rules.push(CompiledIgnore::PathPrefix {
                exact: trimmed.to_string(),
                prefix: format!("{trimmed}/"),
            });
        }
    }
    rules
}

fn matches_compiled_ignores(rel: &Path, rules: &[CompiledIgnore]) -> bool {
    if rules.is_empty() || rel.as_os_str().is_empty() {
        return false;
    }
    let mut parts = Vec::new();
    for component in rel.components() {
        if let Component::Normal(name) = component {
            if let Some(s) = name.to_str() {
                parts.push(s);
            }
        }
    }
    if parts.is_empty() {
        return false;
    }
    let joined = parts.join("/");
    for rule in rules {
        match rule {
            CompiledIgnore::Component(name) => {
                if parts.iter().any(|part| *part == name) {
                    return true;
                }
            }
            CompiledIgnore::PathPrefix { exact, prefix } => {
                if joined == *exact || joined.starts_with(prefix) {
                    return true;
                }
            }
        }
    }
    false
}

/// Directory ignore patterns (`build/`) match a path component, not a string prefix.
pub fn custom_ignored(rel: &Path, patterns: &[String]) -> bool {
    matches_compiled_ignores(rel, &compile_ignores(patterns))
}

pub struct RepoScanner {
    root: PathBuf,
    root_canon: PathBuf,
    config: RepoConfig,
    ignore_rules: Arc<Vec<CompiledIgnore>>,
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub file: IndexedFile,
    pub absolute_path: PathBuf,
    pub content: Option<String>,
}

impl RepoScanner {
    /// Hash the exact scan inputs, independent of mtimes and enumeration order.
    pub fn scan_fingerprint(scanned: &[ScannedFile], config: &RepoConfig) -> String {
        let pairs: Vec<(&str, &str)> = scanned
            .iter()
            .map(|item| {
                (
                    item.file.relative_path.as_str(),
                    item.file.file_hash.as_str(),
                )
            })
            .collect();
        Self::hash_fingerprint_pairs(pairs, config)
    }

    fn hash_fingerprint_pairs<'a>(
        mut pairs: Vec<(&'a str, &'a str)>,
        config: &RepoConfig,
    ) -> String {
        let mut hash = Sha256::new();
        hash.update(b"02-index-snapshot-v1\0");
        // RepoConfig has deterministic field ordering and contains all selection knobs.
        hash.update(serde_json::to_vec(config).expect("RepoConfig is serializable"));
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        for (rel_path, file_hash) in pairs {
            hash.update(rel_path.as_bytes());
            hash.update([0]);
            hash.update(file_hash.as_bytes());
            hash.update([0]);
        }
        hex::encode(hash.finalize())
    }

    pub fn fingerprint(&self) -> String {
        let max_bytes = self.config.max_file_size_kb * 1024;
        let mut entries: Vec<(String, String)> = Vec::new();
        for (path, rel_path, _, _, _) in self.walk_candidate_files() {
            let Ok(hash) = hash_indexed_file_canon(&self.root, &self.root_canon, &path, max_bytes)
            else {
                continue;
            };
            entries.push((rel_path, hash));
        }
        let pairs: Vec<(&str, &str)> = entries
            .iter()
            .map(|(p, h)| (p.as_str(), h.as_str()))
            .collect();
        Self::hash_fingerprint_pairs(pairs, &self.config)
    }

    pub fn new<P: AsRef<Path>>(root: P, config: RepoConfig) -> Self {
        let root_buf = root.as_ref().to_path_buf();
        let root_canon = std::fs::canonicalize(&root_buf).unwrap_or_else(|_| root_buf.clone());
        let ignore_rules = Arc::new(compile_ignores(&config.custom_ignores));
        Self {
            root: root_buf,
            root_canon,
            config,
            ignore_rules,
        }
    }

    fn walk_candidate_files(&self) -> Vec<(PathBuf, String, Language, FileKind, u64)> {
        let mut builder = WalkBuilder::new(&self.root);
        builder.hidden(false); // allow inspecting dotfiles like .gitignore
        builder.git_ignore(true);
        builder.git_global(true);
        builder.git_exclude(true);
        builder.follow_links(false);
        let ignore_root = self.root.clone();
        let rules = Arc::clone(&self.ignore_rules);
        builder.filter_entry(move |entry| match entry.path().strip_prefix(&ignore_root) {
            Ok(rel) if rel.as_os_str().is_empty() => true,
            Ok(rel) => !matches_compiled_ignores(rel, &rules),
            Err(_) => false,
        });

        let agent_ignore = self.root.join(".02agentignore");
        if agent_ignore.exists() {
            builder.add_custom_ignore_filename(".02agentignore");
        }

        let mut files = Vec::new();
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
            if file_type.is_symlink() || !file_type.is_file() {
                continue;
            }
            if path.components().any(|c| c.as_os_str() == ".git") {
                continue;
            }
            if self.config.exclude_secrets && SecretPattern::is_secret(path) {
                continue;
            }
            let rel = match path.strip_prefix(&self.root) {
                Ok(rel) => rel,
                Err(_) => continue,
            };
            let rel_path = match rel.to_str() {
                Some(text) => text.to_string(),
                None => continue,
            };
            if matches_compiled_ignores(rel, &self.ignore_rules) {
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
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files.push((path.to_path_buf(), rel_path, language, kind, size));
        }
        files
    }

    /// Walk repository and gather indexable files respecting gitignore, 02agentignore, and security rules.
    pub fn scan(&self) -> Vec<ScannedFile> {
        let max_bytes = self.config.max_file_size_kb * 1024;
        let mut scanned = Vec::new();

        for (path, rel_path, language, kind, _) in self.walk_candidate_files() {
            let bytes =
                match read_indexed_bytes_canon(&self.root, &self.root_canon, &path, max_bytes) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                };
            let size = bytes.len() as u64;
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let hash = hex::encode(hasher.finalize());
            let content = std::str::from_utf8(&bytes).ok().map(|text| {
                match SecretPattern::redact_cow(text) {
                    Cow::Borrowed(s) => s.to_string(),
                    Cow::Owned(s) => s,
                }
            });
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
                absolute_path: path,
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

        // Tests. Match directory components at the root and nested (`tests/common.rs`
        // has no leading slash, so a `/tests/` substring misses it).
        if path_has_dir_component(&lower, &["tests", "test", "spec", "specs"])
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

/// True when any directory component of `rel_path` is one of `names`.
/// The final component is the file name and is ignored.
fn path_has_dir_component(rel_path: &str, names: &[&str]) -> bool {
    let mut parts = Path::new(rel_path).components().peekable();
    while let Some(component) = parts.next() {
        if parts.peek().is_none() {
            break;
        }
        let Component::Normal(name) = component else {
            continue;
        };
        let lower = name.to_string_lossy().to_lowercase();
        if names.iter().any(|candidate| *candidate == lower) {
            return true;
        }
    }
    false
}
