use agent_core::Result;
use agent_git::GitDiscovery;
use agent_index::GraphExportFormat;
use agent_memory::{export_graph, RepoHandle};
use serde_json::json;
use std::env;
use std::fs;
use std::path::PathBuf;

pub fn execute_export(
    format: &str,
    dataset: Option<String>,
    output: Option<PathBuf>,
    json_output: bool,
) -> Result<()> {
    let parsed = GraphExportFormat::parse(format)?;
    let current = env::current_dir()?;
    let root = GitDiscovery::find_repository_root(&current)?;
    let handle = RepoHandle::open(&root)?;
    let document = export_graph(&handle, dataset.as_deref(), parsed)?;

    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&path, &document)?;
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "success",
                    "format": format,
                    "path": path,
                }))?
            );
        } else {
            println!("Wrote {} graph to {}", format, path.display());
        }
    } else {
        println!("{document}");
    }
    Ok(())
}
