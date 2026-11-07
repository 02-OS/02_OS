use agent_core::{paths::StoragePaths, AgentError, Result};
use agent_git::{GitDiscovery, GitRepo};
use agent_index::IndexDatabase;
use agent_memory::{MemoryStore, MemoryVerifier};
use serde_json::json;
use std::env;

pub fn execute_list(json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let store = MemoryStore::for_repo(&repo_info.id)?;
    store.seed_initial_if_empty(&repo_info)?;
    let memories = store.load_all()?;

    if json_output {
        let out = json!({
            "repo_id": repo_info.id.as_str(),
            "count": memories.len(),
            "memories": memories,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Repository Evidence Memories");
        println!("==================================================");
        if memories.is_empty() {
            println!("  No verified memories recorded for this repository.");
        } else {
            for m in &memories {
                let status_color = match m.status {
                    agent_core::types::MemoryStatus::Fresh => "\x1b[32m[FRESH]\x1b[0m",
                    agent_core::types::MemoryStatus::Degraded => "\x1b[33m[DEGRADED]\x1b[0m",
                    agent_core::types::MemoryStatus::Stale => "\x1b[31m[STALE]\x1b[0m",
                    agent_core::types::MemoryStatus::Invalidated => "\x1b[31m[INVALID]\x1b[0m",
                };

                println!(
                    "• {:<20} {} (conf: {:.0}%)",
                    m.id,
                    status_color,
                    m.confidence * 100.0
                );
                println!("    Claim:    {}", m.claim);
                println!(
                    "    Evidence: {} items (valid at commit: {})",
                    m.evidence.len(),
                    &m.valid_at[..m.valid_at.len().min(7)]
                );
                println!();
            }
        }
        println!("==================================================");
    }

    Ok(())
}

pub fn execute_inspect(id: &str, json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let store = MemoryStore::for_repo(&repo_info.id)?;
    let memory = store.get(id)?;

    match memory {
        Some(m) => {
            if json_output {
                println!("{}", serde_json::to_string_pretty(&m)?);
            } else {
                println!("02 Agent Runtime: Inspect Memory '{}'", m.id);
                println!("==================================================");
                println!("  ID:          {}", m.id);
                println!("  Claim:       {}", m.claim);
                println!("  Kind:        {}", m.kind);
                println!("  Status:      {}", m.status);
                println!("  Confidence:  {:.1}%", m.confidence * 100.0);
                println!("  Valid At:    {}", m.valid_at);
                println!("  Created:     {}", m.created_at.to_rfc2822());
                println!("  Updated:     {}", m.updated_at.to_rfc2822());
                println!();
                println!("  Evidence Links:");
                for (i, ev) in m.evidence.iter().enumerate() {
                    println!("    [{}] File:    {}", i + 1, ev.file);
                    println!("        Symbols: {}", ev.symbols.join(", "));
                    println!("        Commit:  {}", ev.commit);
                    if let Some(ref fp) = ev.fingerprint {
                        println!("        Fingerprint: {}", &fp[..fp.len().min(12)]);
                    }
                }
                println!("==================================================");
            }
        }
        None => {
            return Err(AgentError::General(format!(
                "Memory with ID '{id}' not found."
            )));
        }
    }

    Ok(())
}

pub fn execute_verify(id: &str, json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
    let db = IndexDatabase::open(&db_path)?;

    let store = MemoryStore::for_repo(&repo_info.id)?;
    let memory = store.get(id)?;

    match memory {
        Some(m) => {
            let verified = MemoryVerifier::verify(
                &m,
                &repo_info.root_path,
                &repo_info.id,
                repo_info.head_commit.as_deref(),
                &db,
            )?;

            store.save(&verified)?;

            if json_output {
                let out = json!({
                    "id": verified.id,
                    "previous_status": m.status,
                    "verified_status": verified.status,
                    "confidence": verified.confidence,
                    "valid_at": verified.valid_at,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("02 Agent Runtime: Verification for '{}'", verified.id);
                println!("==================================================");
                println!("  Status:      {} -> {}", m.status, verified.status);
                println!("  Confidence:  {:.1}%", verified.confidence * 100.0);
                println!("  Claim:       {}", verified.claim);
                println!("  Verified At: {}", verified.valid_at);
                println!("==================================================");
            }
        }
        None => {
            return Err(AgentError::General(format!(
                "Memory with ID '{id}' not found."
            )));
        }
    }

    Ok(())
}
