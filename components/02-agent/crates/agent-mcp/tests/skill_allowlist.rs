use agent_mcp::list_tools;
use agent_skills::known_tools;

#[test]
fn skill_allowlist_matches_mcp_tools() {
    let mut published: Vec<String> = list_tools().into_iter().map(|tool| tool.name).collect();
    published.sort();
    let allowed: Vec<String> = known_tools().iter().copied().map(str::to_string).collect();
    assert_eq!(published, allowed);
}
