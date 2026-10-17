use agent_core::{paths::StoragePaths, types::RepoId, Result};
use agent_git::GitRepo;
use agent_index::{IndexDatabase, RepoScanner};
use agent_memory::{index_pipeline, RepoHandle, TaskRegistry};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchedState {
    pub repositories: HashSet<PathBuf>,
    #[serde(default)]
    pub opted_out_repositories: HashSet<PathBuf>,
}

pub struct RepoWatcher {
    state_file: PathBuf,
    last_fingerprints: Mutex<HashMap<RepoId, String>>,
    /// Serializes the watched_repos.json read-modify-write.
    state_mu: Mutex<()>,
}

struct FileLock {
    file: File,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

impl RepoWatcher {
    pub fn new() -> Result<Self> {
        let dir = StoragePaths::data_dir()?;
        StoragePaths::ensure_private_dir(&dir)?;
        Self::with_state_file(dir.join("watched_repos.json"))
    }

    /// Watch state stored at `state_file` (parent directory is created).
    pub fn with_state_file(state_file: PathBuf) -> Result<Self> {
        if let Some(parent) = state_file.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        Ok(Self {
            state_file,
            last_fingerprints: Mutex::new(HashMap::new()),
            state_mu: Mutex::new(()),
        })
    }

    pub fn load_state(&self) -> Result<WatchedState> {
        let _mu = lock_mutex(&self.state_mu);
        let _file = lock_file(&self.state_file)?;
        read_state(&self.state_file)
    }

    pub fn save_state(&self, state: &WatchedState) -> Result<()> {
        let _mu = lock_mutex(&self.state_mu);
        let _file = lock_file(&self.state_file)?;
        write_state(&self.state_file, state)
    }

    pub fn add(&self, path: PathBuf) -> Result<bool> {
        self.register(path, true)
    }

    /// Automatic setup respects an explicit unwatch, including after restart.
    pub fn register_automatic(&self, path: PathBuf) -> Result<bool> {
        self.register(path, false)
    }

    fn register(&self, path: PathBuf, explicit: bool) -> Result<bool> {
        let _mu = lock_mutex(&self.state_mu);
        let _file = lock_file(&self.state_file)?;
        let mut state = read_state(&self.state_file)?;
        if !explicit && state.opted_out_repositories.contains(&path) {
            return Ok(false);
        }
        let enabled = explicit && state.opted_out_repositories.remove(&path);
        let inserted = state.repositories.insert(path);
        if inserted || enabled {
            write_state(&self.state_file, &state)?;
        }
        Ok(inserted)
    }

    pub fn remove(&self, path: &Path) -> Result<bool> {
        let _mu = lock_mutex(&self.state_mu);
        let _file = lock_file(&self.state_file)?;
        let mut state = read_state(&self.state_file)?;
        let removed = state.repositories.remove(path);
        let disabled = state.opted_out_repositories.insert(path.to_path_buf());
        if removed || disabled {
            write_state(&self.state_file, &state)?;
        }
        Ok(removed)
    }

    pub fn list(&self) -> Result<Vec<PathBuf>> {
        Ok(self.load_state()?.repositories.into_iter().collect())
    }

    /// Reconcile each indexed repo once on startup, then sync changed HEAD/worktree fingerprints.
    /// Returns the number of repositories whose sync failed.
    pub fn sync_all(&self) -> usize {
        let state = match self.load_state() {
            Ok(state) => state,
            Err(err) => {
                eprintln!("02-agentd: failed to load watch state: {err}");
                return 1;
            }
        };
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
        let completed = db.completed_index(&repo_info.id)?;
        let head_changed = completed
            .as_ref()
            .is_none_or(|state| state.head_commit != repo_info.head_commit);
        let config = agent_core::RepoConfig::load_or_default(repo_path);
        let current_fingerprint = RepoScanner::new(repo_path, config).fingerprint();
        let previous_head = stats.last_indexed_commit.clone();
        let dirty_changed = {
            let fps = self.fingerprint_lock();
            match fps.get(&repo_info.id) {
                Some(fp) => {
                    fp != &current_fingerprint
                        || completed
                            .as_ref()
                            .is_none_or(|state| state.fingerprint != current_fingerprint)
                }
                // A clean current worktree can differ from the indexed snapshot: an
                // untracked file may have been deleted, or dirty tracked content
                // reverted, while the daemon was stopped. Establish the baseline
                // only after an initial successful reconciliation.
                None => true,
            }
        };
        drop(db);

        if !(head_changed || dirty_changed) {
            self.fingerprint_lock()
                .insert(repo_info.id, current_fingerprint);
            return Ok(());
        }

        let mut handle = RepoHandle::open(repo_path)?;
        let indexed = index_pipeline(&mut handle, false, &TaskRegistry::new());
        if let Err(err) = indexed {
            handle.db.invalidate_index(&handle.info.id)?;
            let mut info = handle.info.clone();
            info.head_commit = previous_head;
            let _ = handle.db.update_repo_info(&info);
            return Err(err);
        }

        self.fingerprint_lock()
            .insert(repo_info.id, current_fingerprint);
        Ok(())
    }

    fn fingerprint_lock(&self) -> MutexGuard<'_, HashMap<RepoId, String>> {
        match self.last_fingerprints.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

fn lock_mutex(mutex: &Mutex<()>) -> MutexGuard<'_, ()> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn lock_file(state_file: &Path) -> Result<FileLock> {
    let path = lock_path_for(state_file);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .truncate(false)
        .open(&path)?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(FileLock { file })
}

fn lock_path_for(state_file: &Path) -> PathBuf {
    let mut name = state_file.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    match state_file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

fn read_state(state_file: &Path) -> Result<WatchedState> {
    match fs::read_to_string(state_file) {
        Ok(text) => {
            if text.trim().is_empty() {
                return Err(agent_core::AgentError::General(format!(
                    "watch state {} is empty",
                    state_file.display()
                )));
            }
            serde_json::from_str(&text).map_err(|err| {
                agent_core::AgentError::General(format!(
                    "watch state {} is malformed: {err}",
                    state_file.display()
                ))
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(WatchedState::default()),
        Err(err) => Err(err.into()),
    }
}

fn write_state(state_file: &Path, state: &WatchedState) -> Result<()> {
    let text = serde_json::to_string_pretty(state)
        .map_err(|err| agent_core::AgentError::General(format!("Serialize error: {err}")))?;
    let parent = state_file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(
        ".{}.{}.{}.tmp",
        state_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("watched_repos.json"),
        std::process::id(),
        seq
    ));
    let write_result = (|| -> Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = fs::rename(&tmp, state_file) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    File::open(parent)?.sync_all()?;
    Ok(())
}
