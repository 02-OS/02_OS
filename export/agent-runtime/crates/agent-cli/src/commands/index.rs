use agent_core::{paths::StoragePaths, Result};
use agent_git::GitDiscovery;
use agent_memory::{index_pipeline, RepoHandle, TaskRegistry};
use serde_json::json;
use std::env;
use std::time::Instant;

pub fn execute(full: bool, json_output: bool) -> Result<()> {
    let start = Instant::now();
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;

    let git_probe = agent_git::GitRepo::open(&repo_root)?;
    let repo_info = git_probe.info()?;
    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
    if !db_path.exists() {
        if json_output {
            crate::commands::init::execute_quiet()?;
        } else {
            println!("Repository not initialized yet. Initializing...");
            crate::commands::init::execute(false)?;
        }
    }

    let mut handle = RepoHandle::open(&repo_root)?;
    let registry = TaskRegistry::new();
    let added = index_pipeline(&mut handle, full, &registry)?;

    let store = agent_memory::MemoryStore::for_repo(&handle.info.id)?;
    let staleness_report = agent_memory::StalenessEngine::evaluate(
        &repo_root,
        &handle.info.id,
        &handle.git,
        &handle.db,
        &store,
    )
    .ok();
    let elapsed = start.elapsed();
    let stats = handle.db.get_stats(&handle.info.id)?;
    let head_commit = handle.info.head_commit.clone();

    if json_output {
        let out = json!({
            "status": "success",
            "mode": if full { "full" } else { "incremental" },
            "files_updated": added.files_updated,
            "total_files": stats.total_files,
            "source_files": stats.source_files,
            "test_files": stats.test_files,
            "doc_files": stats.doc_files,
            "total_symbols": stats.total_symbols,
            "total_references": stats.total_references,
            "duration_ms": elapsed.as_millis(),
            "head_commit": head_commit,
            "staleness": staleness_report,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "02 Agent Runtime: Indexing Complete ({})",
            if full { "full" } else { "incremental" }
        );
        println!("  Files Updated:   {}", added.files_updated);
        println!("  Total Indexed:   {}", stats.total_files);
        println!("  Source Files:    {}", stats.source_files);
        println!("  Test Files:      {}", stats.test_files);
        println!("  Docs / Markdown: {}", stats.doc_files);
        println!("  Manifests:       {}", stats.manifest_files);
        println!("  Symbols Parsed:  {}", stats.total_symbols);
        println!("  References:      {}", stats.total_references);
        if let Some(ref report) = staleness_report {
            println!(
                "  Memory Status:   {} fresh, {} degraded, {} stale",
                report.fresh_count, report.degraded_count, report.stale_count
            );
            if !report.changed_symbols.is_empty() {
                println!("  Changed Symbols: {}", report.changed_symbols.join(", "));
            }
        }
        println!("  Time Taken:      {:.2?}", elapsed);
        println!(
            "  Target Commit:   {}",
            head_commit.as_deref().unwrap_or("none")
        );
    }

    Ok(())
}
