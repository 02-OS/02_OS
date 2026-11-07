use agent_core::Result;
use agent_git::GitDiscovery;
use agent_memory::{cognify_pipeline, RepoHandle, TaskRegistry};
use serde_json::json;
use std::env;

pub fn execute(dataset: Option<String>, full: bool, json_output: bool) -> Result<()> {
    let current = env::current_dir()?;
    let root = GitDiscovery::find_repository_root(&current)?;
    let mut handle = RepoHandle::open(&root)?;
    let report = cognify_pipeline(&mut handle, full, dataset.as_deref(), &TaskRegistry::new())?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "success",
                "dataset_id": report.dataset_id,
                "nodes": report.nodes,
                "edges": report.edges,
                "chunks": report.chunks,
                "symbols": report.symbols,
            }))?
        );
    } else {
        println!("02 Agent Runtime: Cognify complete");
        println!("  Dataset:  {}", report.dataset_id);
        println!("  Symbols:  {}", report.symbols);
        println!("  Nodes:    {}", report.nodes);
        println!("  Edges:    {}", report.edges);
        println!("  Chunks:   {}", report.chunks);
    }
    Ok(())
}
