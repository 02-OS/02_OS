use crate::protocol::{ToolCallResult, ToolDefinition};
use crate::tools::{apply_policy_to_package, scoped_fact_store};
use agent_context::types::EvidencePackage;
use agent_context::{allocate, estimate_tokens, BudgetItem, ContextCompiler};
use agent_core::{truncate_chars, ReferenceKind, RepoId, RepoInfo, SecretPattern};
use agent_git::{discover_workspace, GitRepo};
use agent_index::IndexDatabase;
use agent_memory::{facts_from_hunks, facts_from_symbols, project_facts, DEFAULT_HALF_LIFE_DAYS};
use agent_skills::SessionPolicy;
use chrono::Utc;
use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Tools added for navigation, tests, git, dependencies, ranking, and facts.
pub fn definitions() -> Vec<ToolDefinition> {
    let specs = [
        (
            "list_files",
            "List indexed files.",
            r#"{"limit":{"type":"integer"},"cursor":{"type":"string"}}"#,
        ),
        (
            "file_outline",
            "List symbols declared in one file.",
            r#"{"path":{"type":"string"}}"#,
        ),
        (
            "goto_definition",
            "Find the declaration of a symbol.",
            r#"{"name":{"type":"string"}}"#,
        ),
        (
            "enclosing_symbol",
            "Find the symbol that contains a line.",
            r#"{"path":{"type":"string"},"line":{"type":"integer"}}"#,
        ),
        (
            "search_symbols",
            "Search symbol names.",
            r#"{"query":{"type":"string"},"limit":{"type":"integer"}}"#,
        ),
        (
            "references_in_file",
            "List references whose source is a file.",
            r#"{"path":{"type":"string"},"limit":{"type":"integer"},"cursor":{"type":"string"}}"#,
        ),
        (
            "list_tests",
            "List indexed test files.",
            r#"{"limit":{"type":"integer"},"cursor":{"type":"string"}}"#,
        ),
        (
            "tests_covering",
            "Find tests that cover a symbol.",
            r#"{"symbol":{"type":"string"}}"#,
        ),
        (
            "run_tests",
            "Run one cargo test filter inside the repository. The filter is passed after '--'.",
            r#"{"filter":{"type":"string"}}"#,
        ),
        (
            "git_status",
            "Show branch, HEAD, and whether the worktree is clean.",
            "{}",
        ),
        (
            "git_show",
            "Show one commit's summary and the paths it touched.",
            r#"{"commit":{"type":"string"}}"#,
        ),
        (
            "git_file_log",
            "List recent commits that touch a file.",
            r#"{"path":{"type":"string"},"limit":{"type":"integer"}}"#,
        ),
        ("git_branches", "List local branch names.", "{}"),
        (
            "git_commit_files",
            "List paths touched by one commit.",
            r#"{"commit":{"type":"string"}}"#,
        ),
        (
            "imports_of_file",
            "List import edges declared in a file.",
            r#"{"path":{"type":"string"},"limit":{"type":"integer"},"cursor":{"type":"string"}}"#,
        ),
        (
            "importers_of_file",
            "List files that import a file's stem or path.",
            r#"{"path":{"type":"string"},"limit":{"type":"integer"},"cursor":{"type":"string"}}"#,
        ),
        (
            "callers_of_symbol",
            "List call sites of a symbol.",
            r#"{"symbol":{"type":"string"}}"#,
        ),
        (
            "callees_of_symbol",
            "List calls made by a symbol.",
            r#"{"symbol":{"type":"string"}}"#,
        ),
        (
            "dependency_summary",
            "Count reference kinds in the catalog.",
            "{}",
        ),
        (
            "context_dump",
            "Compile the redacted evidence package as Markdown.",
            r#"{"task":{"type":"string"},"budget":{"type":"integer"}}"#,
        ),
        (
            "rank_files",
            "Rank indexed files for a task.",
            r#"{"task":{"type":"string"}}"#,
        ),
        (
            "score_breakdown",
            "Return the five-signal score for each ranked file.",
            r#"{"task":{"type":"string"}}"#,
        ),
        (
            "budget_preview",
            "Choose files that fit a token budget by score.",
            r#"{"task":{"type":"string"},"budget":{"type":"integer"}}"#,
        ),
        (
            "evidence_package",
            "Return the redacted evidence package JSON.",
            r#"{"task":{"type":"string"},"budget":{"type":"integer"}}"#,
        ),
        (
            "workspace_repos",
            "List this repository and immediate child repositories.",
            "{}",
        ),
        (
            "workspace_search",
            "Search each indexed repository in the workspace.",
            r#"{"query":{"type":"string"},"limit":{"type":"integer"}}"#,
        ),
        (
            "workspace_symbols",
            "Search or browse indexed symbols across workspace repositories.",
            r#"{"query":{"type":"string"},"limit":{"type":"integer"},"cursor":{"type":"string"}}"#,
        ),
        (
            "fact_extract",
            "Store git-anchored facts from the current diff and the symbol table.",
            "{}",
        ),
        (
            "fact_list",
            "List stored facts and their decayed confidence.",
            "{}",
        ),
        (
            "fact_decay",
            "Materialize confidence decay.",
            r#"{"half_life_days":{"type":"number"}}"#,
        ),
    ];
    specs
        .into_iter()
        .map(|(name, description, properties)| {
            let properties: Value = serde_json::from_str(properties).unwrap_or_else(|_| json!({}));
            let required = required_keys(name);
            ToolDefinition {
                name: name.to_string(),
                description: description.to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false
                }),
            }
        })
        .collect()
}

