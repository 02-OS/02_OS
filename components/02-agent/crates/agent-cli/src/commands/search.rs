use agent_core::{error::AgentError, paths::StoragePaths, Result};
use agent_git::{GitDiscovery, GitRepo};
use agent_index::IndexDatabase;
use serde_json::json;
use std::env;

pub fn execute(query: &str, limit: usize, json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
    if !db_path.exists() {
        return Err(AgentError::RepositoryNotInitialized);
    }

    let db = IndexDatabase::open(&db_path)?;
    let results = db.search_content(query, limit)?;
    let unbuilt = db.fts_needs_rebuild()?;

    if json_output {
        let mut out = json!({
            "query": query,
            "count": results.len(),
            "results": results,
        });
        if unbuilt {
            out["warnings"] = json!([{
                "code": "fts_unbuilt",
                "message": "full-text index is empty; run 02 index"
            }]);
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        if unbuilt {
            println!("full-text index is empty; run 02 index");
        }
        println!("Search Results for: \"{}\"", query);
        if results.is_empty() {
            println!("  No matching files or symbols found.");
        } else {
            for (idx, r) in results.iter().enumerate() {
                println!("[{}] {}", idx + 1, r.relative_path);
                if !r.snippet.is_empty() {
                    let clean = r
                        .snippet
                        .replace("<b>", "\x1b[1;33m")
                        .replace("</b>", "\x1b[0m");
                    println!("    Snippet: {}", clean.trim());
                }
            }
        }
    }

    Ok(())
}
