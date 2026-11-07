use agent_core::{paths::StoragePaths, types::LiveEnvironmentInfo, Result};
use agent_git::{GitDiscovery, GitRepo};
use agent_index::IndexDatabase;
use serde_json::json;
use std::env;

pub fn execute(json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
    let live_env = LiveEnvironmentInfo::detect();

    if !db_path.exists() {
        if json_output {
            let out = json!({
                "status": "uninitialized",
                "repo_id": repo_info.id.as_str(),
                "name": repo_info.name,
                "root_path": repo_info.root_path,
                "initialized": false,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!(
                "Repository '{}' found at {}",
                repo_info.name,
                repo_info.root_path.display()
            );
            println!("Status: Not initialized. Run '02 init' to set up the local index.");
        }
        return Ok(());
    }

    let db = IndexDatabase::open(&db_path)?;
    let stats = db.get_stats(&repo_info.id)?;

    // Check memory store
    let store = agent_memory::MemoryStore::for_repo(&repo_info.id).ok();
    let memories = store.and_then(|s| s.load_all().ok()).unwrap_or_default();

    let fresh_memories = memories
        .iter()
        .filter(|m| m.status == agent_core::types::MemoryStatus::Fresh)
        .count();
    let degraded_memories = memories
        .iter()
        .filter(|m| m.status == agent_core::types::MemoryStatus::Degraded)
        .count();
    let stale_memories = memories
        .iter()
        .filter(|m| {
            m.status == agent_core::types::MemoryStatus::Stale
                || m.status == agent_core::types::MemoryStatus::Invalidated
        })
        .count();

    let diff_files = git_repo.get_diff_files().unwrap_or_default();
    let is_index_fresh =
        stats.last_indexed_commit == repo_info.head_commit && diff_files.is_empty();

    if json_output {
        let out = json!({
            "status": "initialized",
            "repo_id": repo_info.id.as_str(),
            "name": repo_info.name,
            "root_path": repo_info.root_path,
            "branch": repo_info.branch,
            "head_commit": repo_info.head_commit,
            "is_clean": repo_info.is_clean,
            "modified_files": repo_info.modified_count,
            "untracked_files": repo_info.untracked_count,
            "diff_files": diff_files,
            "indexed_files": stats.total_files,
            "source_files": stats.source_files,
            "test_files": stats.test_files,
            "doc_files": stats.doc_files,
            "manifest_files": stats.manifest_files,
            "total_symbols": stats.total_symbols,
            "total_references": stats.total_references,
            "last_indexed_commit": stats.last_indexed_commit,
            "last_indexed_at": stats.last_indexed_at,
            "is_index_fresh": is_index_fresh,
            "memories": {
                "total": memories.len(),
                "fresh": fresh_memories,
                "degraded": degraded_memories,
                "stale": stale_memories,
            },
            "live_session": live_env.is_live,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Repository Status");
        println!("  Repository:       {}", repo_info.name);
        println!("  Root Path:        {}", repo_info.root_path.display());
        println!("  Repo ID:          {}", repo_info.id);
        println!(
            "  Branch:           {}",
            repo_info.branch.as_deref().unwrap_or("none")
        );
        println!(
            "  HEAD Commit:      {}",
            repo_info.head_commit.as_deref().unwrap_or("none")
        );
        println!(
            "  Working Tree:     {}",
            if repo_info.is_clean {
                "Clean".to_string()
            } else {
                format!(
                    "{} modified, {} untracked",
                    repo_info.modified_count, repo_info.untracked_count
                )
            }
        );
        if !diff_files.is_empty() {
            println!("  Dirty Files:      {}", diff_files.join(", "));
        }
        println!(
            "  Indexed Files:    {} (source: {}, tests: {}, docs: {}, manifests: {})",
            stats.total_files,
            stats.source_files,
            stats.test_files,
            stats.doc_files,
            stats.manifest_files
        );
        println!(
            "  Symbols Parsed:   {} symbols, {} references",
            stats.total_symbols, stats.total_references
        );
        println!(
            "  Memories:         {} total ({} fresh, {} degraded, {} stale)",
            memories.len(),
            fresh_memories,
            degraded_memories,
            stale_memories
        );
        println!(
            "  Index Freshness:  {}",
            if is_index_fresh {
                "Fresh (synced with HEAD)".to_string()
            } else {
                "Stale (changes detected; run '02 index')".to_string()
            }
        );
        println!(
            "  Last Indexed At:  {}",
            stats
                .last_indexed_at
                .map(|t| t.to_rfc2822())
                .unwrap_or_else(|| "Never".to_string())
        );
        println!(
            "  Index Commit:     {}",
            stats.last_indexed_commit.as_deref().unwrap_or("none")
        );

        if live_env.is_live {
            println!();
            println!("  [!] NOTICE: Live ISO environment detected.");
            println!("      Indices stored in ~/.local/share are ephemeral (RAM overlayfs).");
            println!("      For persistent indexing across reboots, work inside a mounted drive.");
        }
    }

    Ok(())
}
