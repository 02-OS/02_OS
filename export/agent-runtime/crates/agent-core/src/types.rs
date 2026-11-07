use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};

/// Deterministic unique identifier for a repository on the local system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepoId(String);

impl RepoId {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let canonical = match path.as_ref().canonicalize() {
            Ok(c) => c,
            Err(_) => path.as_ref().to_path_buf(),
        };
        let lossy = canonical.to_string_lossy();
        let mut hasher = Sha256::new();
        hasher.update(lossy.as_bytes());
        let hash = hex::encode(hasher.finalize());
        // Use first 16 chars for concise yet collision-free ID
        Self(hash[..16].to_string())
    }

    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Metadata and status of a Git repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub id: RepoId,
    pub name: String,
    pub root_path: PathBuf,
    pub head_commit: Option<String>,
    pub branch: Option<String>,
    pub is_clean: bool,
    pub modified_count: usize,
    pub untracked_count: usize,
}

/// Category of a repository file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    Source,
    Test,
    Documentation,
    Manifest,
    Configuration,
    Data,
    Unknown,
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source => write!(f, "source"),
            Self::Test => write!(f, "test"),
            Self::Documentation => write!(f, "documentation"),
            Self::Manifest => write!(f, "manifest"),
            Self::Configuration => write!(f, "configuration"),
            Self::Data => write!(f, "data"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Programming or markup language classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    C,
    Cpp,
    Bash,
    Dart,
    Go,
    Html,
    Css,
    Json,
    Toml,
    Yaml,
    Markdown,
    Sql,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Self::Rust,
            "py" | "pyi" => Self::Python,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "c" | "h" => Self::C,
            "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "hh" => Self::Cpp,
            "sh" | "bash" | "zsh" => Self::Bash,
            "dart" => Self::Dart,
            "go" => Self::Go,
            "html" | "htm" => Self::Html,
            "css" | "scss" | "sass" | "less" => Self::Css,
            "json" => Self::Json,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "md" | "markdown" => Self::Markdown,
            "sql" => Self::Sql,
            _ => Self::Unknown,
        }
    }

    /// Parse a persisted language name (`"rust"`, `"python"`, …).
    /// Falls back to extension matching so either form round-trips.
    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "rust" => Self::Rust,
            "python" => Self::Python,
            "typescript" => Self::TypeScript,
            "javascript" => Self::JavaScript,
            "c" => Self::C,
            "cpp" => Self::Cpp,
            "bash" => Self::Bash,
            "dart" => Self::Dart,
            "go" => Self::Go,
            "html" => Self::Html,
            "css" => Self::Css,
            "json" => Self::Json,
            "toml" => Self::Toml,
            "yaml" => Self::Yaml,
            "markdown" => Self::Markdown,
            "sql" => Self::Sql,
            "unknown" => Self::Unknown,
            _ => Self::from_extension(name),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Bash => "bash",
            Self::Dart => "dart",
            Self::Go => "go",
            Self::Html => "html",
            Self::Css => "css",
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Markdown => "markdown",
            Self::Sql => "sql",
            Self::Unknown => "unknown",
        }
    }
}

/// Metadata for an indexed file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    pub relative_path: String,
    pub file_hash: String,
    pub git_blob_id: Option<String>,
    pub size_bytes: u64,
    pub language: Language,
    pub kind: FileKind,
    pub is_test: bool,
    pub is_doc: bool,
    pub indexed_at: DateTime<Utc>,
}

/// Category of a code symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Module,
    Constant,
    Variable,
    TypeAlias,
    Unknown,
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Function => write!(f, "function"),
            Self::Method => write!(f, "method"),
            Self::Class => write!(f, "class"),
            Self::Struct => write!(f, "struct"),
            Self::Enum => write!(f, "enum"),
            Self::Interface => write!(f, "interface"),
            Self::Trait => write!(f, "trait"),
            Self::Module => write!(f, "module"),
            Self::Constant => write!(f, "constant"),
            Self::Variable => write!(f, "variable"),
            Self::TypeAlias => write!(f, "type_alias"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl SymbolKind {
    pub fn from_str_kind(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "function" => Self::Function,
            "method" => Self::Method,
            "class" => Self::Class,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "interface" => Self::Interface,
            "trait" => Self::Trait,
            "module" => Self::Module,
            "constant" => Self::Constant,
            "variable" => Self::Variable,
            "type_alias" => Self::TypeAlias,
            _ => Self::Unknown,
        }
    }
}

/// Extracted symbol node in the evidence graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub fingerprint: String,
}

/// Kind of relationship between symbols/files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceKind {
    Calls,
    Imports,
    Implements,
    Extends,
    Instantiates,
    Tests,
    Unknown,
}

impl fmt::Display for ReferenceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Calls => write!(f, "calls"),
            Self::Imports => write!(f, "imports"),
            Self::Implements => write!(f, "implements"),
            Self::Extends => write!(f, "extends"),
            Self::Instantiates => write!(f, "instantiates"),
            Self::Tests => write!(f, "tests"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl ReferenceKind {
    pub fn from_str_kind(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "calls" => Self::Calls,
            "imports" => Self::Imports,
            "implements" => Self::Implements,
            "extends" => Self::Extends,
            "instantiates" => Self::Instantiates,
            "tests" => Self::Tests,
            _ => Self::Unknown,
        }
    }
}

/// Directed edge in the symbol reference graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolReference {
    pub source_file: String,
    pub source_symbol_name: Option<String>,
    pub target_name: String,
    pub target_symbol_id: Option<String>,
    pub kind: ReferenceKind,
    pub line_number: usize,
}

/// Category of an evidence-backed repository memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryKind {
    ArchitecturalFact,
    Convention,
    DependencyConstraint,
    SecurityConstraint,
    PerformanceConstraint,
    DiscoveredContext,
}

