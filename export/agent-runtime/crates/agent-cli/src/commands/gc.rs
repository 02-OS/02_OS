use agent_core::{paths::StoragePaths, Result};
use agent_index::gc_repos;
use serde_json::json;

pub fn execute(json_output: bool) -> Result<()> {
    let data_dir = StoragePaths::data_dir()?;
    let report = if data_dir.is_dir() {
        gc_repos(&data_dir)?
    } else {
        agent_index::GcReport::default()
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "success",
                "orphan_directories": report.orphan_directories,
                "datapoints_removed": report.datapoints_removed,
                "databases_vacuumed": report.databases_vacuumed,
            }))?
        );
    } else {
        println!("02 Agent Runtime: Garbage Collection");
        println!("==================================================");
        println!(
            "  Orphan repo directories removed: {}",
            report.orphan_directories
        );
        println!(
            "  Datapoints removed:              {}",
            report.datapoints_removed
        );
        println!(
            "  Databases vacuumed:              {}",
            report.databases_vacuumed
        );
        println!("==================================================");
    }
    Ok(())
}
