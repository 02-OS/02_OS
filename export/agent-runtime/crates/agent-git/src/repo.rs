use agent_core::{error::AgentError, RepoId, RepoInfo, Result};
use git2::{Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn is_tracked_change(status: git2::Status) -> bool {
    status.is_wt_modified()
        || status.is_wt_deleted()
        || status.is_wt_renamed()
        || status.is_wt_typechange()
        || status.is_index_modified()
        || status.is_index_new()
        || status.is_index_deleted()
        || status.is_index_renamed()
        || status.is_index_typechange()
        || status.is_conflicted()
}

pub struct GitRepo {
    repo: Repository,
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitInfo {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    pub timestamp: i64,
}

impl GitRepo {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let repo = Repository::discover(path.as_ref())
            .map_err(|e| AgentError::Git(format!("Failed to open repository: {}", e)))?;
        let root = repo
            .workdir()
            .ok_or_else(|| AgentError::Git("Bare repository not supported".into()))?
            .to_path_buf();
        Ok(Self { repo, root })
    }

    pub fn root_path(&self) -> &Path {
        &self.root
    }

    pub fn head_commit_id(&self) -> Result<Option<String>> {
        match self.repo.head() {
            Ok(head) => match head.target() {
                Some(oid) => Ok(Some(oid.to_string())),
                None => Ok(None),
            },
            Err(e)
                if e.code() == git2::ErrorCode::UnbornBranch
                    || e.code() == git2::ErrorCode::NotFound =>
            {
                Ok(None)
            }
            Err(e) => Err(AgentError::Git(format!("Failed to read HEAD: {}", e))),
        }
    }

    pub fn current_branch(&self) -> Result<Option<String>> {
        match self.repo.head() {
            Ok(head) => {
                if head.is_branch() {
                    Ok(head.shorthand().map(|s| s.to_string()))
                } else {
                    Ok(Some("HEAD (detached)".to_string()))
                }
            }
            Err(e)
                if e.code() == git2::ErrorCode::UnbornBranch
                    || e.code() == git2::ErrorCode::NotFound =>
            {
                Ok(None)
            }
            Err(e) => Err(AgentError::Git(format!(
                "Failed to read current branch: {}",
                e
            ))),
        }
    }

    pub fn status_summary(&self) -> Result<(bool, usize, usize)> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        opts.renames_head_to_index(true);
        opts.renames_index_to_workdir(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .map_err(|e| AgentError::Git(format!("Failed to query status: {}", e)))?;

        let mut modified = 0;
        let mut untracked = 0;

        for entry in statuses.iter() {
            let status = entry.status();
            if status.is_wt_new() {
                untracked += 1;
            } else if is_tracked_change(status) {
                modified += 1;
            }
        }

        let is_clean = modified == 0 && untracked == 0;
        Ok((is_clean, modified, untracked))
    }

    /// Generates a status fingerprint of all uncommitted files and modification times.
    /// Used by daemon watcher to prevent repeated re-indexing of unchanged dirty repositories.
    pub fn status_fingerprint(&self) -> Result<String> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        opts.renames_head_to_index(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .map_err(|e| AgentError::Git(format!("Failed to query status: {}", e)))?;

        let mut entries = Vec::new();
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                let status_bits = entry.status().bits();
                let full_path = self.root.join(path);
                let meta = std::fs::symlink_metadata(&full_path).ok();
                let mtime = meta
                    .as_ref()
                    .and_then(|metadata| metadata.modified().ok())
                    .map(|modified| format!("{modified:?}"))
                    .unwrap_or_default();
                let len = meta.as_ref().map(|metadata| metadata.len()).unwrap_or(0);
                entries.push(format!("{path}:{status_bits}:{mtime}:{len}"));
            }
        }
        entries.sort();
        Ok(entries.join(";"))
    }

    /// Untracked and ignored worktree paths, with ignored directories expanded to files.
    pub fn list_untracked_and_ignored(&self) -> Result<Vec<String>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .include_ignored(true)
            .recurse_untracked_dirs(true)
            .recurse_ignored_dirs(true);
        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .map_err(|e| AgentError::Git(format!("Failed to query status: {}", e)))?;
        let mut paths = Vec::new();
        for entry in statuses.iter() {
            let status = entry.status();
            if !(status.is_wt_new() || status.is_ignored()) {
                continue;
            }
            if let Some(path) = entry.path() {
                paths.push(path.to_string());
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub fn info(&self) -> Result<RepoInfo> {
        let id = RepoId::from_path(&self.root);
        let name = self
            .root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed_repo")
            .to_string();

        let head_commit = self.head_commit_id()?;
        let branch = self.current_branch()?;
        let (is_clean, modified_count, untracked_count) = self.status_summary()?;

        Ok(RepoInfo {
            id,
            name,
            root_path: self.root.clone(),
            head_commit,
            branch,
            is_clean,
            modified_count,
            untracked_count,
        })
    }

    /// List relative paths of all files tracked by Git HEAD / index.
    pub fn list_tracked_files(&self) -> Result<Vec<String>> {
        let index = self
            .repo
            .index()
            .map_err(|e| AgentError::Git(format!("Failed to get index: {}", e)))?;

        let mut files = Vec::new();
        for entry in index.iter() {
            if let Ok(path) = std::str::from_utf8(&entry.path) {
                files.push(path.to_string());
            }
        }
        Ok(files)
    }

    /// Look up the Git blob OID for a relative file path.
    pub fn get_blob_id(&self, relative_path: &str) -> Result<Option<String>> {
        let index = self
            .repo
            .index()
            .map_err(|e| AgentError::Git(format!("Failed to get index: {}", e)))?;

        if let Some(entry) = index.get_path(Path::new(relative_path), 0) {
            Ok(Some(entry.id.to_string()))
        } else {
            Ok(None)
        }
    }

    /// Retrieve the recent commit log.
    pub fn get_recent_commits(&self, limit: usize) -> Result<Vec<GitCommitInfo>> {
        let mut revwalk = match self.repo.revwalk() {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        if revwalk.push_head().is_err() {
            return Ok(Vec::new());
        }

        let mut commits = Vec::new();
        for id in revwalk.take(limit) {
            let oid = match id {
                Ok(o) => o,
                Err(_) => continue,
            };
            if let Ok(commit) = self.repo.find_commit(oid) {
                let summary = commit.summary().unwrap_or("").to_string();
                let author = commit.author().name().unwrap_or("Unknown").to_string();
                let timestamp = commit.time().seconds();
                let hex = oid.to_string();
                let short_id = if hex.len() >= 7 {
                    hex[..7].to_string()
                } else {
                    hex.clone()
                };

                commits.push(GitCommitInfo {
                    id: hex,
                    short_id,
                    summary,
                    author,
                    timestamp,
                });
            }
        }

        Ok(commits)
    }

    /// Files modified or staged in the working tree compared to HEAD.
    pub fn get_diff_files(&self) -> Result<Vec<String>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(false);
        opts.renames_head_to_index(true);
        opts.renames_index_to_workdir(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .map_err(|e| AgentError::Git(format!("Failed to query diff statuses: {}", e)))?;

        let mut diff_files = Vec::new();
        for entry in statuses.iter() {
            let status = entry.status();
            if is_tracked_change(status) {
                if let Some(p) = entry.path() {
                    diff_files.push(p.to_string());
                }
            }
        }
        Ok(diff_files)
    }

    /// Retrieve hunk line ranges for all modified files in working tree vs HEAD.
    pub fn get_diff_hunks(&self) -> Result<Vec<FileDiffHunks>> {
        let head_tree = match self.repo.head() {
            Ok(head) => head.peel_to_tree().ok(),
            Err(_) => None,
        };

        let mut diff_opts = git2::DiffOptions::new();
        diff_opts.include_untracked(false);

        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut diff_opts))
            .map_err(|e| AgentError::Git(format!("Failed to build diff: {}", e)))?;

        let file_hunks = std::cell::RefCell::new(Vec::<FileDiffHunks>::new());

        diff.foreach(
            &mut |delta, _| {
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let is_deleted = delta.status() == git2::Delta::Deleted;
                let is_new = delta.status() == git2::Delta::Added;

                file_hunks.borrow_mut().push(FileDiffHunks {
                    file_path: path,
                    is_deleted,
                    is_new,
                    hunks: Vec::new(),
                });
                true
            },
            None,
            Some(&mut |_, hunk| {
                let mut borrowed = file_hunks.borrow_mut();
                if let Some(last) = borrowed.last_mut() {
                    last.hunks.push(DiffHunk {
                        old_start: hunk.old_start() as usize,
                        old_lines: hunk.old_lines() as usize,
                        new_start: hunk.new_start() as usize,
                        new_lines: hunk.new_lines() as usize,
                    });
                }
                true
            }),
            None,
        )
        .map_err(|e| AgentError::Git(format!("Failed to iterate diff: {}", e)))?;

        Ok(file_hunks.into_inner())
    }

    /// Paths added, modified, or deleted by one commit compared with its first parent.
    pub fn paths_touched_by_commit(&self, commit_id: &str) -> Result<Vec<String>> {
        let oid = git2::Oid::from_str(commit_id)
            .map_err(|e| AgentError::Git(format!("Invalid commit id: {}", e)))?;
        let commit = self
            .repo
            .find_commit(oid)
            .map_err(|e| AgentError::Git(format!("Commit not found: {}", e)))?;
        let new_tree = commit
            .tree()
            .map_err(|e| AgentError::Git(format!("Commit tree missing: {}", e)))?;
        let old_tree = commit.parent(0).ok().and_then(|parent| parent.tree().ok());
        let diff = self
            .repo
            .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)
            .map_err(|e| AgentError::Git(format!("Commit diff failed: {}", e)))?;

        let mut paths = Vec::new();
        for delta in diff.deltas() {
            if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                paths.push(path.to_string_lossy().to_string());
            }
        }
        Ok(paths)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiffHunks {
    pub file_path: String,
    pub is_deleted: bool,
    pub is_new: bool,
    pub hunks: Vec<DiffHunk>,
}