fn required_keys(name: &str) -> Vec<&'static str> {
    match name {
        "file_outline" | "references_in_file" | "imports_of_file" | "importers_of_file"
        | "git_file_log" => vec!["path"],
        "goto_definition" => vec!["name"],
        "enclosing_symbol" => vec!["path", "line"],
        "search_symbols" | "workspace_search" => vec!["query"],
        "tests_covering" | "callers_of_symbol" | "callees_of_symbol" => vec!["symbol"],
        "run_tests" => vec!["filter"],
        "git_show" | "git_commit_files" => vec!["commit"],
        "context_dump" | "rank_files" | "score_breakdown" | "budget_preview"
        | "evidence_package" => vec!["task"],
        _ => Vec::new(),
    }
}

pub fn call(
    name: &str,
    repo_root: &Path,
    git: &GitRepo,
    db: &IndexDatabase,
    repo_id: &RepoId,
    repo_info: &RepoInfo,
    arguments: &Value,
) -> Option<ToolCallResult> {
    call_with_policy(
        name, repo_root, git, db, repo_id, repo_info, arguments, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn call_with_policy(
    name: &str,
    repo_root: &Path,
    git: &GitRepo,
    db: &IndexDatabase,
    repo_id: &RepoId,
    repo_info: &RepoInfo,
    arguments: &Value,
    policy: Option<&SessionPolicy>,
) -> Option<ToolCallResult> {
    let result = match name {
        "list_files" => list_files(db, repo_id, arguments),
        "file_outline" => file_outline(db, repo_id, arguments),
        "goto_definition" => goto_definition(db, repo_id, arguments),
        "enclosing_symbol" => enclosing_symbol(db, repo_id, arguments),
        "search_symbols" => search_symbols(db, repo_id, arguments),
        "references_in_file" => references_in_file(db, repo_id, arguments),
        "list_tests" => list_tests(db, repo_id, arguments),
        "tests_covering" => tests_covering(db, repo_id, arguments),
        "run_tests" => run_tests(repo_root, arguments),
        "git_status" => git_status(repo_info),
        "git_show" => git_show(git, arguments),
        "git_file_log" => git_file_log(git, arguments),
        "git_branches" => git_branches(git),
        "git_commit_files" => git_commit_files(git, arguments),
        "imports_of_file" => imports_of_file(db, repo_id, arguments),
        "importers_of_file" => importers_of_file(db, repo_id, arguments),
        "callers_of_symbol" => callers_of_symbol(db, repo_id, arguments),
        "callees_of_symbol" => callees_of_symbol(db, repo_id, arguments),
        "dependency_summary" => dependency_summary(db, repo_id),
        "context_dump" => context_dump(repo_root, repo_id, arguments, policy),
        "rank_files" => rank_files(repo_root, arguments),
        "score_breakdown" => rank_files(repo_root, arguments),
        "budget_preview" => budget_preview(repo_root, arguments),
        "evidence_package" => evidence_package(repo_root, repo_id, arguments, policy),
        "workspace_repos" => workspace_repos(repo_root),
        "workspace_search" => workspace_search(repo_root, arguments, policy),
        "workspace_symbols" => workspace_symbols(repo_root, arguments, policy),
        "fact_extract" => fact_extract(git, db, repo_id, repo_info, policy),
        "fact_list" => fact_list(repo_id, arguments, policy),
        "fact_decay" => fact_decay(db, repo_info, arguments, policy),
        _ => return None,
    };
    Some(result)
}

fn ok(value: Value) -> ToolCallResult {
    ToolCallResult::text(value.to_string())
}

fn fail(error: impl std::fmt::Display) -> ToolCallResult {
    ToolCallResult::error(error.to_string())
}

fn required<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolCallResult> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| fail(format!("{key} is required")))
}

