/// Tool names published by the agent MCP server.
///
/// `agent-mcp` checks this list against `list_tools()`. A skill that names
/// anything else is rejected.
const KNOWN_TOOLS: &[&str] = &[
    "add",
    "budget_preview",
    "callees_of_symbol",
    "callers_of_symbol",
    "cognify",
    "context_dump",
    "dependency_summary",
    "enclosing_symbol",
    "evidence_package",
    "fact_decay",
    "fact_extract",
    "fact_list",
    "file_outline",
    "find_symbol",
    "git_branches",
    "git_commit_files",
    "git_diff_context",
    "git_file_log",
    "git_history",
    "git_show",
    "git_status",
    "goto_definition",
    "graph_export",
    "importers_of_file",
    "imports_of_file",
    "list_files",
    "list_tests",
    "memory_get",
    "memory_status",
    "memory_write",
    "rank_files",
    "references_in_file",
    "related_tests",
    "repository_context",
    "repository_search",
    "repository_status",
    "run_tests",
    "score_breakdown",
    "search_symbols",
    "symbol_dependencies",
    "symbol_references",
    "tests_covering",
    "workspace_repos",
    "workspace_search",
];

pub fn known_tools() -> &'static [&'static str] {
    KNOWN_TOOLS
}

pub(crate) fn is_known_tool(name: &str) -> bool {
    KNOWN_TOOLS.binary_search(&name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tools_are_sorted_and_unique() {
        let mut sorted = KNOWN_TOOLS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(KNOWN_TOOLS, sorted.as_slice());
        assert!(is_known_tool("repository_search"));
        assert!(!is_known_tool("shell"));
        assert!(!is_known_tool("../etc/passwd"));
    }
}
