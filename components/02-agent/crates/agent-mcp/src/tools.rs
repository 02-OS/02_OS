use crate::protocol::{ToolCallResult, ToolDefinition};
use agent_context::{estimate_tokens, types::EvidencePackage, ContextCompiler, ScoredFact};
use agent_core::{
    config::RepoConfig,
    contain::validate_evidence_file,
    paths::StoragePaths,
    types::{EvidenceMemory, MemoryKind, MemoryStatus, RepoId, SecretPattern},
    AgentError, Result,
};
use agent_git::{discover_workspace, GitRepo};
use agent_index::IndexDatabase;
use agent_memory::{FactStore, MemoryStore, MemoryVerifier, StalenessEngine};
use agent_skills::{SessionPolicy, SharedContextScope};
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn list_tools() -> Vec<ToolDefinition> {
    let mut tools = vec![
        ToolDefinition {
            name: "add".to_string(),
            description: "Ingest working tree, docs, git history, a session note, or pasted text into a dataset.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Comma-separated sources: worktree, docs, history, session, text. Default: worktree,docs" },
                    "text": { "type": "string", "description": "Required for session and text sources" },
                    "dataset": { "type": "string", "description": "Dataset name" },
                    "full": { "type": "boolean", "description": "Re-read every file" }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "cognify".to_string(),
            description: "Deterministically build symbol, chunk, and commit edges for a dataset.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dataset": { "type": "string", "description": "Dataset name" },
                    "full": { "type": "boolean", "description": "Re-extract every source file" }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "graph_export".to_string(),
            description: "Export the knowledge graph as JSON, DOT, or Mermaid.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "format": { "type": "string", "description": "json, dot, or mermaid" },
                    "dataset": { "type": "string", "description": "Dataset name" }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "repository_status".to_string(),
            description: "Get current Git repository status, branch, HEAD commit, clean/dirty state, and index stats.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "repository_search".to_string(),
            description: "Search repository code and documentation using SQLite FTS5 BM25 full-text search.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "integer", "description": "Maximum results (default: 10)" }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "find_symbol".to_string(),
            description: "Look up code symbols (functions, structs, classes, variables) by name or partial pattern.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Symbol name or pattern" }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "symbol_references".to_string(),
            description: "Find callers, callees, and file imports associated with a symbol.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Target symbol name" }
                },
                "required": ["symbol"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "symbol_dependencies".to_string(),
            description: "Get the dependency graph (callers, callees, and relationships) for a symbol.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Target symbol name" }
                },
                "required": ["symbol"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "git_history".to_string(),
            description: "Get recent Git commit history with commit IDs, authors, timestamps, and summaries.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Number of commits (default: 10)" }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "git_diff_context".to_string(),
            description: "Get current working tree Git diff status and modified line hunks.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "related_tests".to_string(),
            description: "Find test files and test functions that exercise or reference a target symbol.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Symbol name" }
                },
                "required": ["symbol"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "repository_context".to_string(),
            description: "Compile a task-focused, Git-aware, token-budgeted evidence package for coding agents.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Description of the task to be performed" },
                    "token_budget": { "type": "integer", "description": "Maximum token budget (default: 8000)" }
                },
                "required": ["task"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "memory_get".to_string(),
            description: "Retrieve an architectural fact or verified memory by its ID.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Memory ID" }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "memory_write".to_string(),
            description: "Record or update an architectural memory connected to code evidence.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Unique memory ID" },
                    "claim": { "type": "string", "description": "Architectural fact or rule" },
                    "file": { "type": "string", "description": "Evidence file path" },
                    "symbols": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Evidence symbols"
                    }
                },
                "required": ["id", "claim", "file"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "memory_status".to_string(),
            description: "Get a freshness and staleness summary of all repository memories.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "memory_list".to_string(),
            description: "List repository evidence memories with optional status filter and pagination cursor.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "description": "Optional status filter: fresh, degraded, stale, invalidated" },
                    "limit": { "type": "integer", "description": "Maximum results (default: 50, max: 200)" },
                    "cursor": { "type": "string", "description": "Pagination offset cursor" }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "memory_verify".to_string(),
            description: "Re-evaluate evidence freshness and staleness against the repository working tree.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Optional memory ID to inspect" }
                },
                "additionalProperties": false
            }),
        },
    ];
    tools.extend(crate::nav_tools::definitions());

    for tool in &mut tools {
        if let Some(props) = tool
            .input_schema
            .get_mut("properties")
            .and_then(|p| p.as_object_mut())
        {
            props.insert(
                "repo_path".to_string(),
                json!({
                    "type": "string",
                    "description": "Optional absolute path of the Git repository. When omitted, the daemon uses the MCP session workspace or the only watched repository."
                }),
            );
        }
    }

    tools
}

