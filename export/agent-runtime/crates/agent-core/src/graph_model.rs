use crate::error::{AgentError, Result};
use crate::types::RepoId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Where a datapoint's bytes came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceKind {
    Worktree,
    GitHistory,
    Document,
    SessionNote,
    PastedText,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worktree => "worktree",
            Self::GitHistory => "history",
            Self::Document => "docs",
            Self::SessionNote => "session",
            Self::PastedText => "text",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "worktree" | "working-tree" | "tree" => Some(Self::Worktree),
            "history" | "git" | "commits" => Some(Self::GitHistory),
            "docs" | "doc" | "document" | "documents" => Some(Self::Document),
            "session" | "note" => Some(Self::SessionNote),
            "text" | "paste" | "pasted" | "pasted-text" => Some(Self::PastedText),
            _ => None,
        }
    }

    pub fn parse_list(spec: &str) -> Result<Vec<Self>> {
        let mut kinds = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let kind = Self::parse(part).ok_or_else(|| {
                AgentError::Config(format!(
                    "unknown source '{part}' (expected worktree, docs, history, session, text)"
                ))
            })?;
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        if kinds.is_empty() {
            return Err(AgentError::Config(
                "at least one source is required".to_string(),
            ));
        }
        Ok(kinds)
    }
}

/// Typed edge stored in the knowledge graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Calls,
    Imports,
    Tests,
    TouchedInCommit,
    Owns,
    DecidedBy,
    Supersedes,
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Calls => "calls",
            Self::Imports => "imports",
            Self::Tests => "tests",
            Self::TouchedInCommit => "touched-in-commit",
            Self::Owns => "owns",
            Self::DecidedBy => "decided-by",
            Self::Supersedes => "supersedes",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "calls" => Some(Self::Calls),
            "imports" => Some(Self::Imports),
            "tests" => Some(Self::Tests),
            "touched-in-commit" | "touched_in_commit" => Some(Self::TouchedInCommit),
            "owns" => Some(Self::Owns),
            "decided-by" | "decided_by" => Some(Self::DecidedBy),
            "supersedes" => Some(Self::Supersedes),
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Calls,
            Self::Imports,
            Self::Tests,
            Self::TouchedInCommit,
            Self::Owns,
            Self::DecidedBy,
            Self::Supersedes,
        ]
    }
}

/// Provenance carried on every datapoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source_kind: SourceKind,
    pub commit_id: Option<String>,
    pub path: String,
    pub start_byte: Option<u64>,
    pub end_byte: Option<u64>,
    pub symbol_fingerprint: Option<String>,
}

/// Versioned, content-hashed record. `id` is the content hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub id: String,
    pub repo_id: RepoId,
    pub dataset_id: String,
    pub version: u32,
    pub kind: String,
    pub content_hash: String,
    pub content: String,
    pub provenance: Provenance,
    pub confidence: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// SHA-256 of the dataset, record kind, path, and content bytes.
pub fn content_id(dataset_id: &str, kind: &str, path: &str, content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dataset_id.as_bytes());
    hasher.update([0]);
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(path.as_bytes());
    hasher.update([0]);
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Named collection of datapoints for one repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    pub id: String,
    pub repo_id: RepoId,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl Dataset {
    pub fn named(repo_id: &RepoId, name: &str, created_at: DateTime<Utc>) -> Self {
        Self {
            id: format!("dataset:{}:{name}", repo_id.as_str()),
            repo_id: repo_id.clone(),
            name: name.to_string(),
            created_at,
        }
    }

    pub fn default_for(repo_id: &RepoId) -> Self {
        Self::named(repo_id, "default", Utc::now())
    }
}

/// Free tag over graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSet {
    pub id: String,
    pub repo_id: RepoId,
    pub name: String,
}

impl NodeSet {
    pub fn named(repo_id: &RepoId, name: &str) -> Self {
        Self {
            id: format!("nodeset:{}:{name}", repo_id.as_str()),
            repo_id: repo_id.clone(),
            name: name.to_string(),
        }
    }
}

/// Ephemeral agent-session note that can be linked into the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub repo_id: RepoId,
    pub dataset_id: String,
    pub label: Option<String>,
    pub ephemeral: bool,
    pub created_at: DateTime<Utc>,
}

/// Optional TOML ontology. Missing file means every built-in kind is allowed.
#[derive(Debug, Clone)]
pub struct Ontology {
    pub node_kinds: Option<Vec<String>>,
    pub edge_kinds: Vec<EdgeKind>,
}

#[derive(Debug, Deserialize)]
struct OntologyFile {
    #[serde(default)]
    node_kinds: Option<Vec<String>>,
    #[serde(default)]
    edge_kinds: Option<Vec<String>>,
}

impl Default for Ontology {
    fn default() -> Self {
        Self {
            node_kinds: None,
            edge_kinds: EdgeKind::all().to_vec(),
        }
    }
}

impl Ontology {
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(".02agent").join("ontology.toml");
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)?;
        let parsed: OntologyFile = toml::from_str(&text)?;
        let edge_kinds = match parsed.edge_kinds {
            None => EdgeKind::all().to_vec(),
            Some(names) => {
                let mut kinds = Vec::new();
                for name in names {
                    let kind = EdgeKind::parse(&name).ok_or_else(|| {
                        AgentError::Config(format!("unknown ontology edge kind '{name}'"))
                    })?;
                    if !kinds.contains(&kind) {
                        kinds.push(kind);
                    }
                }
                kinds
            }
        };
        Ok(Self {
            node_kinds: parsed.node_kinds,
            edge_kinds,
        })
    }

    pub fn allows_edge(&self, kind: EdgeKind) -> bool {
        self.edge_kinds.contains(&kind)
    }

    pub fn allows_node(&self, kind: &str) -> bool {
        match &self.node_kinds {
            None => true,
            Some(kinds) => kinds.iter().any(|item| item == kind),
        }
    }
}

/// One directed fact in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub src_id: String,
    pub dst_id: String,
    pub kind: EdgeKind,
    #[serde(default)]
    pub payload: Option<String>,
}
