use crate::protocol::{ToolCallResult, ToolDefinition};
use agent_context::ContextCompiler;
use agent_core::{
    config::RepoConfig,
    contain::validate_evidence_file,
    paths::StoragePaths,
    types::{EvidenceItem, EvidenceMemory, MemoryKind, MemoryStatus},
};
use agent_git::GitRepo;
use agent_index::IndexDatabase;
use agent_memory::MemoryStore;
use chrono::Utc;
use serde_json::{json, Value};
use std::path::Path;

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
    ];

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

pub fn call_tool(repo_root: &Path, name: &str, arguments: &Value) -> ToolCallResult {
    let override_root = arguments
        .get("repo_path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let repo_root = override_root.as_deref().unwrap_or(repo_root);

    let git_repo = match GitRepo::open(repo_root) {
        Ok(r) => r,
        Err(e) => return ToolCallResult::error(format!("Git error: {}", e)),
    };

    let repo_info = match git_repo.info() {
        Ok(info) => info,
        Err(e) => return ToolCallResult::error(format!("Repo info error: {}", e)),
    };
    let repo_id = &repo_info.id;

    let db_path = match StoragePaths::repo_db_path(repo_id) {
        Ok(p) => p,
        Err(e) => return ToolCallResult::error(format!("Path error: {}", e)),
    };

    let db = match IndexDatabase::open(&db_path) {
        Ok(d) => d,
        Err(e) => return ToolCallResult::error(format!("Database error: {}", e)),
    };

    if matches!(name, "add" | "cognify" | "graph_export") {
        return crate::graph_tools::call(repo_root, name, arguments);
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

            match db.search_content(query, limit) {
                Ok(results) => ToolCallResult::text(json!(results).to_string()),
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
                Ok(commits) => ToolCallResult::text(json!(commits).to_string()),
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
            let budget = arguments["token_budget"].as_u64().unwrap_or(8000) as usize;

            match ContextCompiler::compile(repo_root, task, budget) {
                Ok(package) => {
                    let markdown = package.to_markdown();
                    let res = json!({
                        "package": package,
                        "markdown": markdown,
                    });
                    ToolCallResult::text(res.to_string())
                }
                Err(e) => ToolCallResult::error(format!("Context compiler error: {}", e)),
            }
        }

        "memory_get" => {
            let id = arguments["id"].as_str().unwrap_or("");
            let store = match MemoryStore::for_repo(repo_id) {
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

            let store = match MemoryStore::for_repo(repo_id) {
                Ok(s) => s,
                Err(e) => return ToolCallResult::error(format!("Store error: {}", e)),
            };

            let max_bytes = RepoConfig::load_or_default(repo_root).max_file_size_kb * 1024;
            if let Err(err) = validate_evidence_file(repo_root, Path::new(file), max_bytes) {
                return ToolCallResult::error(format!("evidence path rejected: {err}"));
            }

            let commit_id = repo_info.head_commit.unwrap_or_else(|| "none".to_string());
            let mem = EvidenceMemory {
                id: id.to_string(),
                claim: claim.to_string(),
                kind: MemoryKind::ArchitecturalFact,
                evidence: vec![EvidenceItem {
                    file: file.to_string(),
                    symbols,
                    commit: commit_id.clone(),
                    fingerprint: None,
                }],
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
            let store = match MemoryStore::for_repo(repo_id) {
                Ok(s) => s,
                Err(e) => return ToolCallResult::error(format!("Store error: {}", e)),
            };

            let memories = store.load_all().unwrap_or_default();
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

        unknown => ToolCallResult::error(format!("Unknown tool: '{}'", unknown)),
    }
}