pub(crate) fn scoped_memory_store(
    repo_id: &RepoId,
    policy: Option<&SessionPolicy>,
) -> Result<MemoryStore> {
    let Some(policy) = policy else {
        return MemoryStore::for_repo(repo_id);
    };
    if !policy.allows_repository(repo_id.as_str()) {
        return Err(AgentError::Security(
            "context access denied for scope".into(),
        ));
    }
    let dir = StoragePaths::repo_dir(repo_id)?;
    match &policy.scope {
        SharedContextScope::Private => Ok(MemoryStore::new(dir.join(format!(
            "memories.private.{}.jsonl",
            policy.agent_id.as_str()
        )))),
        SharedContextScope::Session(id) => Ok(MemoryStore::new(
            dir.join(format!("memories.session.{id}.jsonl")),
        )),
        SharedContextScope::Repository(_) => MemoryStore::for_repo(repo_id),
    }
}

pub(crate) fn scoped_fact_store(
    repo_id: &RepoId,
    policy: Option<&SessionPolicy>,
) -> Result<FactStore> {
    let Some(policy) = policy else {
        return FactStore::for_repo(repo_id);
    };
    if !policy.allows_repository(repo_id.as_str()) {
        return Err(AgentError::Security(
            "context access denied for scope".into(),
        ));
    }
    let dir = StoragePaths::repo_dir(repo_id)?;
    match &policy.scope {
        SharedContextScope::Private => {
            FactStore::open(dir.join(format!("facts.private.{}.sqlite", policy.agent_id.as_str())))
        }
        SharedContextScope::Session(id) => {
            FactStore::open(dir.join(format!("facts.session.{id}.sqlite")))
        }
        SharedContextScope::Repository(_) => FactStore::for_repo(repo_id),
    }
}

pub(crate) fn apply_policy_to_package(
    repo_id: &RepoId,
    policy: Option<&SessionPolicy>,
    package: &mut EvidencePackage,
) {
    let Some(policy) = policy else {
        return;
    };
    if matches!(policy.scope, SharedContextScope::Repository(_)) {
        return;
    }
    // Private and session scopes isolate prior memories and facts away from shared stores.
    let memories = scoped_memory_store(repo_id, Some(policy))
        .and_then(|store| store.load_all())
        .unwrap_or_default();
    let mut verified = Vec::new();
    let mut diagnostic = Vec::new();
    for mem in memories {
        if mem.status == MemoryStatus::Fresh && mem.confidence > 0.0 {
            verified.push(mem);
        } else {
            diagnostic.push(mem);
        }
    }
    package.verified_memories = verified;
    package.diagnostic_memories = diagnostic;

    let facts = scoped_fact_store(repo_id, Some(policy))
        .and_then(|store| {
            let now = Utc::now();
            Ok(store
                .list(repo_id.as_str())?
                .into_iter()
                .filter_map(|f| {
                    let eff = store.effective_confidence(&f, now);
                    (eff > 0.05).then_some(ScoredFact {
                        id: f.id,
                        claim: f.claim,
                        relative_path: f.relative_path,
                        symbol: f.symbol,
                        effective_confidence: eff,
                    })
                })
                .collect::<Vec<_>>())
        })
        .unwrap_or_default();
    package.relevant_facts = facts;
}