impl fmt::Display for MemoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArchitecturalFact => write!(f, "architectural_fact"),
            Self::Convention => write!(f, "convention"),
            Self::DependencyConstraint => write!(f, "dependency_constraint"),
            Self::SecurityConstraint => write!(f, "security_constraint"),
            Self::PerformanceConstraint => write!(f, "performance_constraint"),
            Self::DiscoveredContext => write!(f, "discovered_context"),
        }
    }
}

impl MemoryKind {
    pub fn from_str_kind(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "architectural_fact" => Self::ArchitecturalFact,
            "convention" => Self::Convention,
            "dependency_constraint" => Self::DependencyConstraint,
            "security_constraint" => Self::SecurityConstraint,
            "performance_constraint" => Self::PerformanceConstraint,
            _ => Self::DiscoveredContext,
        }
    }
}

/// Status of memory freshness against current Git HEAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryStatus {
    Fresh,
    Degraded,
    Stale,
    Invalidated,
}

impl fmt::Display for MemoryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fresh => write!(f, "fresh"),
            Self::Degraded => write!(f, "degraded"),
            Self::Stale => write!(f, "stale"),
            Self::Invalidated => write!(f, "invalidated"),
        }
    }
}

impl MemoryStatus {
    pub fn from_str_status(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "fresh" => Self::Fresh,
            "degraded" => Self::Degraded,
            "stale" => Self::Stale,
            _ => Self::Invalidated,
        }
    }
}

/// Concrete evidence supporting a repository claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub file: String,
    pub symbols: Vec<String>,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

/// Evidence-backed repository memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceMemory {
    pub id: String,
    pub claim: String,
    pub kind: MemoryKind,
    pub evidence: Vec<EvidenceItem>,
    pub valid_at: String,
    pub confidence: f64,
    pub status: MemoryStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Secret and credential patterns to exclude from analysis.
pub struct SecretPattern;

impl SecretPattern {
    pub fn is_secret(path: &Path) -> bool {
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_lowercase(),
            None => return false,
        };

        // Exact matches
        let exact_secrets = [
            ".env",
            ".env.local",
            ".env.production",
            ".env.development",
            ".env.test",
            "id_rsa",
            "id_ed25519",
            "id_dsa",
            "id_ecdsa",
            "credentials",
            "credentials.json",
            "service_account.json",
            "secret.key",
            "secrets.json",
            "secrets.toml",
            "secrets.yaml",
            "secrets.yml",
        ];

        if exact_secrets.contains(&file_name.as_str()) {
            return true;
        }

        // Prefix check for .env variants
        if file_name.starts_with(".env.") {
            return true;
        }

        // Extension matches
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            let secret_exts = ["pem", "key", "p12", "pfx", "kdbx", "keystore", "jks"];
            if secret_exts.contains(&ext_lower.as_str()) {
                return true;
            }
        }

        // Substring matches for credential filenames. Source files such as
        // `secret_rotation.go` stay in the catalog; their bodies are redacted.
        if !is_source_file_name(&file_name)
            && !file_name.ends_with(".md")
            && !file_name.ends_with(".txt")
            && (file_name.contains("secret")
                || file_name.contains("credential")
                || file_name.contains("private_key"))
        {
            return true;
        }

        false
    }

    /// Replace documented credential patterns with `[REDACTED]`.
    pub fn redact(content: &str) -> String {
        let mut redacted = content.to_string();
        for pattern in secret_patterns() {
            let next = pattern.replace_all(&redacted, "[REDACTED]");
            if next != redacted {
                redacted = next.into_owned();
            }
        }
        redacted
    }
}

fn is_source_file_name(file_name: &str) -> bool {
    const SOURCE_EXTS: &[&str] = &[
        "rs", "go", "py", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "js", "jsx", "ts", "tsx",
        "dart", "java", "kt", "swift", "rb", "php", "cs", "scala", "sh", "bash", "zsh",
    ];
    SOURCE_EXTS
        .iter()
        .any(|ext| file_name.ends_with(&format!(".{ext}")))
}

fn secret_patterns() -> &'static [regex::Regex] {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
            r"\bAKIA[0-9A-Z]{16}\b",
            r"\bghp_[A-Za-z0-9_]{36}\b",
            r"\bgithub_pat_[A-Za-z0-9_]{82}\b",
            r"\bxox[baprs]-[0-9A-Za-z-]{10,48}",
            r#"(?i)(api[_-]?key|secret|token)\s*[:=]\s*["'][A-Za-z0-9_\-]{20,}["']"#,
        ]
        .into_iter()
        .map(|pattern| regex::Regex::new(pattern).expect("secret pattern"))
        .collect()
    })
}

/// Live ISO runtime detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveEnvironmentInfo {
    pub is_live: bool,
    pub live_overlay_detected: bool,
    pub details: String,
}

impl LiveEnvironmentInfo {
    pub fn detect() -> Self {
        let has_bootmnt = Path::new("/run/archiso/bootmnt").exists();
        let has_archiso_airootfs = Path::new("/run/archiso/airootfs").exists();
        // The account name `live` is not evidence of the live image. sudo keeps
        // the caller's USER, and an installed account can also be named live.
        let is_live = has_bootmnt || has_archiso_airootfs;

        let details = if is_live {
            "Running inside 02_OS Live ISO session. Repository indices created in home directory are stored in volatile RAM overlayfs unless mounted from persistent media.".to_string()
        } else {
            "Running on installed 02_OS system with persistent storage.".to_string()
        };

        Self {
            is_live,
            live_overlay_detected: has_bootmnt || has_archiso_airootfs,
            details,
        }
    }
}