fn limit_of(arguments: &Value, default: usize, max: usize) -> usize {
    arguments
        .get("limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(default as u64)
        .clamp(1, max as u64) as usize
}

fn paginate_vec<T>(
    items: Vec<T>,
    arguments: &Value,
    default_limit: usize,
    max_limit: usize,
) -> (Vec<T>, bool, Option<String>) {
    let limit = limit_of(arguments, default_limit, max_limit);
    let offset = arguments
        .get("cursor")
        .and_then(|value| value.as_str())
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if offset >= items.len() {
        return (Vec::new(), false, None);
    }
    let end = offset.saturating_add(limit).min(items.len());
    let truncated = end < items.len();
    let next_cursor = truncated.then(|| end.to_string());
    let page = items.into_iter().skip(offset).take(end - offset).collect();
    (page, truncated, next_cursor)
}

fn list_files(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    match db.list_files(repo_id) {
        Ok(files) => {
            let entries: Vec<Value> = files
                .into_iter()
                .map(|file| {
                    json!({
                        "path": file.relative_path,
                        "language": format!("{:?}", file.language).to_lowercase(),
                        "kind": file.kind.to_string(),
                        "is_test": file.is_test
                    })
                })
                .collect();
            let (page, truncated, next_cursor) = paginate_vec(entries, arguments, 200, 200);
            ok(json!({ "files": page, "truncated": truncated, "next_cursor": next_cursor }))
        }
        Err(err) => fail(err),
    }
}

fn file_outline(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    let path = match required(arguments, "path") {
        Ok(path) => path,
        Err(result) => return result,
    };
    match db.find_symbols_in_file(repo_id, path) {
        Ok(symbols) => ok(json!({ "path": path, "symbols": symbols })),
        Err(err) => fail(err),
    }
}

fn goto_definition(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    let name = match required(arguments, "name") {
        Ok(name) => name,
        Err(result) => return result,
    };
    let exact = db.find_symbols_by_name(repo_id, name).unwrap_or_default();
    let symbols = if exact.is_empty() {
        db.search_symbols(repo_id, name, 10).unwrap_or_default()
    } else {
        exact
    };
    ok(json!({ "symbols": symbols.into_iter().take(20).collect::<Vec<_>>() }))
}

fn enclosing_symbol(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    let path = match required(arguments, "path") {
        Ok(path) => path,
        Err(result) => return result,
    };
    let Some(line) = arguments.get("line").and_then(|value| value.as_u64()) else {
        return fail("line is required");
    };
    let symbols = match db.find_symbols_in_file(repo_id, path) {
        Ok(symbols) => symbols,
        Err(err) => return fail(err),
    };
    let mut best = None;
    for symbol in symbols {
        if symbol.start_line as u64 <= line && line <= symbol.end_line as u64 {
            let span = symbol.end_line.saturating_sub(symbol.start_line);
            let replace = best.as_ref().is_none_or(|(_, best_span)| span < *best_span);
            if replace {
                best = Some((symbol, span));
            }
        }
    }
    ok(json!({ "symbol": best.map(|(symbol, _)| symbol) }))
}

fn search_symbols(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    let query = match required(arguments, "query") {
        Ok(query) => query,
        Err(result) => return result,
    };
    match db.search_symbols(repo_id, query, limit_of(arguments, 20, 50)) {
        Ok(symbols) => ok(json!({ "symbols": symbols })),
        Err(err) => fail(err),
    }
}

fn references_in_file(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    let path = match required(arguments, "path") {
        Ok(path) => path,
        Err(result) => return result,
    };
    match db.list_references_in_file(repo_id, path, None) {
        Ok(references) => {
            let (page, truncated, next_cursor) = paginate_vec(references, arguments, 200, 200);
            ok(json!({ "references": page, "truncated": truncated, "next_cursor": next_cursor }))
        }
        Err(err) => fail(err),
    }
}

fn list_tests(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    match db.list_files(repo_id) {
        Ok(files) => {
            let test_files: Vec<String> = files
                .into_iter()
                .filter(|file| file.is_test)
                .map(|file| file.relative_path)
                .collect();
            let (page, truncated, next_cursor) = paginate_vec(test_files, arguments, 200, 200);
            ok(json!({ "tests": page, "truncated": truncated, "next_cursor": next_cursor }))
        }
        Err(err) => fail(err),
    }
}

fn tests_covering(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    let symbol = match required(arguments, "symbol") {
        Ok(symbol) => symbol,
        Err(result) => return result,
    };
    match db.find_tests_for_symbol(repo_id, symbol) {
        Ok(tests) => ok(json!({ "tests": tests })),
        Err(err) => fail(err),
    }
}

fn run_tests(repo_root: &Path, arguments: &Value) -> ToolCallResult {
    let filter = match required(arguments, "filter") {
        Ok(filter) => filter,
        Err(result) => return result,
    };
    if !valid_test_filter(filter) {
        return fail(
            "test filter must be 1-80 non-leading-dash characters of letters, numbers, '_', ':', '-', or '.'",
        );
    }
    if !repo_root.join("Cargo.toml").is_file() {
        return fail("run_tests needs a Cargo.toml at the repository root");
    }
    let mut command = Command::new("cargo");
    command
        .arg("test")
        .arg("--offline")
        .arg("--locked")
        .arg("--")
        .arg(filter)
        .arg("--test-threads=1")
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Put cargo in its own process group so timeouts kill spawned rustc/test descendants too (#130).
    // SAFETY: `setpgid(0, 0)` is async-signal-safe in pre_exec before exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => return fail(format!("could not start cargo: {err}")),
    };
    let pgid = child.id() as libc::pid_t;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = thread::spawn(move || read_capped(stdout, 8_000));
    let stderr_task = thread::spawn(move || read_capped(stderr, 4_000));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > Duration::from_secs(20) => {
                // SAFETY: `pgid` matches the child group created via `setpgid(0, 0)`.
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_task.join();
                let _ = stderr_task.join();
                return fail("cargo test exceeded 20 seconds");
            }
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(err) => {
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_task.join();
                let _ = stderr_task.join();
                return fail(err);
            }
        }
    };
    let stdout = stdout_task.join().unwrap_or_default();
    let stderr = stderr_task.join().unwrap_or_default();
    ok(json!({
        "status": status.code(),
        "success": status.success(),
        "stdout": SecretPattern::redact(&stdout),
        "stderr": SecretPattern::redact(&stderr)
    }))
}

