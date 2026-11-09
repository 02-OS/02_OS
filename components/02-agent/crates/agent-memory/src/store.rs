use agent_core::{
    paths::StoragePaths,
    types::{
        EvidenceItem, EvidenceMemory, MemoryKind, MemoryStatus, RepoId, RepoInfo, SecretPattern,
    },
    AgentError, Result,
};
use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

pub struct MemoryStore {
    path: PathBuf,
}

impl MemoryStore {
    pub fn for_repo(repo_id: &RepoId) -> Result<Self> {
        let path = StoragePaths::repo_memories_path(repo_id)?;
        Ok(Self { path })
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load all evidence memories from memories.jsonl.
    pub fn load_all(&self) -> Result<Vec<EvidenceMemory>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = fs::File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut memories = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(mut mem) = serde_json::from_str::<EvidenceMemory>(&line) {
                // Older stores may contain raw claims. Never expose those through any reader.
                mem.claim = SecretPattern::redact(&mem.claim);
                memories.push(mem);
            }
        }

        Ok(memories)
    }

    /// Find memory by ID.
    pub fn get(&self, id: &str) -> Result<Option<EvidenceMemory>> {
        let all = self.load_all()?;
        Ok(all.into_iter().find(|m| m.id == id))
    }

    /// Insert or update a memory in memories.jsonl atomically.
    pub fn save(&self, memory: &EvidenceMemory) -> Result<()> {
        let _lock = self.lock()?;
        self.save_unlocked(memory)
    }

    /// Replace `memory.id` only when its stored `updated_at` is still `seen`.
    ///
    /// A concurrent writer that stored a newer timestamp wins. This does not
    /// resurrect a row the caller has not loaded.
    pub fn save_if_unchanged(
        &self,
        memory: &EvidenceMemory,
        seen_updated_at: chrono::DateTime<Utc>,
    ) -> Result<()> {
        let _lock = self.lock()?;
        let mut memory = memory.clone();
        memory.claim = SecretPattern::redact(&memory.claim);
        let mut all = self.load_all()?;
        if let Some(pos) = all.iter().position(|existing| existing.id == memory.id) {
            if all[pos].updated_at != seen_updated_at {
                return Err(AgentError::General(format!(
                    "stale memory write for {} refused",
                    memory.id
                )));
            }
            all[pos] = memory;
        } else {
            all.push(memory);
        }
        self.write_all(&all)
    }

    fn save_unlocked(&self, memory: &EvidenceMemory) -> Result<()> {
        let mut memory = memory.clone();
        memory.claim = SecretPattern::redact(&memory.claim);
        let mut all = self.load_all()?;
        if let Some(pos) = all.iter().position(|m| m.id == memory.id) {
            all[pos] = memory.clone();
        } else {
            all.push(memory.clone());
        }

        self.write_all(&all)
    }

    /// Delete a memory by ID.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let _lock = self.lock()?;
        let mut all = self.load_all()?;
        let initial_len = all.len();
        all.retain(|m| m.id != id);

        if all.len() != initial_len {
            self.write_all(&all)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn lock(&self) -> Result<ExclusiveLock> {
        ExclusiveLock::acquire(&lock_path(&self.path))
    }

    fn write_all(&self, memories: &[EvidenceMemory]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp_path = self.path.with_extension("jsonl.tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)?;

            for mem in memories {
                let mut sanitized = mem.clone();
                sanitized.claim = SecretPattern::redact(&sanitized.claim);
                let serialized = serde_json::to_string(&sanitized)?;
                writeln!(file, "{}", serialized)?;
            }
            file.flush()?;
        }

        fs::rename(temp_path, &self.path)?;
        Ok(())
    }

    /// Automatically discover foundational architectural facts if memories are currently empty.
    pub fn seed_initial_if_empty(&self, repo_info: &RepoInfo) -> Result<usize> {
        let _lock = self.lock()?;
        let existing = self.load_all()?;
        if !existing.is_empty() {
            return Ok(0);
        }

        let commit = repo_info
            .head_commit
            .as_deref()
            .unwrap_or("initial")
            .to_string();
        let mut seeded = Vec::new();

        // 1. Check for ArchISO profiledef.sh (02_OS distribution anchor)
        let profiledef = repo_info.root_path.join("profile/profiledef.sh");
        if profiledef.exists() {
            seeded.push(EvidenceMemory {
                id: "mem_archiso_profile".to_string(),
                claim: "02_OS is a live image built with mkarchiso from profile/".to_string(),
                kind: MemoryKind::ArchitecturalFact,
                evidence: vec![EvidenceItem {
                    file: "profile/profiledef.sh".to_string(),
                    symbols: vec!["iso_name".to_string(), "airootfs_image_type".to_string()],
                    commit: commit.clone(),
                    fingerprint: None,
                }],
                valid_at: commit.clone(),
                confidence: 1.0,
                status: MemoryStatus::Fresh,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        }

        // 2. Check for Cargo workspace. The split tree vendors the runtime.
        let cargo_rel = [
            "components/02-agent/Cargo.toml",
            "vendor/agent-runtime/Cargo.toml",
        ]
        .into_iter()
        .find(|rel| repo_info.root_path.join(rel).is_file());
        if let Some(cargo_rel) = cargo_rel {
            let cargo_dir = cargo_rel
                .trim_end_matches("/Cargo.toml")
                .trim_end_matches("Cargo.toml");
            seeded.push(EvidenceMemory {
                id: "mem_agent_runtime".to_string(),
                claim: format!(
                    "02 Agent Runtime is implemented as a modular Rust workspace in {cargo_dir}/ providing native Git & SQLite repository intelligence"
                ),
                kind: MemoryKind::ArchitecturalFact,
                evidence: vec![EvidenceItem {
                    file: cargo_rel.to_string(),
                    symbols: vec!["agent-core".to_string(), "agent-index".to_string()],
                    commit: commit.clone(),
                    fingerprint: None,
                }],
                valid_at: commit.clone(),
                confidence: 1.0,
                status: MemoryStatus::Fresh,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        }

        for mem in &mut seeded {
            for item in &mut mem.evidence {
                match crate::MemoryVerifier::capture_evidence(
                    &repo_info.root_path,
                    &item.file,
                    item.symbols.clone(),
                    &item.commit,
                ) {
                    Ok(captured) => *item = captured,
                    Err(_) => {
                        mem.status = MemoryStatus::Stale;
                        mem.confidence = 0.0;
                    }
                }
            }
            self.save_unlocked(mem)?;
        }

        Ok(seeded.len())
    }
}

fn lock_path(path: &Path) -> PathBuf {
    let mut file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("memories.jsonl"))
        .to_os_string();
    file_name.push(".lock");
    path.with_file_name(file_name)
}

struct ExclusiveLock {
    file: fs::File,
}

impl ExclusiveLock {
    fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(AgentError::Io(std::io::Error::last_os_error()));
        }
        Ok(Self { file })
    }
}

impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
