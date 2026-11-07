use agent_core::{error::AgentError, paths::StoragePaths, Result};
use agent_git::{GitDiscovery, GitRepo};
use agent_index::IndexDatabase;
use serde_json::json;
use std::env;

pub fn execute(symbol: &str, json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
    if !db_path.exists() {
        return Err(AgentError::RepositoryNotInitialized);
    }

    let db = IndexDatabase::open(&db_path)?;
    let tests = db.find_tests_for_symbol(&repo_info.id, symbol)?;

    if json_output {
        let out = json!({
            "target_symbol": symbol,
            "test_count": tests.len(),
            "related_tests": tests,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Related Tests for '{}'", symbol);
        println!("==================================================");
        if tests.is_empty() {
            println!(
                "  No test cases or test files directly reference '{}'.",
                symbol
            );
        } else {
            for (idx, t) in tests.iter().enumerate() {
                let test_name = t.source_symbol_name.as_deref().unwrap_or("<test_suite>");
                println!(
                    "[{}] {} in {}:{}",
                    idx + 1,
                    test_name,
                    t.source_file,
                    t.line_number
                );
                println!("    Relationship: {} -> {}", t.kind, t.target_name);
            }
        }
        println!("==================================================");
    }

    Ok(())
}
