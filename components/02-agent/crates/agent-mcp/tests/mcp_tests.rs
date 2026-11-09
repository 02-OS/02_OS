use agent_core::{
    config::RepoConfig,
    types::{FileKind, IndexedFile, Language, RepoId, RepoInfo},
};
use agent_index::{IndexDatabase, RepoScanner, ScannedFile};
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
    assert_eq!(codex_conf["mcp_servers"]["02"]["command"], "02");
    assert_eq!(
        codex_conf["mcp_servers"]["02"]["args"].clone(),
        json!(["mcp"])
    );

    let connect_out = AgentConnector::connect("claude", false).unwrap();
    let claude_value: serde_json::Value = serde_json::from_str(&connect_out).unwrap();
    assert_eq!(claude_value["mcpServers"]["02"]["command"], "02");
    assert_eq!(
        claude_value["mcpServers"]["02"]["args"].clone(),
        json!(["mcp"])
    );
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
        AgentTarget::parse("codex").default_config_path().unwrap(),
        home.join(".codex/config.toml")
    );
    assert_eq!(
        AgentTarget::parse("claude").default_config_path().unwrap(),
        home.join(".claude.json")
    );
    assert_eq!(
        AgentTarget::parse("opencode")
            .default_config_path()
            .unwrap(),
        home.join(".config/opencode/opencode.json")
    );
    assert_eq!(
        AgentTarget::parse("gemini").default_config_path().unwrap(),
        home.join(".gemini/settings.json")
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
    let gemini: serde_json::Value =
        serde_json::from_str(&AgentConnector::connect("gemini", false).unwrap()).unwrap();
    assert_eq!(gemini["mcpServers"]["02"]["command"], "02");
    let opencode: serde_json::Value =
        serde_json::from_str(&AgentConnector::connect("opencode", false).unwrap()).unwrap();
    assert_eq!(opencode["mcp"]["02"]["type"], "local");
    assert_eq!(
        opencode["mcp"]["02"]["command"].clone(),
        json!(["02", "mcp"])
    );
    assert!(opencode.get("mcpServers").is_none());
    let codex = AgentConnector::connect("codex", false).unwrap();
    assert!(codex.contains("[mcp_servers.02]"));
    assert!(!codex.contains("mcpServers"));

    let tools = agent_mcp::list_tools();
    assert!(tools
        .iter()
        .all(|tool| { tool.input_schema["properties"]["repo_path"]["type"] == "string" }));
}

