use agent_core::{paths::StoragePaths, truncate_chars, AgentError, Result};
use agent_git::{GitDiscovery, GitRepo};
use agent_index::IndexDatabase;
use agent_skills::{discover_skills, load_skill, Skill};
use serde_json::json;
use std::env;
use std::path::Path;

pub fn execute_approvals(json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
    if !db_path.exists() {
        return Err(AgentError::RepositoryNotInitialized);
    }
    let db = IndexDatabase::open(&db_path)?;
    let approvals = db.list_approvals(&repo_info.id)?;
    let pending = db.list_pending_confirmations(&repo_info.id)?;

    if json_output {
        let out = json!({
            "repo_id": repo_info.id.as_str(),
            "count": approvals.len(),
            "pending_count": pending.len(),
            "approvals": approvals.iter().map(|a| json!({
                "id": a.id,
                "run_id": a.run_id,
                "command_id": a.command_id,
                "args_hash": a.args_hash,
                "decision": a.decision,
            })).collect::<Vec<_>>(),
            "pending": pending.iter().map(|p| json!({
                "id": p.id,
                "tool": p.tool,
                "args_hash": p.args_hash,
                "created_at": p.created_at,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Recorded Approvals");
        println!("==================================================");
        if approvals.is_empty() && pending.is_empty() {
            println!("  No approvals recorded for this repository.");
        } else {
            for p in &pending {
                println!(
                    "  [PENDING] {} tool={} hash={}",
                    p.id,
                    p.tool,
                    truncate_chars(&p.args_hash, 12)
                );
            }
            for a in &approvals {
                println!(
                    "  [{}] {} cmd={} hash={}",
                    a.decision.to_uppercase(),
                    a.id,
                    a.command_id.as_deref().unwrap_or("-"),
                    truncate_chars(&a.args_hash, 12)
                );
            }
        }
        println!("==================================================");
    }
    Ok(())
}

pub fn execute_evidence(json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let git_repo = GitRepo::open(&repo_root)?;
    let repo_info = git_repo.info()?;

    let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
    if !db_path.exists() {
        return Err(AgentError::RepositoryNotInitialized);
    }
    let db = IndexDatabase::open(&db_path)?;
    let packages = db.list_evidence_summaries(&repo_info.id)?;

    if json_output {
        let out = json!({
            "repo_id": repo_info.id.as_str(),
            "count": packages.len(),
            "evidence": packages.iter().map(|p| json!({
                "id": p.id,
                "agt_run_id": p.agt_run_id,
                "task_type": p.task_type,
                "interaction": p.interaction,
                "outcome": p.outcome,
                "package_digest": p.package_digest,
                "head_commit": p.head_commit,
                "created_at": p.created_at,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Evidence Packages");
        println!("==================================================");
        if packages.is_empty() {
            println!("  No evidence packages recorded for this repository.");
        } else {
            for p in &packages {
                println!(
                    "  • {} [{}] outcome={} digest={}",
                    p.id,
                    p.task_type,
                    p.outcome,
                    truncate_chars(&p.package_digest, 12)
                );
            }
        }
        println!("==================================================");
    }
    Ok(())
}

fn collect_repo_skills(repo_root: &Path) -> Result<Vec<Skill>> {
    let mut skills = Vec::new();
    for candidate in [
        repo_root.join(".02agent/skills"),
        repo_root.join("skills"),
        repo_root.to_path_buf(),
    ] {
        if candidate.is_dir() {
            if let Ok(found) = discover_skills(&candidate) {
                for s in found {
                    if !skills
                        .iter()
                        .any(|existing: &Skill| existing.name == s.name)
                    {
                        skills.push(s);
                    }
                }
            }
        }
    }
    Ok(skills)
}

pub fn execute_skill_list(json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root =
        GitDiscovery::find_repository_root(&current_dir).unwrap_or_else(|_| current_dir.clone());
    let skills = collect_repo_skills(&repo_root)?;

    if json_output {
        let out = json!({
            "count": skills.len(),
            "skills": skills.iter().map(|s| json!({
                "name": s.name,
                "version": s.version,
                "tools": s.tools,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Discovered Skills");
        println!("==================================================");
        if skills.is_empty() {
            println!("  No skills found in repository.");
        } else {
            for s in &skills {
                println!(
                    "  • {} (v{}) — tools: {}",
                    s.name,
                    s.version,
                    s.tools.join(", ")
                );
            }
        }
        println!("==================================================");
    }
    Ok(())
}

pub fn execute_skill_verify(path: Option<&Path>, json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let target = path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| current_dir.clone());
    let (root, rel) = if target.file_name().and_then(|n| n.to_str()) == Some("SKILL.toml") {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        (parent.to_path_buf(), Path::new("SKILL.toml").to_path_buf())
    } else {
        (target, Path::new("SKILL.toml").to_path_buf())
    };
    let skill = load_skill(&root, &rel)
        .map_err(|err| AgentError::Security(format!("skill verification failed: {err}")))?;

    if json_output {
        let out = json!({
            "verified": true,
            "name": skill.name,
            "version": skill.version,
            "tools": skill.tools,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "Verified skill '{}' (v{}) with {} tool(s).",
            skill.name,
            skill.version,
            skill.tools.len()
        );
    }
    Ok(())
}