pub fn valid_test_filter(filter: &str) -> bool {
    !filter.is_empty()
        && !filter.starts_with('-')
        && filter.len() <= 80
        && filter
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '-' | '.'))
}

fn read_capped(pipe: Option<impl std::io::Read>, cap: usize) -> String {
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < cap {
        match std::io::Read::read(&mut pipe, &mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                let take = read.min(cap - buf.len());
                buf.extend_from_slice(&chunk[..take]);
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn git_status(repo_info: &RepoInfo) -> ToolCallResult {
    ok(json!({
        "branch": repo_info.branch,
        "head": repo_info.head_commit,
        "clean": repo_info.is_clean,
        "modified": repo_info.modified_count,
        "untracked": repo_info.untracked_count
    }))
}

fn git_show(git: &GitRepo, arguments: &Value) -> ToolCallResult {
    let commit = match required(arguments, "commit") {
        Ok(commit) => commit,
        Err(result) => return result,
    };
    let info = match git.commit_info(commit) {
        Ok(info) => info,
        Err(err) => return fail(err),
    };
    let files = git.paths_touched_by_commit(&info.id).unwrap_or_default();
    ok(json!({
        "id": info.id,
        "summary": SecretPattern::redact(&info.summary),
        "author": SecretPattern::redact(&info.author),
        "timestamp": info.timestamp,
        "files": files
    }))
}

fn git_file_log(git: &GitRepo, arguments: &Value) -> ToolCallResult {
    let path = match required(arguments, "path") {
        Ok(path) => path,
        Err(result) => return result,
    };
    match git.commits_touching(path, limit_of(arguments, 15, 40)) {
        Ok(commits) => ok(json!({
            "commits": commits.into_iter().map(|commit| json!({
                "id": commit.id,
                "summary": SecretPattern::redact(&commit.summary),
                "timestamp": commit.timestamp
            })).collect::<Vec<_>>()
        })),
        Err(err) => fail(err),
    }
}

fn git_branches(git: &GitRepo) -> ToolCallResult {
    match git.list_local_branches() {
        Ok(branches) => ok(json!({ "branches": branches })),
        Err(err) => fail(err),
    }
}

fn git_commit_files(git: &GitRepo, arguments: &Value) -> ToolCallResult {
    let commit = match required(arguments, "commit") {
        Ok(commit) => commit,
        Err(result) => return result,
    };
    match git.paths_touched_by_commit(commit) {
        Ok(files) => ok(json!({ "files": files })),
        Err(err) => fail(err),
    }
}

fn imports_of_file(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    edges_for_file(db, repo_id, arguments, true)
}

fn importers_of_file(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    edges_for_file(db, repo_id, arguments, false)
}

fn edges_for_file(
    db: &IndexDatabase,
    repo_id: &RepoId,
    arguments: &Value,
    outgoing: bool,
) -> ToolCallResult {
    let path = match required(arguments, "path") {
        Ok(path) => path,
        Err(result) => return result,
    };
    let stem = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(path);
    let edges = if outgoing {
        db.list_references_in_file(repo_id, path, Some(ReferenceKind::Imports))
    } else {
        db.list_importers_of_file(repo_id, stem).map(|refs| {
            refs.into_iter()
                .filter(|reference| reference.target_name == path || reference.target_name == stem)
                .collect()
        })
    };
    match edges {
        Ok(matches) => {
            let (page, truncated, next_cursor) = paginate_vec(matches, arguments, 200, 200);
            ok(json!({ "imports": page, "truncated": truncated, "next_cursor": next_cursor }))
        }
        Err(err) => fail(err),
    }
}

fn callers_of_symbol(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    symbol_side(db, repo_id, arguments, true)
}

fn callees_of_symbol(db: &IndexDatabase, repo_id: &RepoId, arguments: &Value) -> ToolCallResult {
    symbol_side(db, repo_id, arguments, false)
}

fn symbol_side(
    db: &IndexDatabase,
    repo_id: &RepoId,
    arguments: &Value,
    callers: bool,
) -> ToolCallResult {
    let symbol = match required(arguments, "symbol") {
        Ok(symbol) => symbol,
        Err(result) => return result,
    };
    match db.find_symbol_deps(repo_id, symbol) {
        Ok(deps) => ok(json!(if callers { deps.callers } else { deps.callees })),
        Err(err) => fail(err),
    }
}

fn dependency_summary(db: &IndexDatabase, repo_id: &RepoId) -> ToolCallResult {
    match db.reference_summary_by_kind(repo_id) {
        Ok(summary) => ok(json!({
            "calls": summary.calls_count,
            "imports": summary.imports_count,
            "tests": summary.tests_count,
            "other": 0,
            "top_imported": summary.top_imported.into_iter().map(|(target, count)| json!({
                "target": target,
                "count": count
            })).collect::<Vec<_>>()
        })),
        Err(err) => fail(err),
    }
}

fn context_dump(
    repo_root: &Path,
    repo_id: &RepoId,
    arguments: &Value,
    policy: Option<&SessionPolicy>,
) -> ToolCallResult {
    let task = match required(arguments, "task") {
        Ok(task) => task,
        Err(result) => return result,
    };
    let budget = arguments
        .get("budget")
        .and_then(|value| value.as_u64())
        .unwrap_or(4000)
        .min(100_000) as usize;
    match ContextCompiler::compile(repo_root, task, budget) {
        Ok(mut package) => {
            apply_policy_to_package(repo_id, policy, &mut package);
            let markdown = package.to_markdown();
            let char_cap = budget.saturating_mul(4).max(16_000);
            let truncated_text = truncate_chars(&markdown, char_cap);
            let truncated = truncated_text.len() < markdown.len();
            ok(json!({
                "markdown": truncated_text,
                "truncated": truncated,
                "budget": package.budget
            }))
        }
        Err(err) => fail(err),
    }
}

fn rank_files(repo_root: &Path, arguments: &Value) -> ToolCallResult {
    let task = match required(arguments, "task") {
        Ok(task) => task,
        Err(result) => return result,
    };
    match ContextCompiler::rank(repo_root, task) {
        Ok(ranked) => ok(json!({ "files": ranked.into_iter().take(30).collect::<Vec<_>>() })),
        Err(err) => fail(err),
    }
}

fn budget_preview(repo_root: &Path, arguments: &Value) -> ToolCallResult {
    let task = match required(arguments, "task") {
        Ok(task) => task,
        Err(result) => return result,
    };
    let budget = arguments
        .get("budget")
        .and_then(|value| value.as_u64())
        .unwrap_or(2000)
        .min(100_000) as usize;
    let ranked = match ContextCompiler::rank(repo_root, task) {
        Ok(ranked) => ranked,
        Err(err) => return fail(err),
    };
    let items: Vec<BudgetItem> = ranked
        .iter()
        .map(|file| BudgetItem {
            id: file.path.clone(),
            score: file.total,
            tokens: estimate_tokens(&file.path) + 48,
        })
        .collect();
    let max_items = (budget / 128).clamp(5, 50);
    let chosen = allocate(&items, budget, max_items);
    ok(json!({ "budget": budget, "chosen": chosen }))
}

fn trim_package_for_json(package: &mut EvidencePackage, max_chars: usize) -> (String, bool) {
    let payload = package.serialized_payload();
    if payload.chars().count() <= max_chars {
        return (payload, false);
    }

    for idx in (0..package.relevant_files.len()).rev() {
        if package.relevant_files[idx].snippet.take().is_some() {
            let next = package.serialized_payload();
            if next.chars().count() <= max_chars {
                return (next, true);
            }
        }
    }
    macro_rules! pop_until_fit {
        ($vec:expr) => {
            while !$vec.is_empty() {
                $vec.pop();
                package.budget.returned_files = package.relevant_files.len();
                let next = package.serialized_payload();
                if next.chars().count() <= max_chars {
                    return (next, true);
                }
            }
        };
    }
    pop_until_fit!(package.diagnostic_memories);
    pop_until_fit!(package.git_context.recent_commits);
    pop_until_fit!(package.relevant_files);
    pop_until_fit!(package.relevant_symbols);
    pop_until_fit!(package.verified_memories);
    pop_until_fit!(package.relevant_facts);
    pop_until_fit!(package.dependencies);
    pop_until_fit!(package.relevant_tests);
    package.git_context.modified_files.clear();
    (package.serialized_payload(), true)
}

fn evidence_package(
    repo_root: &Path,
    repo_id: &RepoId,
    arguments: &Value,
    policy: Option<&SessionPolicy>,
) -> ToolCallResult {
    let task = match required(arguments, "task") {
        Ok(task) => task,
        Err(result) => return result,
    };
    let budget = arguments
        .get("budget")
        .and_then(|value| value.as_u64())
        .unwrap_or(4000)
        .min(100_000) as usize;
    match ContextCompiler::compile(repo_root, task, budget) {
        Ok(mut package) => {
            apply_policy_to_package(repo_id, policy, &mut package);
            let (payload, truncated) = trim_package_for_json(&mut package, 20_000);
            ok(json!({ "package": payload, "truncated": truncated }))
        }
        Err(err) => fail(err),
    }
}

fn workspace_repos(repo_root: &Path) -> ToolCallResult {
    match discover_workspace(repo_root) {
        Ok(members) => ok(json!({
            "repositories": members.into_iter().map(|member| json!({
                "name": member.name,
                "root": member.root
            })).collect::<Vec<_>>()
        })),
        Err(err) => fail(err),
    }
}

fn workspace_search(
    repo_root: &Path,
    arguments: &Value,
    policy: Option<&SessionPolicy>,
) -> ToolCallResult {
    let query = match required(arguments, "query") {
        Ok(query) => query,
        Err(result) => return result,
    };
    let limit = limit_of(arguments, 10, 20);
    let members = match discover_workspace(repo_root) {
        Ok(members) => members,
        Err(err) => return fail(err),
    };
    let mut hits = Vec::new();
    for member in members {
        if hits.len() >= limit {
            break;
        }
        let repo_id = RepoId::from_path(&member.root);
        if let Some(p) = policy {
            if !p.allows_repository(repo_id.as_str()) {
                continue;
            }
        }
        let Ok(db_path) = agent_core::paths::StoragePaths::repo_db_path(&repo_id) else {
            continue;
        };
        if !db_path.is_file() {
            continue;
        }
        let Ok(db) = IndexDatabase::open(&db_path) else {
            continue;
        };
        if let Ok(found) = db.search_content(query, limit - hits.len()) {
            for hit in found {
                hits.push(json!({
                    "repository": member.name,
                    "path": hit.relative_path,
                    "snippet": SecretPattern::redact(&hit.snippet)
                }));
            }
        }
    }
    ok(json!({ "hits": hits }))
}

fn workspace_symbols(
    repo_root: &Path,
    arguments: &Value,
    policy: Option<&SessionPolicy>,
) -> ToolCallResult {
    let query = arguments
        .get("query")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let members = match discover_workspace(repo_root) {
        Ok(members) => members,
        Err(err) => return fail(err),
    };
    let mut collected = Vec::new();
    for member in members {
        let repo_id = RepoId::from_path(&member.root);
        if let Some(p) = policy {
            if !p.allows_repository(repo_id.as_str()) {
                continue;
            }
        }
        let Ok(db_path) = agent_core::paths::StoragePaths::repo_db_path(&repo_id) else {
            continue;
        };
        if !db_path.is_file() {
            continue;
        }
        let Ok(db) = IndexDatabase::open(&db_path) else {
            continue;
        };
        if let Ok(symbols) = db.list_symbols_paginated(&repo_id, query, 0, 200) {
            for sym in symbols {
                collected.push(json!({
                    "repository": member.name,
                    "repo_id": repo_id.as_str(),
                    "symbol": sym
                }));
            }
        }
    }
    let (page, truncated, next_cursor) = paginate_vec(collected, arguments, 50, 200);
    ok(json!({ "symbols": page, "truncated": truncated, "next_cursor": next_cursor }))
}

fn fact_extract(
    git: &GitRepo,
    db: &IndexDatabase,
    repo_id: &RepoId,
    repo_info: &RepoInfo,
    policy: Option<&SessionPolicy>,
) -> ToolCallResult {
    let commit = repo_info
        .head_commit
        .clone()
        .unwrap_or_else(|| "uncommitted".to_string());
    let hunks = git.get_diff_hunks().unwrap_or_default();
    let mut facts = facts_from_hunks(repo_id.as_str(), &commit, &hunks);
    if let Ok(symbols) = db.list_symbols(repo_id.as_str()) {
        facts.extend(facts_from_symbols(repo_id.as_str(), &commit, &symbols));
    }
    let store = match scoped_fact_store(repo_id, policy) {
        Ok(store) => store,
        Err(err) => return fail(err),
    };
    if let Err(err) = store.upsert_batch(&facts) {
        return fail(err);
    }
    if policy.is_none()
        || matches!(policy, Some(p) if matches!(p.scope, agent_skills::SharedContextScope::Repository(_)))
    {
        if let Err(err) = project_facts(db, repo_info, &facts) {
            return fail(err);
        }
    }
    ok(json!({ "stored": facts.len(), "commit": commit }))
}

fn fact_list(
    repo_id: &RepoId,
    arguments: &Value,
    policy: Option<&SessionPolicy>,
) -> ToolCallResult {
    let store = match scoped_fact_store(repo_id, policy) {
        Ok(store) => store,
        Err(err) => return fail(err),
    };
    match store.list(repo_id.as_str()) {
        Ok(facts) => {
            let now = Utc::now();
            let listed = facts
                .into_iter()
                .map(|fact| {
                    let effective = store.effective_confidence(&fact, now);
                    json!({
                        "id": fact.id,
                        "claim": fact.claim,
                        "path": fact.relative_path,
                        "symbol": fact.symbol,
                        "commit": fact.commit_id,
                        "source": fact.source,
                        "confidence": effective
                    })
                })
                .collect::<Vec<_>>();
            let (page, truncated, next_cursor) = paginate_vec(listed, arguments, 200, 200);
            ok(json!({ "facts": page, "truncated": truncated, "next_cursor": next_cursor }))
        }
        Err(err) => fail(err),
    }
}

fn fact_decay(
    db: &IndexDatabase,
    repo_info: &RepoInfo,
    arguments: &Value,
    policy: Option<&SessionPolicy>,
) -> ToolCallResult {
    let repo_id = &repo_info.id;
    let half_life = arguments
        .get("half_life_days")
        .and_then(|value| value.as_f64())
        .unwrap_or(DEFAULT_HALF_LIFE_DAYS);
    let store = match scoped_fact_store(repo_id, policy) {
        Ok(store) => store,
        Err(err) => return fail(err),
    };
    match store.decay(Utc::now(), half_life) {
        Ok(changed) => match store.list(repo_id.as_str()) {
            Ok(facts) => {
                let projected = if policy.is_none()
                    || matches!(policy, Some(p) if matches!(p.scope, agent_skills::SharedContextScope::Repository(_)))
                {
                    match project_facts(db, repo_info, &facts) {
                        Ok(n) => n,
                        Err(err) => return fail(err),
                    }
                } else {
                    0
                };
                ok(json!({
                    "updated": changed,
                    "projected": projected,
                    "half_life_days": half_life
                }))
            }
            Err(err) => fail(err),
        },
        Err(err) => fail(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_surface_has_at_least_twenty_five_tools() {
        assert!(definitions().len() >= 25, "{}", definitions().len());
    }

    #[test]
    fn test_filter_rejects_shell_syntax() {
        assert!(valid_test_filter("auth::rotate"));
        assert!(!valid_test_filter(""));
        assert!(!valid_test_filter("--manifest-path=/etc/passwd"));
        assert!(!valid_test_filter("-p"));
        assert!(!valid_test_filter("auth; rm -rf /"));
        assert!(!valid_test_filter("$(id)"));
        assert!(!valid_test_filter("a b"));
    }
}