#[test]
fn test_merge_config_text_rejects_non_objects() {
    let array = AgentConnector::merge_config_text(&AgentTarget::Cursor, "[]");
    assert!(array.is_err());
    let list = AgentConnector::merge_config_text(&AgentTarget::Cursor, r#"{"mcpServers":[]}"#);
    assert!(list.is_err());
    let invalid = AgentConnector::merge_config_text(&AgentTarget::Cursor, "{");
    assert!(invalid.is_err());
    let bad_toml = AgentConnector::merge_config_text(&AgentTarget::Codex, "[[[not toml");
    assert!(bad_toml.is_err());

    let merged = AgentConnector::merge_config_text(
        &AgentTarget::Cursor,
        r#"{"theme":"dark","mcpServers":{"other":{"command":"keep"}}}"#,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(value["mcpServers"]["02"]["command"], "02");
    assert_eq!(value["mcpServers"]["02"]["args"].clone(), json!(["mcp"]));
    assert_eq!(value["mcpServers"]["other"]["command"], "keep");
    assert_eq!(value["theme"], "dark");

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

#[test]
fn jsonrpc_notifications_are_silent_and_bad_version_is_invalid_request() {
    use std::io::Cursor;
    use std::path::PathBuf;

    let server = McpServer::new(PathBuf::from("."));
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"no/such-method\"}\n",
        "{\"jsonrpc\":\"1.0\",\"method\":\"ping\"}\n",
    );
    let mut output = Vec::new();
    server.run_stdio(Cursor::new(input), &mut output).unwrap();
    assert!(output.is_empty(), "{}", String::from_utf8_lossy(&output));

    let mut output = Vec::new();
    server
        .run_stdio(
            Cursor::new("{\"jsonrpc\":\"1.0\",\"id\":9,\"method\":\"ping\"}\n"),
            &mut output,
        )
        .unwrap();
    let bad: serde_json::Value =
        serde_json::from_str(std::str::from_utf8(&output).unwrap().trim()).unwrap();
    assert_eq!(bad["error"]["code"], -32600);
    assert_eq!(bad["id"], 9);
    assert!(bad.get("result").is_none());

    let mut output = Vec::new();
    server
        .run_stdio(
            Cursor::new("{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\"}\n"),
            &mut output,
        )
        .unwrap();
    let listed: serde_json::Value =
        serde_json::from_str(std::str::from_utf8(&output).unwrap().trim()).unwrap();
    assert_eq!(listed["id"], 3);
    assert!(!listed["result"]["tools"].as_array().unwrap().is_empty());

    let mut output = Vec::new();
    server
        .run_stdio(Cursor::new("not-json\n"), &mut output)
        .unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(std::str::from_utf8(&output).unwrap().trim()).unwrap();
    assert_eq!(parsed["error"]["code"], -32700);
}

#[test]
fn connect_write_preserves_client_files() {
    let executable = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let home = tempdir().unwrap();
    let root = home.path();

    let codex = root.join(".codex/config.toml");
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::write(
        &codex,
        "\
model = \"keep-me\"
disable_response_storage = true

[mcp_servers.other]
command = \"npx\"
args = [\"-y\", \"other\"]

[projects.\"/work/repo\"]
trust_level = \"trusted\"
",
    )
    .unwrap();
    AgentConnector::connect_at(root, "codex", true).unwrap();
    let codex_text = fs::read_to_string(&codex).unwrap();
    let codex_doc =
        AgentConnector::parse_config_document(&AgentTarget::Codex, &codex_text).unwrap();
    assert_eq!(codex_doc["model"], "keep-me");
    assert_eq!(codex_doc["disable_response_storage"].as_bool(), Some(true));
    assert_eq!(codex_doc["mcp_servers"]["other"]["command"], "npx");
    assert_eq!(
        codex_doc["mcp_servers"]["other"]["args"].clone(),
        json!(["-y", "other"])
    );
    assert_eq!(
        codex_doc["mcp_servers"]["02"]["command"],
        executable.as_str()
    );
    assert_eq!(
        codex_doc["mcp_servers"]["02"]["args"].clone(),
        json!(["mcp"])
    );
    assert_eq!(
        codex_doc["projects"]["/work/repo"]["trust_level"],
        "trusted"
    );

    let broken = root.join(".codex/config.toml");
    let original = "mcp_servers = \"nope\"\n";
    fs::write(&broken, original).unwrap();
    assert!(AgentConnector::connect_at(root, "codex", true).is_err());
    assert_eq!(fs::read_to_string(&broken).unwrap(), original);

    fs::write(
        &codex,
        "model = \"keep-me\"\n\n[mcp_servers.other]\ncommand = \"npx\"\nargs = [\"-y\", \"other\"]\n",
    )
    .unwrap();
    AgentConnector::connect_at(root, "codex", true).unwrap();

    let opencode = root.join(".config/opencode/opencode.json");
    fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    fs::write(
        &opencode,
        r#"{"$schema":"https://opencode.ai/config.json","theme":"dark","mcp":{"other":{"type":"remote","url":"https://example.com","enabled":true}}}"#,
    )
    .unwrap();
    AgentConnector::connect_at(root, "opencode", true).unwrap();
    let opencode_doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&opencode).unwrap()).unwrap();
    assert_eq!(opencode_doc["$schema"], "https://opencode.ai/config.json");
    assert_eq!(opencode_doc["theme"], "dark");
    assert_eq!(opencode_doc["mcp"]["other"]["type"], "remote");
    assert_eq!(
        opencode_doc["mcp"]["other"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(opencode_doc["mcp"]["02"]["type"], "local");
    assert_eq!(
        opencode_doc["mcp"]["02"]["command"].clone(),
        json!([executable, "mcp"])
    );

    let claude = root.join(".claude.json");
    fs::write(
        &claude,
        r#"{"numStartups":4,"projects":{"/work":{"mcpServers":{"local":{"command":"keep"}}}},"mcpServers":{"other":{"command":"keep","args":["x"]}}}"#,
    )
    .unwrap();
    AgentConnector::connect_at(root, "claude", true).unwrap();
    let claude_doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&claude).unwrap()).unwrap();
    assert_eq!(claude_doc["numStartups"].as_i64(), Some(4));
    assert_eq!(
        claude_doc["projects"]["/work"]["mcpServers"]["local"]["command"],
        "keep"
    );
    assert_eq!(claude_doc["mcpServers"]["other"]["command"], "keep");
    assert_eq!(
        claude_doc["mcpServers"]["02"]["command"],
        executable.as_str()
    );
    assert_eq!(
        claude_doc["mcpServers"]["02"]["args"].clone(),
        json!(["mcp"])
    );

    let gemini = root.join(".gemini/settings.json");
    fs::create_dir_all(gemini.parent().unwrap()).unwrap();
    fs::write(
        &gemini,
        r#"{"theme":"dark","mcpServers":{"other":{"command":"keep","args":[]}}}"#,
    )
    .unwrap();
    AgentConnector::connect_at(root, "gemini", true).unwrap();
    let gemini_doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&gemini).unwrap()).unwrap();
    assert_eq!(gemini_doc["theme"], "dark");
    assert_eq!(gemini_doc["mcpServers"]["other"]["command"], "keep");
    assert_eq!(
        gemini_doc["mcpServers"]["02"]["args"].clone(),
        json!(["mcp"])
    );

    assert_eq!(
        AgentTarget::parse("cursor").config_path(root).unwrap(),
        root.join(".cursor/mcp.json")
    );
    assert_eq!(
        AgentTarget::parse("zed").config_path(root).unwrap(),
        root.join(".config/zed/settings.json")
    );
}

