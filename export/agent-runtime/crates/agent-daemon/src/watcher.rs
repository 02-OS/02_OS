use agent_core::{paths::StoragePaths, types::RepoId, Result};
use agent_git::GitRepo;
use agent_index::IndexDatabase;
use agent_memory::{index_pipeline, MemoryStore, RepoHandle, StalenessEngine, TaskRegistry};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchedState {
    pub repositories: HashSet<PathBuf>,
}

pub struct RepoWatcher {
    state_file: PathBuf,
    last_fingerprints: Mutex<HashMap<RepoId, String>>,
}

impl RepoWatcher {
    pub fn new() -> Result<Self> {
        let dir = StoragePaths::data_dir()?;
        fs::create_dir_all(&dir)?;
        Ok(Self {
            state_file: dir.join("watched_repos.json"),
            last_fingerprints: Mutex::new(HashMap::new()),
        })
    }

    pub fn load_state(&self) -> WatchedState {
        if self.state_file.exists() {
            if let Ok(text) = fs::read_to_string(&self.state_file) {
                if let Ok(state) = serde_json::from_str(&text) {
                    return state;
                }
            }
        }
        WatchedState::default()
    }

    pub fn save_state(&self, state: &WatchedState) -> Result<()> {
        let text = serde_json::to_string_pretty(state)
            .map_err(|e| agent_core::AgentError::General(format!("Serialize error: {}", e)))?;
        fs::write(&self.state_file, text)?;
        Ok(())
    }

    pub fn add(&self, path: PathBuf) -> Result<bool> {
        let mut state = self.load_state();
        let inserted = state.repositories.insert(path);
        if inserted {
            self.save_state(&state)?;
        }
        Ok(inserted)
    }

    pub fn remove(&self, path: &Path) -> Result<bool> {
        let mut state = self.load_state();
        let removed = state.repositories.remove(path);
        if removed {
            self.save_state(&state)?;
        }
        Ok(removed)
    }

    pub fn list(&self) -> Vec<PathBuf> {
        self.load_state().repositories.into_iter().collect()
    }

    /// Background tick: reindex watched repos whose HEAD or worktree fingerprint changed.
    /// Returns the number of repositories whose sync failed.
    pub fn sync_all(&self) -> usize {
        let state = self.load_state();
        let mut errors = 0;
        for repo_path in state.repositories {
            if !repo_path.exists() {
                continue;
            }
            if let Err(err) = self.sync_one(&repo_path) {
                errors += 1;
                eprintln!("02-agentd: sync {}: {err}", repo_path.display());
            }
        }
        errors
    }

    fn sync_one(&self, repo_path: &Path) -> Result<()> {
        let git_repo = GitRepo::open(repo_path)?;
        let repo_info = git_repo.info()?;
        let db_path = StoragePaths::repo_db_path(&repo_info.id)?;
        if !db_path.exists() {
            return Ok(());
        }

        let db = IndexDatabase::open(&db_path)?;
        let stats = db.get_stats(&repo_info.id)?;
        let head_changed = stats.last_indexed_commit != repo_info.head_commit;
        let current_fingerprint = git_repo.status_fingerprint()?;
        let previous_head = stats.last_indexed_commit.clone();
        let dirty_changed = {
            let fps = self.fingerprint_lock();
            match fps.get(&repo_info.id) {
                Some(fp) => fp != &current_fingerprint,
                None => repo_info.modified_count > 0 || repo_info.untracked_count > 0,
            }
        };
        drop(db);

        if !(head_changed || dirty_changed) {
            self.fingerprint_lock()
                .insert(repo_info.id, current_fingerprint);
            return Ok(());
        }

        let mut handle = RepoHandle::open(repo_path)?;
        let indexed = (|| {
            index_pipeline(&mut handle, false, &TaskRegistry::new())?;
            let store = MemoryStore::for_repo(&handle.info.id)?;
            StalenessEngine::evaluate(repo_path, &handle.info.id, &handle.git, &handle.db, &store)?;
            Ok(())
        })();
        if let Err(err) = indexed {
            let mut info = handle.info.clone();
            info.head_commit = previous_head;
            let _ = handle.db.update_repo_info(&info);
            return Err(err);
        }

        self.fingerprint_lock()
            .insert(repo_info.id, current_fingerprint);
        Ok(())
    }

    fn fingerprint_lock(&self) -> std::sync::MutexGuard<'_, HashMap<RepoId, String>> {
        match self.last_fingerprints.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
