use agent_core::Result;
use agent_git::GitDiscovery;
use agent_mcp::McpServer;
use std::env;
use std::io;

pub fn execute() -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;

    let server = McpServer::new(repo_root);
    let stdin = io::stdin();
    let stdout = io::stdout();

    server.run_stdio(stdin.lock(), stdout.lock())
}
