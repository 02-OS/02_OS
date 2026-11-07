use agent_core::Result;
use agent_mcp::AgentConnector;

pub fn execute(agent: &str, write: bool) -> Result<()> {
    let output = AgentConnector::connect(agent, write)?;
    println!("{}", output);
    Ok(())
}
