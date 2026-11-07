use agent_context::ContextCompiler;
use agent_core::Result;
use agent_git::GitDiscovery;
use std::env;

pub fn execute(task: &str, budget: usize, json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;

    let package = ContextCompiler::compile(&repo_root, task, budget)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&package)?);
    } else {
        println!("02 Agent Runtime: Context Compiler");
        println!("Task: \"{}\"", package.task);
        println!();

        println!("Relevant implementation");
        if package.relevant_files.is_empty() {
            println!("  (none found)");
        } else {
            for f in &package.relevant_files {
                let mut unique_reasons = f.reasons.clone();
                unique_reasons.sort();
                unique_reasons.dedup();
                let reason = if !unique_reasons.is_empty() {
                    let preview: Vec<_> = unique_reasons.into_iter().take(2).collect();
                    format!(" ({})", preview.join(", "))
                } else {
                    String::new()
                };
                println!("  {}{}", f.relative_path, reason);
            }
        }
        println!();

        if !package.relevant_symbols.is_empty() {
            println!("Relevant symbols");
            for s in &package.relevant_symbols {
                let sig = s.symbol.signature.as_deref().unwrap_or(&s.symbol.name);
                println!("  {} [{}:{}]", sig, s.symbol.file_path, s.symbol.start_line);
            }
            println!();
        }

        println!("Relevant tests");
        if package.relevant_tests.is_empty() {
            println!("  (none found)");
        } else {
            for t in &package.relevant_tests {
                println!("  {} (for symbol '{}')", t.test_file, t.target_symbol);
            }
        }
        println!();

        println!("Dependencies");
        if package.dependencies.is_empty() {
            println!("  (none detected)");
        } else {
            for d in &package.dependencies {
                println!("  {} -> {} [{}]", d.caller, d.callee, d.relationship);
            }
        }
        println!();

        println!("Prior verified context");
        if package.verified_memories.is_empty() {
            println!("  (none recorded)");
        } else {
            for m in &package.verified_memories {
                println!("  {}", m.claim);
                for ev in &m.evidence {
                    let commit_short = if ev.commit.len() >= 7 {
                        &ev.commit[..7]
                    } else {
                        &ev.commit
                    };
                    println!("  source: {} (commit {})", ev.file, commit_short);
                }
            }
        }
        println!();

        if !package.diagnostic_memories.is_empty() {
            println!("Diagnostic context (not verified)");
            for m in &package.diagnostic_memories {
                println!("  {}", m.claim);
            }
            println!();
        }

        println!("Git information");
        if package.git_context.modified_files.is_empty() {
            println!("  clean working tree");
        } else {
            println!(
                "  files modified in working tree: {}",
                package.git_context.modified_files.join(", ")
            );
        }
        if !package.git_context.recent_commits.is_empty() {
            println!("  recent commits touching context:");
            for c in package.git_context.recent_commits.iter().take(3) {
                let short_id = if c.commit_id.len() >= 7 {
                    &c.commit_id[..7]
                } else {
                    &c.commit_id
                };
                println!("    {} {}", short_id, c.summary);
            }
        }
        println!();

        println!("Staleness");
        println!("  {}", package.staleness);
        println!();

        println!("Context budget");
        println!("  candidate: {} tokens", package.budget.candidate_tokens);
        println!(
            "  returned:  {} tokens (budget: {})",
            package.budget.returned_tokens, package.budget.budget_limit
        );
    }

    Ok(())
}