fn canonicalize_worktree(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn resolve_confined_repo(
    default_root: &Path,
    allowed_roots: &[PathBuf],
    arguments: &Value,
) -> std::result::Result<GitRepo, String> {
    let base_git = GitRepo::open(default_root).map_err(|e| format!("Git error: {e}"))?;
    let override_str = arguments
        .get("repo_path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(override_path) = override_str else {
        return Ok(base_git);
    };
    let candidate = Path::new(override_path);
    if !candidate.is_absolute() {
        return Err("repo_path must be an absolute path".to_string());
    }
    let cand_git = GitRepo::open(candidate).map_err(|e| format!("Git error: {e}"))?;
    let cand_canon = canonicalize_worktree(cand_git.root_path());

    if cand_canon == canonicalize_worktree(base_git.root_path()) {
        return Ok(cand_git);
    }
    for root in allowed_roots {
        if let Ok(git) = GitRepo::open(root) {
            if canonicalize_worktree(git.root_path()) == cand_canon {
                return Ok(cand_git);
            }
        } else if canonicalize_worktree(root) == cand_canon {
            return Ok(cand_git);
        }
    }
    if let Ok(members) = discover_workspace(base_git.root_path()) {
        for member in members {
            if canonicalize_worktree(&member.root) == cand_canon {
                return Ok(cand_git);
            }
        }
    }
    Err("repo_path is outside allowed workspace repositories".to_string())
}

pub fn call_tool(repo_root: &Path, name: &str, arguments: &Value) -> ToolCallResult {
    call_tool_with_policy(
        repo_root,
        std::slice::from_ref(&repo_root.to_path_buf()),
        None,
        name,
        arguments,
    )
}

pub fn call_tool_with_policy(
    repo_root: &Path,
    allowed_roots: &[PathBuf],
    policy: Option<&SessionPolicy>,
    name: &str,
    arguments: &Value,
) -> ToolCallResult {
    if matches!(name, "approve" | "approval" | "record_approval") {
        return ToolCallResult::error("mcp client cannot approve its own call".to_string());
    }
    if let Some(policy) = policy {
        if let Err(err) = policy.allows_tool(name) {
            return ToolCallResult::error(err.to_string());
        }
    }

    let git_repo = match resolve_confined_repo(repo_root, allowed_roots, arguments) {
        Ok(r) => r,
        Err(msg) => return ToolCallResult::error(msg),
    };
    let repo_root = git_repo.root_path();

    let repo_info = match git_repo.info() {
        Ok(info) => info,
        Err(e) => return ToolCallResult::error(format!("Repo info error: {}", e)),
    };
    let repo_id = &repo_info.id;
    if let Some(policy) = policy {
        if !policy.allows_repository(repo_id.as_str()) {
            return ToolCallResult::error("context access denied for repository scope".to_string());
        }
    }

    let db_path = match StoragePaths::repo_db_path(repo_id) {
        Ok(p) => p,
        Err(e) => return ToolCallResult::error(format!("Path error: {}", e)),
    };

    let db = match IndexDatabase::open(&db_path) {
        Ok(d) => d,
        Err(e) => return ToolCallResult::error(format!("Database error: {}", e)),
    };

    let result = dispatch_tool(
        name, repo_root, &git_repo, &db, repo_id, &repo_info, arguments, policy,
    );

    // Record tool invocation and estimated returned tokens in SQLite v5 telemetry (#153).
    if db.note_repo_presence(&repo_info).is_ok() {
        let canonical = serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string());
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        let args_hash = hex::encode(hasher.finalize());
        let tokens: u64 = result
            .content
            .iter()
            .map(|c| estimate_tokens(&c.text) as u64)
            .sum();
        let actor_kind = if policy.is_some() {
            "agt"
        } else {
            "mcp-client"
        };
        let actor_name = policy.map(|p| p.agent_id.as_str()).unwrap_or("default");
        let _ = db.record_completed_invocation(
            repo_id,
            actor_kind,
            actor_name,
            name,
            &args_hash,
            &canonical,
            result.is_error,
            result.is_error.then_some("tool_error"),
            Some(tokens),
        );
    }

    result
}

