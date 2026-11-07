use agent_core::{Result, SourceKind};
use agent_git::GitDiscovery;
use agent_memory::{add_pipeline, AddRequest, RepoHandle, TaskRegistry};
use serde_json::json;
use std::env;

pub fn execute(
    source: &str,
    text: Option<String>,
    dataset: Option<String>,
    full: bool,
    json_output: bool,
) -> Result<()> {
    let sources = SourceKind::parse_list(source)?;
    let current = env::current_dir()?;
    let root = GitDiscovery::find_repository_root(&current)?;
    let mut handle = RepoHandle::open(&root)?;
    let report = add_pipeline(
        &mut handle,
        &AddRequest {
            sources,
            dataset_name: dataset,
            text,
            full,
        },
        &TaskRegistry::new(),
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "success",
                "dataset_id": report.dataset_id,
                "files_updated": report.files_updated,
                "datapoints_inserted": report.datapoints_inserted,
                "datapoints_unchanged": report.datapoints_unchanged,
            }))?
        );
    } else {
        println!("02 Agent Runtime: Add complete");
        println!("  Dataset:              {}", report.dataset_id);
        println!("  Files updated:        {}", report.files_updated);
        println!("  Datapoints inserted:  {}", report.datapoints_inserted);
        println!("  Datapoints unchanged: {}", report.datapoints_unchanged);
    }
    Ok(())
}
