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
    let deps = db.find_symbol_deps(&repo_info.id, symbol)?;

    if json_output {
        let out = json!({
            "symbol": deps.symbol_name,
            "definitions": deps.defined_in,
            "callers": deps.callers,
            "callees": deps.callees,
            "imports": deps.imports,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Dependency Graph for '{}'", symbol);
        println!("==================================================");

        println!("Defined In:");
        if deps.defined_in.is_empty() {
            println!("  (No local definition found; may be external or runtime dependency)");
        } else {
            for def in &deps.defined_in {
                println!(
                    "  • {} ({}) at {}:{}",
                    def.name, def.kind, def.file_path, def.start_line
                );
            }
        }

        println!();
        println!("Callers (symbols calling '{}'):", symbol);
        if deps.callers.is_empty() {
            println!("  (No callers recorded in evidence graph)");
        } else {
            for c in &deps.callers {
                let caller_str = c.source_symbol_name.as_deref().unwrap_or("<file_scope>");
                println!("  • {} in {}:{}", caller_str, c.source_file, c.line_number);
            }
        }

        println!();
        println!("Callees (calls made by '{}'):", symbol);
        if deps.callees.is_empty() {
            println!("  (No outgoing calls recorded)");
        } else {
            for c in &deps.callees {
                println!("  • {} (line {})", c.target_name, c.line_number);
            }
        }

        println!();
        println!("File Scope Imports:");
        if deps.imports.is_empty() {
            println!("  (None)");
        } else {
            for imp in &deps.imports {
                println!("  • {} in {}", imp.target_name, imp.source_file);
            }
        }
        println!("==================================================");
    }

    Ok(())
}
