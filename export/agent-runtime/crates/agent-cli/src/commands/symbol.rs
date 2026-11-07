use agent_core::{error::AgentError, paths::StoragePaths, types::SecretPattern, Result};
use agent_git::{GitDiscovery, GitRepo};
use agent_index::IndexDatabase;
use serde_json::json;
use std::env;

pub fn execute(name: &str, json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
    if !db_path.exists() {
        return Err(AgentError::RepositoryNotInitialized);
    }

    let db = IndexDatabase::open(&db_path)?;
    let mut symbols = db.find_symbols_by_name(&repo_info.id, name)?;
    for symbol in &mut symbols {
        if let Some(signature) = symbol.signature.as_mut() {
            *signature = SecretPattern::redact(signature);
        }
    }

    if json_output {
        let out = json!({
            "query_symbol": name,
            "count": symbols.len(),
            "symbols": symbols,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Symbol Definition for '{}'", name);
        println!("==================================================");
        if symbols.is_empty() {
            println!("  Symbol '{}' not found in current index.", name);
            println!("  Tip: Run '02 index' if you recently added this symbol.");
        } else {
            for (i, sym) in symbols.iter().enumerate() {
                println!("[{}] {} ({})", i + 1, sym.name, sym.kind);
                println!("    File:        {}:{}", sym.file_path, sym.start_line);
                println!("    Qualified:   {}", sym.qualified_name);
                if let Some(ref sig) = sym.signature {
                    println!("    Signature:   {}", sig.trim());
                }
                println!("    Fingerprint: {}", &sym.fingerprint[..12]);
            }
        }
        println!("==================================================");
    }

    Ok(())
}
