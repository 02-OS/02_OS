use agent_core::{paths::StoragePaths, types::LiveEnvironmentInfo, Result};
use agent_git::{GitDiscovery, GitRepo};
use agent_index::IndexDatabase;
use serde_json::json;
use std::env;
use std::fs;
use std::process::Command;

pub fn execute(json_output: bool) -> Result<()> {
    // Effective UID, not $USER: sudo often preserves the invoking user's name.
    #[cfg(unix)]
    let is_root = unsafe { libc::geteuid() } == 0;
    #[cfg(not(unix))]
    let is_root = false;

    // 2. Git check
    let git_status = Command::new("git").arg("--version").output();
    let git_version = match git_status {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "Not installed".to_string(),
    };

    // 3. Storage path check
    let data_dir = StoragePaths::data_dir()?;
    let storage_writable = match fs::create_dir_all(&data_dir) {
        Ok(_) => {
            let test_file = data_dir.join(".doctor_write_test");
            if fs::write(&test_file, b"ok").is_ok() {
                let _ = fs::remove_file(test_file);
                true
            } else {
                false
            }
        }
        Err(_) => false,
    };

    // 4. Live ISO check
    let live_env = LiveEnvironmentInfo::detect();

    // 5. Current repository check
    let current_dir = env::current_dir()?;
    let repo_check = GitDiscovery::find_repository_root(&current_dir)
        .ok()
        .and_then(|root| {
            let repo = GitRepo::open(&root).ok()?;
            let info = repo.info().ok()?;
            let db_path = StoragePaths::repo_db_path(&info.id).ok()?;
            let initialized = db_path.exists();
            Some((info, initialized, db_path))
        });

    if json_output {
        let out = json!({
            "is_root": is_root,
            "git_version": git_version,
            "storage_dir": data_dir,
            "storage_writable": storage_writable,
            "live_environment": live_env,
            "in_repository": repo_check.is_some(),
            "repository": repo_check.map(|(info, init, db)| json!({
                "id": info.id.as_str(),
                "name": info.name,
                "root": info.root_path,
                "initialized": init,
                "db_path": db,
            }))
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("02 Agent Runtime: Diagnostic Report (02 doctor)");
        println!("==================================================");
        println!("System & Environment:");
        println!(
            "  • Privileges:       {}",
            if is_root {
                "Running as ROOT (Warning: analysis should run as normal user!)"
            } else {
                "Normal non-root user (OK)"
            }
        );
        println!("  • Git Engine:       {}", git_version);
        println!(
            "  • XDG Data Dir:     {} (Writable: {})",
            data_dir.display(),
            if storage_writable {
                "Yes"
            } else {
                "No [FAILED]"
            }
        );
        println!(
            "  • Distribution:     {}",
            if live_env.is_live {
                "02_OS Live ISO"
            } else {
                "02_OS Installed System"
            }
        );
        if live_env.is_live {
            println!("    [Notice]          {}", live_env.details);
        }

        println!();
        println!("Repository Context:");
        if let Some((info, initialized, db_path)) = repo_check {
            println!("  • Detected Repo:    {}", info.name);
            println!("  • Path:             {}", info.root_path.display());
            println!("  • Repo ID:          {}", info.id);
            println!(
                "  • Initialized:      {}",
                if initialized {
                    "Yes (Database ready)"
                } else {
                    "No (Run '02 init')"
                }
            );
            if initialized {
                println!("  • Database:         {}", db_path.display());
                if let Ok(db) = IndexDatabase::open(&db_path) {
                    if let Ok(stats) = db.get_stats(&info.id) {
                        println!("  • Indexed Files:    {}", stats.total_files);
                    }
                }
            }
        } else {
            println!("  • Current Dir:      Not inside a Git repository.");
        }
        println!("==================================================");
    }

    Ok(())
}