#[test]
fn codex_connect_preserves_complete_toml_values_and_replaces_atomically() {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;

    let home = tempdir().unwrap();
    let path = home.path().join(".codex/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = r####"
model = "keep-me"
custom_instructions = """Follow \u0061ll checks. \U0001f642\b\f
Continue \
    across this line."""
literal_instructions = '''Keep literal \u0061 and "quotes".'''
inline_settings = { "quoted.key" = [1, 2], enabled = true }

[[skills.config]]
path = "/tmp/fixture/first/SKILL.md"
enabled = false

[[skills.config]]
path = "/tmp/fixture/second/SKILL.md"
enabled = true

[projects."/tmp/fixture/quoted project"]
trust_level = "trusted"

[mcp_servers.other]
command = "other-server"
args = ["--keep"]

[mcp_servers.02]
command = "old-server"
args = ["--old"]
startup_timeout_sec = 30

[mcp_servers.02.env]
KEEP_SETTING = "keep-me"
"####;
    fs::write(&path, original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let mut old_file = fs::File::open(&path).unwrap();
    let mut expected: toml::Table = toml::from_str(original).unwrap();
    expected["mcp_servers"]["02"]["command"] =
        toml::Value::String(std::env::current_exe().unwrap().to_str().unwrap().into());
    expected["mcp_servers"]["02"]["args"] =
        toml::Value::Array(vec![toml::Value::String("mcp".into())]);

    AgentConnector::connect_at(home.path(), "codex", true).unwrap();
    let rewritten = fs::read_to_string(&path).unwrap();
    let actual: toml::Table = toml::from_str(&rewritten).unwrap();
    assert_eq!(actual, expected);
    assert!(actual["custom_instructions"]
        .as_str()
        .unwrap()
        .starts_with("Follow all checks. 🙂"));
    assert_eq!(actual["skills"]["config"].as_array().unwrap().len(), 2);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );

    // Existing readers must keep the complete old document; a truncating rewrite
    // would also change the bytes visible through this already-open handle.
    let mut old_contents = String::new();
    old_file.read_to_string(&mut old_contents).unwrap();
    assert_eq!(old_contents, original);
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn codex_connect_rejects_invalid_input_without_replacing_the_config() {
    let home = tempdir().unwrap();
    let path = home.path().join(".codex/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    for invalid in ["model = \"unterminated", "mcp_servers.02 = 5\n"] {
        fs::write(&path, invalid).unwrap();
        assert!(AgentConnector::connect_at(home.path(), "codex", true).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }
}

#[test]
fn connect_refuses_symlinked_config_and_creates_new_configs_privately() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir().unwrap();
    let external = home.path().join("external-config.toml");
    let original = "model = \"keep-me\"\n";
    fs::write(&external, original).unwrap();
    let path = home.path().join(".codex/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&external, &path).unwrap();
    assert!(AgentConnector::connect_at(home.path(), "codex", true).is_err());
    assert!(path.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(&external).unwrap(), original);
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);

    fs::remove_file(&path).unwrap();
    AgentConnector::connect_at(home.path(), "codex", true).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn repository_search_warns_when_the_full_text_index_is_empty() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn zebra_marker() {}\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("src/main.rs")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    let repo_id = RepoId::from_path(root);
    let db_path = agent_core::paths::StoragePaths::repo_db_path(&repo_id).unwrap();
    let mut db = IndexDatabase::open(&db_path).unwrap();
    db.update_repo_info(&RepoInfo {
        id: repo_id.clone(),
        name: "fts-unbuilt".into(),
        root_path: root.to_path_buf(),
        head_commit: None,
        branch: Some("master".into()),
        is_clean: true,
        modified_count: 0,
        untracked_count: 0,
    })
    .unwrap();
    db.save_scanned_files(
        &repo_id,
        &[ScannedFile {
            file: IndexedFile {
                relative_path: "src/main.rs".into(),
                file_hash: "abc".into(),
                git_blob_id: None,
                size_bytes: 4,
                language: Language::Rust,
                kind: FileKind::Source,
                is_test: false,
                is_doc: false,
                indexed_at: chrono::Utc::now(),
            },
            absolute_path: root.join("src/main.rs"),
            content: None,
        }],
    )
    .unwrap();
    drop(db);

    let unbuilt = agent_mcp::call_tool(
        root,
        "repository_search",
        &json!({ "query": "zebra", "limit": 5 }),
    );
    assert!(!unbuilt.is_error, "{unbuilt:?}");
    let body: serde_json::Value = serde_json::from_str(&unbuilt.content[0].text).unwrap();
    assert!(body.as_array().is_none(), "unbuilt search returned {body}");
    assert_eq!(body["warnings"][0]["code"], "fts_unbuilt");
    assert_eq!(body["hits"].as_array().unwrap().len(), 0);

    let mut db = IndexDatabase::open(&db_path).unwrap();
    db.save_scanned_files(
        &repo_id,
        &[ScannedFile {
            file: IndexedFile {
                relative_path: "src/main.rs".into(),
                file_hash: "abc".into(),
                git_blob_id: None,
                size_bytes: 20,
                language: Language::Rust,
                kind: FileKind::Source,
                is_test: false,
                is_doc: false,
                indexed_at: chrono::Utc::now(),
            },
            absolute_path: root.join("src/main.rs"),
            content: Some("fn zebra_marker() {}\n".into()),
        }],
    )
    .unwrap();
    drop(db);

    let built = agent_mcp::call_tool(
        root,
        "repository_search",
        &json!({ "query": "zebra", "limit": 5 }),
    );
    assert!(!built.is_error, "{built:?}");
    let hits: serde_json::Value = serde_json::from_str(&built.content[0].text).unwrap();
    let Some(hits) = hits.as_array() else {
        panic!("built search returned {hits}");
    };
    assert!(hits.iter().any(|hit| hit["relative_path"] == "src/main.rs"));

    let _ = fs::remove_dir_all(db_path.parent().unwrap());
}

#[test]
fn unknown_connect_target_writes_nothing() {
    let home = tempdir().unwrap();
    let err = AgentConnector::connect_at(home.path(), "not-a-client", true).unwrap_err();
    assert!(err.to_string().contains("unsupported connect target"));
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
    let preview = AgentConnector::connect_at(home.path(), "not-a-client", false).unwrap_err();
    assert!(preview.to_string().contains("unsupported connect target"));
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
}
