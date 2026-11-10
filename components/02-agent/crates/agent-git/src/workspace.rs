use agent_core::{error::AgentError, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// One Git worktree that belongs to a local workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMember {
    pub name: String,
    pub root: PathBuf,
}

/// Repositories at `root` and in its immediate children.
///
/// A child is included only when it contains a `.git` entry and its
/// canonical path stays inside `root`. The scan stops at 32 repositories.
pub fn discover_workspace(root: &Path) -> Result<Vec<WorkspaceMember>> {
    const MAX_REPOS: usize = 32;
    let root = root
        .canonicalize()
        .map_err(|err| AgentError::Git(format!("workspace root is unavailable: {err}")))?;
    let mut found = Vec::new();
    consider(&mut found, &root, &root, MAX_REPOS)?;
    let entries = fs::read_dir(&root)
        .map_err(|err| AgentError::Git(format!("workspace listing failed: {err}")))?;
    for entry in entries.flatten() {
        if found.len() >= MAX_REPOS {
            break;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(&root) {
            continue;
        }
        consider(&mut found, &root, &canonical, MAX_REPOS)?;
    }
    Ok(found)
}

fn consider(
    found: &mut Vec<WorkspaceMember>,
    workspace: &Path,
    path: &Path,
    max: usize,
) -> Result<()> {
    if found.len() >= max || found.iter().any(|member| member.root == path) {
        return Ok(());
    }
    if !path.join(".git").exists() {
        return Ok(());
    }
    if !path.starts_with(workspace) {
        return Ok(());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo")
        .to_string();
    found.push(WorkspaceMember {
        name,
        root: path.to_path_buf(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Repository;
    use std::os::unix::fs::symlink;

    #[test]
    fn discovery_keeps_child_repositories_and_skips_an_outside_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        Repository::init(root).unwrap();
        let child = root.join("libs");
        std::fs::create_dir(&child).unwrap();
        Repository::init(&child).unwrap();
        std::fs::create_dir(root.join("notes")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        Repository::init(outside.path()).unwrap();
        symlink(outside.path(), root.join("escape")).unwrap();

        let members = discover_workspace(root).unwrap();
        assert!(members
            .iter()
            .any(|member| member.root == root.canonicalize().unwrap()));
        assert!(members.iter().any(|member| member.root.ends_with("libs")));
        assert!(!members.iter().any(|member| member.root.ends_with("notes")));
        assert!(!members
            .iter()
            .any(|member| member.root.starts_with(outside.path())));
    }
}
