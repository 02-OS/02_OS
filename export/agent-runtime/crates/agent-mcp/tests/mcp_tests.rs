use agent_core::{
    config::RepoConfig,
    types::{RepoId, RepoInfo},
};
use agent_index::{IndexDatabase, RepoScanner};
use agent_mcp::{protocol::JsonRpcRequest, AgentConnector, AgentTarget, McpServer};
use git2::Repository;
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_mcp_protocol_and_tools() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();

    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();

    // 1. Create file and commit
    fs::create_dir_all(root.join("src")).unwrap();
    let file = root.join("src/auth.rs");
    fs::write(
        &file,
        "pub fn rotate_refresh_token() -> bool {\n    true\n}\n",
    )
    .unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("src/auth.rs")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let commit_id = repo
        .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .unwrap();

    // 2. Index repo
    let repo_id = RepoId::from_path(root);
    let db_path = agent_core::paths::StoragePaths::repo_db_path(&repo_id).unwrap();
    let mut db = IndexDatabase::open(&db_path).unwrap();

    let repo_info = RepoInfo {
        id: repo_id.clone(),
        name: "test_mcp_repo".to_string(),
        root_path: root.to_path_buf(),
        head_commit: Some(commit_id.to_string()),
        branch: Some("master".to_string()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    };
    db.update_repo_info(&repo_info).unwrap();

    let scanner = RepoScanner::new(root, RepoConfig::default());
    let scanned = scanner.scan();
    db.save_scanned_files(&repo_id, &scanned).unwrap();

    let server = McpServer::new(root.to_path_buf());

    // 3. Test 'initialize'
    let init_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "initialize".to_string(),
        params: Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {}
        })),
    };
    let init_resp = server.handle_request(init_req).unwrap();
    assert_eq!(init_resp.id, json!(1));
    let res = init_resp.result.unwrap();
    assert_eq!(res["serverInfo"]["name"], "02-agent-runtime");

    // 4. Test 'ping'
    let ping_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(2)),
        method: "ping".to_string(),
        params: None,
    };
    let ping_resp = server.handle_request(ping_req).unwrap();
    assert_eq!(ping_resp.id, json!(2));

    // 5. Test 'tools/list'
    let list_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(3)),
        method: "tools/list".to_string(),
        params: None,
    };
    let list_resp = server.handle_request(list_req).unwrap();
    let tools = list_resp.result.unwrap()["tools"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(tools.len(), 15);
    assert!(tools.iter().any(|t| t["name"] == "repository_context"));
    assert!(tools.iter().any(|t| t["name"] == "repository_status"));
    assert!(tools.iter().any(|t| t["name"] == "add"));
    assert!(tools.iter().any(|t| t["name"] == "cognify"));
    assert!(tools.iter().any(|t| t["name"] == "graph_export"));

    // 6. Test 'tools/call' -> repository_status
    let status_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(4)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "repository_status",
            "arguments": {}
        })),
    };
    let status_resp = server.handle_request(status_req).unwrap();
    let status_content = status_resp.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(status_content.contains(repo_id.as_str()));

    // 7. Test 'tools/call' -> memory_write and memory_get
    let write_mem_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(5)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "memory_write",
            "arguments": {
                "id": "mem_test_token",
                "claim": "Tokens are single use",
                "file": "src/auth.rs",
                "symbols": ["rotate_refresh_token"]
            }
        })),
    };
    let write_resp = server.handle_request(write_mem_req).unwrap();
    assert!(write_resp.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("saved"));

    let get_mem_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(6)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "memory_get",
            "arguments": { "id": "mem_test_token" }
        })),
    };
    let get_resp = server.handle_request(get_mem_req).unwrap();
    let get_text = get_resp.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(get_text.contains("Tokens are single use"));

    // 8. Test 'tools/call' -> repository_context
    let context_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(7)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "repository_context",
            "arguments": {
                "task": "refresh token rotation",
                "token_budget": 4000
            }
        })),
    };
    let context_resp = server.handle_request(context_req).unwrap();
    let context_text = context_resp.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(context_text.contains("refresh token rotation"));

    // 9. Test AgentConnector configuration generator
    let codex_conf = AgentConnector::generate_config(&AgentTarget::Codex);
    assert_eq!(codex_conf["mcpServers"]["02"]["command"], "02");

    let connect_out = AgentConnector::connect("claude", false).unwrap();
    assert!(connect_out.contains("\"02\""));
}

#[test]
fn test_connect_targets_and_repo_path_schema() {
    use agent_core::paths::StoragePaths;

    let home = StoragePaths::home_dir().unwrap();
    assert_eq!(
        AgentTarget::parse("cursor").default_config_path().unwrap(),
        home.join(".cursor/mcp.json")
    );
    assert_eq!(
        AgentTarget::parse("gemini").default_config_path().unwrap(),
        home.join(".gemini/antigravity-cli/mcp_config.json")
    );
    assert_eq!(
        AgentTarget::parse("zed").default_config_path().unwrap(),
        home.join(".config/zed/settings.json")
    );

    let zed = AgentConnector::generate_config(&AgentTarget::Zed);
    assert_eq!(zed["context_servers"]["02"]["command"], "02");
    assert_eq!(zed["context_servers"]["02"]["source"], "custom");

    let cursor = AgentConnector::connect("cursor", false).unwrap();
    assert!(cursor.contains("mcpServers"));
    let gemini = AgentConnector::connect("gemini", false).unwrap();
    assert!(gemini.contains("mcpServers"));

    let tools = agent_mcp::list_tools();
    assert!(tools
        .iter()
        .all(|tool| { tool.input_schema["properties"]["repo_path"]["type"] == "string" }));
}

#[test]
fn test_merge_config_text_rejects_non_objects() {
    let array = AgentConnector::merge_config_text(&AgentTarget::Codex, "[]");
    assert!(array.is_err());
    let list = AgentConnector::merge_config_text(&AgentTarget::Codex, r#"{"mcpServers":[]}"#);
    assert!(list.is_err());
    let invalid = AgentConnector::merge_config_text(&AgentTarget::Codex, "{");
    assert!(invalid.is_err());

    let merged = AgentConnector::merge_config_text(
        &AgentTarget::Codex,
        r#"{"mcpServers":{"other":{"command":"keep"}}}"#,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(value["mcpServers"]["02"]["command"], "02");
    assert_eq!(value["mcpServers"]["other"]["command"], "keep");

    let zed =
        AgentConnector::merge_config_text(&AgentTarget::Zed, r#"{"context_servers":{}}"#).unwrap();
    let zed_value: serde_json::Value = serde_json::from_str(&zed).unwrap();
    assert_eq!(zed_value["context_servers"]["02"]["command"], "02");
}

#[test]
fn memory_write_rejects_paths_outside_the_repository() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("src/main.rs")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "nope").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("leak.rs")).unwrap();

    for file in ["/etc/passwd", "../secret.txt", "leak.rs", ""] {
        let result = agent_mcp::call_tool(
            root,
            "memory_write",
            &json!({
                "id": "mem",
                "claim": "outside",
                "file": file,
                "symbols": []
            }),
        );
        assert!(result.is_error, "accepted evidence path {file}");
    }

    let ok = agent_mcp::call_tool(
        root,
        "memory_write",
        &json!({
            "id": "mem_ok",
            "claim": "inside",
            "file": "src/main.rs",
            "symbols": ["main"]
        }),
    );
    assert!(!ok.is_error, "{ok:?}");
}
