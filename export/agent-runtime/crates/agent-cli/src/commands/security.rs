use agent_core::{types::SecretPattern, Result};
use agent_git::{GitDiscovery, GitRepo};
use ignore::WalkBuilder;
use serde_json::json;
use std::env;
use std::fs;
use std::path::Path;

struct SecurityAudit {
    has_gitignore: bool,
    secrets_detected: Vec<String>,
    tracked_secrets: Vec<String>,
    untracked_secrets: Vec<String>,
    loose_permissions: Vec<String>,
    is_secure: bool,
}

pub fn execute(json_output: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let repo_root = GitDiscovery::find_repository_root(&current_dir)?;
    let report = audit_repository(&repo_root)?;

    if json_output {
        let out = json!({
            "status": if report.is_secure { "passed" } else { "warnings_found" },
            "has_gitignore": report.has_gitignore,
            "detected_sensitive_files": report.secrets_detected,
            "tracked_secrets_critical": report.tracked_secrets,
            "untracked_sensitive_files": report.untracked_secrets,
            "world_writable_files": report.loose_permissions,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Security Audit (02 security)");
        println!("==================================================");
        println!("Repository Root: {}", repo_root.display());
        println!(
            "  • .gitignore file: {}",
            if report.has_gitignore {
                "Present (Good)"
            } else {
                "MISSING (Warning)"
            }
        );

        if !report.tracked_secrets.is_empty() {
            println!();
            println!("  [CRITICAL] Tracked Secret Files Found in Git Index:");
            for f in &report.tracked_secrets {
                println!("    - {}", f);
            }
            println!("    Remedy: Remove with 'git rm --cached <file>' and add to .gitignore immediately.");
        } else {
            println!("  • Tracked Secrets in Git: None (Clean)");
        }

        if !report.untracked_secrets.is_empty() {
            println!();
            println!("  • Untracked Local Sensitive Files Excluded from Index:");
            for f in &report.untracked_secrets {
                println!("    - {} (Excluded by 02 Agent Runtime filter)", f);
            }
        }

        if !report.loose_permissions.is_empty() {
            println!();
            println!("  [WARN] World-Writable Files:");
            for f in &report.loose_permissions {
                println!("    - {}", f);
            }
        }

        println!();
        if report.is_secure {
            println!("Summary: Security audit PASSED. No sensitive data exposed.");
        } else {
            println!("Summary: Security audit completed with WARNINGS. See above.");
        }
        println!("==================================================");
    }

    Ok(())
}

fn audit_repository(repo_root: &Path) -> Result<SecurityAudit> {
    let git_repo = GitRepo::open(repo_root)?;
    let tracked_files = git_repo.list_tracked_files().unwrap_or_default();

    let mut secrets_detected = Vec::new();
    let mut tracked_secrets = Vec::new();
    let mut loose_permissions = Vec::new();
    let tracked_set: std::collections::HashSet<&str> =
        tracked_files.iter().map(String::as_str).collect();
    let has_gitignore = repo_root.join(".gitignore").exists();

    for rel_path in &tracked_files {
        let path = repo_root.join(rel_path);
        note_sensitive(
            &path,
            rel_path,
            true,
            &mut secrets_detected,
            &mut tracked_secrets,
            &mut loose_permissions,
        );
    }

    let mut builder = WalkBuilder::new(repo_root);
    builder.hidden(false);
    builder.git_ignore(true);
    builder.git_global(true);
    builder.git_exclude(true);
    builder.follow_links(false);
    let agent_ignore = repo_root.join(".02agentignore");
    if agent_ignore.exists() {
        builder.add_custom_ignore_filename(".02agentignore");
    }

    for entry in builder.build().filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        if path
            .components()
            .any(|component| component.as_os_str() == ".git")
        {
            continue;
        }
        let rel_path = match path.strip_prefix(repo_root) {
            Ok(relative) => relative.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        if tracked_set.contains(rel_path.as_str()) {
            continue;
        }
        note_sensitive(
            path,
            &rel_path,
            false,
            &mut secrets_detected,
            &mut tracked_secrets,
            &mut loose_permissions,
        );
    }

    if let Ok(extras) = git_repo.list_untracked_and_ignored() {
        for rel_path in extras {
            if tracked_set.contains(rel_path.as_str()) {
                continue;
            }
            let path = repo_root.join(&rel_path);
            if SecretPattern::is_secret(&path)
                && !secrets_detected.iter().any(|seen| seen == &rel_path)
            {
                secrets_detected.push(rel_path);
            }
        }
    }

    let untracked_secrets: Vec<String> = secrets_detected
        .iter()
        .filter(|path| !tracked_secrets.contains(path))
        .cloned()
        .collect();
    let is_secure = tracked_secrets.is_empty() && loose_permissions.is_empty();
    Ok(SecurityAudit {
        has_gitignore,
        secrets_detected,
        tracked_secrets,
        untracked_secrets,
        loose_permissions,
        is_secure,
    })
}

fn note_sensitive(
    path: &Path,
    rel_path: &str,
    tracked: bool,
    secrets_detected: &mut Vec<String>,
    tracked_secrets: &mut Vec<String>,
    loose_permissions: &mut Vec<String>,
) {
    if SecretPattern::is_secret(path) {
        secrets_detected.push(rel_path.to_string());
        if tracked {
            tracked_secrets.push(rel_path.to_string());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.file_type().is_file() && metadata.permissions().mode() & 0o002 != 0 {
                loose_permissions.push(rel_path.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::audit_repository;
    use git2::Repository;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn tracked_ignored_secret_fails_and_untracked_ignored_secret_does_not() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let repo = Repository::init(root).unwrap();
        fs::write(root.join(".gitignore"), ".env\nignored/\n").unwrap();
        fs::write(root.join(".env"), "TOKEN=tracked\n").unwrap();
        fs::write(root.join("src.rs"), "fn main() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(".gitignore")).unwrap();
        index.add_path(Path::new("src.rs")).unwrap();
        index.add_path(Path::new(".env")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::write(root.join("ignored/.env"), "TOKEN=local\n").unwrap();

        let report = audit_repository(root).unwrap();
        assert!(report.tracked_secrets.iter().any(|path| path == ".env"));
        assert!(report
            .untracked_secrets
            .iter()
            .any(|path| path.ends_with(".env") && path != ".env"));
        assert!(!report.is_secure);
    }
}