#[allow(clippy::too_many_arguments)]
fn dispatch_tool(
    name: &str,
    repo_root: &Path,
    git_repo: &GitRepo,
    db: &IndexDatabase,
    repo_id: &RepoId,
    repo_info: &agent_core::RepoInfo,
    arguments: &Value,
    policy: Option<&SessionPolicy>,
) -> ToolCallResult {
    if matches!(name, "add" | "cognify" | "graph_export") {
        return crate::graph_tools::call(repo_root, name, arguments);
    }
    if let Some(result) = crate::nav_tools::call_with_policy(
        name, repo_root, git_repo, db, repo_id, repo_info, arguments, policy,
    ) {
        return result;
    }

    match name {
        "repository_status" => {
            let stats = match db.get_stats(repo_id) {
                Ok(s) => s,
                Err(e) => return ToolCallResult::error(format!("Stats error: {}", e)),
            };

            let res = json!({
                "repository": repo_info.name,
                "repo_id": repo_info.id.as_str(),
                "root_path": repo_info.root_path,
                "branch": repo_info.branch,
                "head_commit": repo_info.head_commit,
                "is_clean": repo_info.is_clean,
                "modified_files": repo_info.modified_count,
                "untracked_files": repo_info.untracked_count,
                "total_indexed_files": stats.total_files,
                "source_files": stats.source_files,
                "test_files": stats.test_files,
                "doc_files": stats.doc_files,
                "total_symbols": stats.total_symbols,
                "total_references": stats.total_references,
                "last_indexed_at": stats.last_indexed_at,
            });
            ToolCallResult::text(res.to_string())
        }

        "repository_search" => {
            let query = arguments["query"].as_str().unwrap_or("");
            let limit = arguments["limit"].as_u64().unwrap_or(10) as usize;
            match db.fts_needs_rebuild() {
                Ok(true) => {
                    let res = json!({
                        "hits": [],
                        "warnings": [{
                            "code": "fts_unbuilt",
                            "message": "full-text index is empty; run 02 index"
                        }]
                    });
                    ToolCallResult::text(res.to_string())
                }
                Ok(false) => match db.search_content(query, limit) {
                    Ok(results) => ToolCallResult::text(json!(results).to_string()),
                    Err(e) => ToolCallResult::error(format!("Search error: {}", e)),
                },
                Err(e) => ToolCallResult::error(format!("Search error: {}", e)),
            }
        }

        "find_symbol" => {
            let symbol_name = arguments["name"].as_str().unwrap_or("");
            let mut symbols = db
                .find_symbols_by_name(repo_id, symbol_name)
                .unwrap_or_default();
            if symbols.is_empty() {
                symbols = db
                    .search_symbols(repo_id, symbol_name, 20)
                    .unwrap_or_default();
            }
            ToolCallResult::text(json!(symbols).to_string())
        }

        "symbol_references" | "symbol_dependencies" => {
            let symbol = arguments["symbol"].as_str().unwrap_or("");
            match db.find_symbol_deps(repo_id, symbol) {
                Ok(deps) => ToolCallResult::text(json!(deps).to_string()),
                Err(e) => ToolCallResult::error(format!("Symbol deps error: {}", e)),
            }
        }

        "git_history" => {
            let limit = arguments["limit"].as_u64().unwrap_or(10) as usize;
            match git_repo.get_recent_commits(limit) {
                Ok(mut commits) => {
                    for commit in &mut commits {
                        commit.summary = SecretPattern::redact(&commit.summary);
                        commit.author = SecretPattern::redact(&commit.author);
                    }
                    let text = SecretPattern::redact(&json!(commits).to_string());
                    ToolCallResult::text(text)
                }
                Err(e) => ToolCallResult::error(format!("Git history error: {}", e)),
            }
        }

        "git_diff_context" => {
            let diff_files = git_repo.get_diff_files().unwrap_or_default();
            let diff_hunks = git_repo.get_diff_hunks().unwrap_or_default();
            let res = json!({
                "modified_files": diff_files,
                "hunks": diff_hunks,
            });
            ToolCallResult::text(res.to_string())
        }

        "related_tests" => {
            let symbol = arguments["symbol"].as_str().unwrap_or("");
            match db.find_tests_for_symbol(repo_id, symbol) {
                Ok(tests) => ToolCallResult::text(json!(tests).to_string()),
                Err(e) => ToolCallResult::error(format!("Related tests error: {}", e)),
            }
        }

        "repository_context" => {
            let task = arguments["task"].as_str().unwrap_or("");
            let budget = arguments["token_budget"]
                .as_u64()
                .unwrap_or(8000)
                .min(100_000) as usize;

            match ContextCompiler::compile(repo_root, task, budget) {
                Ok(mut package) => {
                    apply_policy_to_package(repo_id, policy, &mut package);
                    ToolCallResult::text(package.serialized_payload())
                }
                Err(e) => ToolCallResult::error(format!("Context compiler error: {}", e)),
            }
        }

        "memory_get" => {
            let id = arguments["id"].as_str().unwrap_or("");
            let store = match scoped_memory_store(repo_id, policy) {
                Ok(s) => s,
                Err(e) => return ToolCallResult::error(format!("Store error: {}", e)),
            };

            match store.get(id) {
                Ok(Some(mem)) => ToolCallResult::text(json!(mem).to_string()),
                Ok(None) => ToolCallResult::error(format!("Memory '{}' not found", id)),
                Err(e) => ToolCallResult::error(format!("Get memory error: {}", e)),
            }
        }

        "memory_write" => {
            let id = arguments["id"].as_str().unwrap_or("");
            let claim = arguments["claim"].as_str().unwrap_or("");
            let file = arguments["file"].as_str().unwrap_or("");
            let symbols: Vec<String> = arguments["symbols"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let store = match scoped_memory_store(repo_id, policy) {
                Ok(s) => s,
                Err(e) => return ToolCallResult::error(format!("Store error: {}", e)),
            };

            let max_bytes = RepoConfig::load_or_default(repo_root).max_file_size_kb * 1024;
            if let Err(err) = validate_evidence_file(repo_root, Path::new(file), max_bytes) {
                return ToolCallResult::error(format!("evidence path rejected: {err}"));
            }

            let commit_id = repo_info
                .head_commit
                .clone()
                .unwrap_or_else(|| "none".to_string());
            let evidence =
                match MemoryVerifier::capture_evidence(repo_root, file, symbols, &commit_id) {
                    Ok(evidence) => evidence,
                    Err(err) => return ToolCallResult::error(format!("evidence rejected: {err}")),
                };
            let mem = EvidenceMemory {
                id: id.to_string(),
                claim: SecretPattern::redact(claim),
                kind: MemoryKind::ArchitecturalFact,
                evidence: vec![evidence],
                valid_at: commit_id,
                confidence: 1.0,
                status: MemoryStatus::Fresh,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            match store.save(&mem) {
                Ok(_) => {
                    ToolCallResult::text(json!({ "status": "saved", "memory": mem }).to_string())
                }
                Err(e) => ToolCallResult::error(format!("Failed to save memory: {}", e)),
            }
        }

        "memory_status" => {
            let store = match scoped_memory_store(repo_id, policy) {
                Ok(s) => s,
                Err(e) => return ToolCallResult::error(format!("Store error: {}", e)),
            };

            let memories = match store.load_all() {
                Ok(list) => list,
                Err(e) => return ToolCallResult::error(format!("Load memories error: {}", e)),
            };
            let fresh = memories
                .iter()
                .filter(|m| m.status == MemoryStatus::Fresh)
                .count();
            let degraded = memories
                .iter()
                .filter(|m| m.status == MemoryStatus::Degraded)
                .count();
            let stale = memories
                .iter()
                .filter(|m| {
                    m.status == MemoryStatus::Stale || m.status == MemoryStatus::Invalidated
                })
                .count();

            let res = json!({
                "total_memories": memories.len(),
                "fresh": fresh,
                "degraded": degraded,
                "stale": stale,
                "memories": memories,
            });
            ToolCallResult::text(res.to_string())
        }

        "memory_list" => {
            let store = match scoped_memory_store(repo_id, policy) {
                Ok(s) => s,
                Err(e) => return ToolCallResult::error(format!("Store error: {}", e)),
            };
            let memories = match store.load_all() {
                Ok(list) => list,
                Err(e) => return ToolCallResult::error(format!("Load memories error: {}", e)),
            };
            let status_filter = arguments
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_ascii_lowercase());
            let filtered: Vec<EvidenceMemory> = memories
                .into_iter()
                .filter(|m| match status_filter.as_deref() {
                    Some(wanted) if !wanted.is_empty() => m.status.to_string() == wanted,
                    _ => true,
                })
                .collect();
            let offset = arguments
                .get("cursor")
                .and_then(|v| v.as_str())
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(50)
                .clamp(1, 200) as usize;
            let end = offset.saturating_add(limit).min(filtered.len());
            let truncated = end < filtered.len();
            let next_cursor = truncated.then(|| end.to_string());
            let page = if offset < filtered.len() {
                filtered[offset..end].to_vec()
            } else {
                Vec::new()
            };
            ToolCallResult::text(
                json!({
                    "memories": page,
                    "truncated": truncated,
                    "next_cursor": next_cursor
                })
                .to_string(),
            )
        }

        "memory_verify" => {
            let store = match scoped_memory_store(repo_id, policy) {
                Ok(s) => s,
                Err(e) => return ToolCallResult::error(format!("Store error: {}", e)),
            };
            let report = match StalenessEngine::evaluate(repo_root, repo_id, git_repo, db, &store) {
                Ok(report) => report,
                Err(e) => return ToolCallResult::error(format!("Verify failed: {}", e)),
            };
            let target_id = arguments
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            if let Some(id) = target_id {
                match store.get(id) {
                    Ok(Some(mem)) => ToolCallResult::text(
                        json!({
                            "memory": mem,
                            "fresh": report.fresh_count,
                            "degraded": report.degraded_count,
                            "stale": report.stale_count,
                        })
                        .to_string(),
                    ),
                    Ok(None) => ToolCallResult::error(format!("Memory '{}' not found", id)),
                    Err(e) => ToolCallResult::error(format!("Verify lookup error: {}", e)),
                }
            } else {
                ToolCallResult::text(json!(report).to_string())
            }
        }

        unknown => ToolCallResult::error(format!("Unknown tool: '{}'", unknown)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn mcp_cannot_approve_itself() {
        let result = call_tool(Path::new("/tmp"), "approve", &json!({}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("cannot approve"));
    }
}
