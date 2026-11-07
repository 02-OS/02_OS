use agent_core::{config::RepoConfig, paths::StoragePaths, Result};
use agent_git::{GitDiscovery, GitRepo};
use agent_index::IndexDatabase;
use serde_json::json;
use std::env;

pub fn execute(json_output: bool) -> Result<()> {
    run(json_output, true)
}

/// Initialize storage without writing a human-readable banner to stdout.
pub fn execute_quiet() -> Result<()> {
    run(false, false)
}

fn run(json_output: bool, print_human: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let repo_dir = StoragePaths::repo_dir(&repo_info.id)?;
    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;

    let mut db = IndexDatabase::open(&db_path)?;
    db.update_repo_info(&repo_info)?;

    // Create .02agent/config.toml if not present
    let config = RepoConfig::load_or_default(&repo_root);
    config.save_to_repo(&repo_root)?;

    if json_output {
        let out = json!({
            "status": "success",
            "repo_id": repo_info.id.as_str(),
            "name": repo_info.name,
            "root_path": repo_info.root_path,
            "storage_dir": repo_dir,
            "db_path": db_path,
            "branch": repo_info.branch,
            "head_commit": repo_info.head_commit,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if print_human {
        println!("02 Agent Runtime: Repository Initialized");
        println!("  Repository:  {}", repo_info.name);
        println!("  Root Path:   {}", repo_info.root_path.display());
        println!("  Repo ID:     {}", repo_info.id);
        println!(
            "  Branch:      {}",
            repo_info.branch.as_deref().unwrap_or("none")
        );
        println!(
            "  Commit:      {}",
            repo_info.head_commit.as_deref().unwrap_or("none")
        );
        println!("  Index Store: {}", repo_dir.display());
        println!(
            "  Config:      {}",
            repo_info.root_path.join(".02agent/config.toml").display()
        );
        println!();
        println!("Next step: run '02 index' to build the initial repository index.");
    }

    Ok(())
}
