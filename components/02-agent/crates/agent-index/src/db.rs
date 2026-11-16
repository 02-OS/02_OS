use crate::scanner::ScannedFile;
use crate::schema::{
    CURRENT_SCHEMA_VERSION, SCHEMA_V1_SQL, SCHEMA_V2_SQL, SCHEMA_V4_SQL, SCHEMA_V5_SQL,
};
use crate::search::{FtsSearcher, SearchResult};
use agent_core::{
    error::AgentError,
    types::{
        FileKind, IndexedFile, Language, ReferenceKind, RepoId, RepoInfo, Symbol, SymbolKind,
        SymbolReference,
    },
    Result,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct IndexDatabase {
    pub(crate) conn: Connection,
}

#[derive(Debug, Clone)]
pub struct CompletedIndexState {
    pub head_commit: Option<String>,
    pub fingerprint: String,
    pub phase: String,
}

/// Serializes multi-transaction indexing pipelines, including separate processes.
pub struct IndexRunGuard(Option<fs::File>);

impl Drop for IndexRunGuard {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        if let Some(file) = &self.0 {
            unsafe {
                libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub repo_id: String,
    pub total_files: usize,
    pub source_files: usize,
    pub test_files: usize,
    pub doc_files: usize,
    pub manifest_files: usize,
    pub config_files: usize,
    pub total_symbols: usize,
    pub total_references: usize,
    pub last_indexed_commit: Option<String>,
    pub last_indexed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDepsResult {
    pub symbol_name: String,
    pub defined_in: Vec<Symbol>,
    pub callers: Vec<SymbolReference>,
    pub callees: Vec<SymbolReference>,
    pub imports: Vec<SymbolReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingConfirmation {
    pub id: String,
    pub repo_id: String,
    pub tool: String,
    pub args_hash: String,
    pub args_canonical: String,
    pub nonce: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredActor {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub peer_uid: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTokenCount {
    pub tool: String,
    pub invocations: usize,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvidenceSummary {
    pub id: String,
    pub agt_run_id: String,
    pub task_type: String,
    pub interaction: String,
    pub outcome: String,
    pub package_digest: String,
    pub head_commit: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSummary {
    pub calls_count: usize,
    pub imports_count: usize,
    pub tests_count: usize,
    pub top_imported: Vec<(String, usize)>,
}

impl IndexDatabase {
    pub fn open<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path.as_ref())
            .map_err(|e| AgentError::Database(format!("Cannot open SQLite database: {}", e)))?;

        // Enable foreign keys, WAL mode, and busy timeout for high concurrency
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA cache_size = -64000;",
        )
        .map_err(|e| AgentError::Database(format!("Pragma setup failed: {}", e)))?;

        let mut db = Self { conn };
        db.init_schema()?;
        restrict_sqlite_files(db_path.as_ref());
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AgentError::Database(format!("Cannot open in-memory database: {}", e)))?;

        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA synchronous = OFF;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|e| AgentError::Database(format!("Pragma setup failed: {}", e)))?;

        let mut db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&mut self) -> Result<()> {
        let existing = stored_schema_version(&self.conn)?;
        if let Some(found) = existing {
            if found > CURRENT_SCHEMA_VERSION {
                return Err(AgentError::SchemaVersion {
                    found,
                    supported: CURRENT_SCHEMA_VERSION,
                });
            }
            // schema_version is the authority. A current catalog must open
            // without a write: rewriting user_version takes a lock and makes
            // `02 symbol` fail while the daemon's index transaction is open.
            if found == CURRENT_SCHEMA_VERSION {
                return Ok(());
            }
        }
        // SCHEMA_V5_SQL rebuilds `files` and is not safe to run twice.
        let apply_v5 = existing.map(|version| version < 5).unwrap_or(true);
        let sidecar = if matches!(existing, Some(version) if version < 5) {
            self.snapshot_pre_v5()?
        } else {
            None
        };

        let tx = self
            .conn
            .transaction()
            .map_err(|e| AgentError::Database(format!("Failed to start transaction: {}", e)))?;

        tx.execute_batch(SCHEMA_V1_SQL)
            .map_err(|e| AgentError::Database(format!("Schema initialization failed: {}", e)))?;
        tx.execute_batch(SCHEMA_V2_SQL)
            .map_err(|e| AgentError::Database(format!("Schema v2 initialization failed: {}", e)))?;
        // v3 carries exact callers without discarding existing catalogs. Older rows
        // remain readable and their files are re-extracted at the next sync.
        let has_source_identity = {
            let mut stmt = tx
                .prepare("PRAGMA table_info(symbol_references)")
                .map_err(|e| {
                    AgentError::Database(format!("Inspect reference schema failed: {e}"))
                })?;
            let columns = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| AgentError::Database(format!("Read reference schema failed: {e}")))?;
            columns
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| AgentError::Database(format!("Reference column failed: {e}")))?
                .iter()
                .any(|name| name == "source_symbol_id")
        };
        if !has_source_identity {
            tx.execute_batch("ALTER TABLE symbol_references ADD COLUMN source_symbol_id TEXT;")
                .map_err(|e| {
                    AgentError::Database(format!("Schema v3 initialization failed: {e}"))
                })?;
        }
        crate::graph::migrate_unscoped_commit_nodes(&tx)?;
        tx.execute_batch(SCHEMA_V4_SQL)
            .map_err(|e| AgentError::Database(format!("Schema v4 initialization failed: {e}")))?;
        if apply_v5 {
            tx.execute_batch(SCHEMA_V5_SQL).map_err(|e| {
                AgentError::Database(format!("Schema v5 initialization failed: {e}"))
            })?;
        }

        let version = stored_schema_version(&tx)?;
        match version {
            None => {
                tx.execute(
                    "INSERT INTO schema_version (version) VALUES (?1)",
                    params![CURRENT_SCHEMA_VERSION],
                )
                .map_err(|e| AgentError::Database(format!("Version update failed: {}", e)))?;
            }
            Some(found) if found < CURRENT_SCHEMA_VERSION => {
                tx.execute(
                    "UPDATE schema_version SET version = ?1",
                    params![CURRENT_SCHEMA_VERSION],
                )
                .map_err(|e| AgentError::Database(format!("Version update failed: {}", e)))?;
            }
            Some(found) if found > CURRENT_SCHEMA_VERSION => {
                return Err(AgentError::SchemaVersion {
                    found,
                    supported: CURRENT_SCHEMA_VERSION,
                });
            }
            Some(_) => {}
        }

        tx.execute_batch("PRAGMA user_version = 5;")
            .map_err(|e| AgentError::Database(format!("user_version update failed: {e}")))?;

        tx.commit()
            .map_err(|e| AgentError::Database(format!("Failed to commit schema: {}", e)))?;

        if apply_v5 {
            let check: String = self
                .conn
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))
                .map_err(|e| AgentError::Database(format!("integrity_check failed: {e}")))?;
            if check != "ok" {
                return Err(AgentError::Database(format!(
                    "integrity_check after schema v5 returned {check}"
                )));
            }
            if let Some(path) = sidecar {
                fs::remove_file(&path).map_err(|e| {
                    AgentError::Database(format!("remove schema v5 snapshot failed: {e}"))
                })?;
            }
        }

        Ok(())
    }

    /// Consistent copy of a catalog whose stored version is below 5.
    /// An existing sidecar is left untouched. In-memory databases have no file to copy.
    fn snapshot_pre_v5(&self) -> Result<Option<PathBuf>> {
        let Some(db_path) = self
            .conn
            .path()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
        else {
            return Ok(None);
        };
        let sidecar = pre_v5_sidecar(&db_path);
        if sidecar.symlink_metadata().is_ok() {
            return Ok(None);
        }
        let text = sidecar.to_str().ok_or_else(|| {
            AgentError::Database("schema v5 snapshot path is not UTF-8".to_string())
        })?;
        if text
            .chars()
            .any(|ch| matches!(ch, '\'' | '\n' | '\r' | '\0'))
        {
            return Err(AgentError::Database(
                "schema v5 snapshot path contains a quote, newline, or NUL".to_string(),
            ));
        }
        self.conn
            .execute_batch(&format!("VACUUM INTO '{text}'"))
            .map_err(|e| AgentError::Database(format!("schema v5 snapshot failed: {e}")))?;
        let mut permissions = fs::metadata(&sidecar)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&sidecar, permissions)?;
        Ok(Some(sidecar))
    }

    pub fn schema_version(&self) -> Result<i32> {
        self.conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .map_err(|e| AgentError::Database(format!("Schema version read failed: {}", e)))
    }

    pub fn lock_index(&self) -> Result<IndexRunGuard> {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let Some(path) = self.conn.path().filter(|path| !path.is_empty()) else {
            return Ok(IndexRunGuard(None));
        };
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(format!("{path}.index.lock"))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(IndexRunGuard(Some(file)))
    }

    pub fn invalidate_index(&self, repo_id: &RepoId) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM completed_index_state WHERE repo_id = ?1",
                params![repo_id.as_str()],
            )
            .map_err(|e| AgentError::Database(format!("Invalidate index: {e}")))?;
        Ok(())
    }

    pub fn completed_index(&self, repo_id: &RepoId) -> Result<Option<CompletedIndexState>> {
        self.conn
            .query_row(
                "SELECT head_commit, fingerprint, phase FROM completed_index_state WHERE repo_id = ?1",
                params![repo_id.as_str()],
                |row| {
                    Ok(CompletedIndexState {
                        head_commit: row.get(0)?,
                        fingerprint: row.get(1)?,
                        phase: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AgentError::Database(format!("Read index completion: {e}")))
    }

    pub fn complete_index(&mut self, info: &RepoInfo, fingerprint: &str) -> Result<()> {
        self.update_repo_info(info)?;
        self.conn.execute("INSERT INTO completed_index_state (repo_id, head_commit, fingerprint, completed_at, phase)
            VALUES (?1, ?2, ?3, ?4, 'complete') ON CONFLICT(repo_id) DO UPDATE SET
            head_commit=excluded.head_commit, fingerprint=excluded.fingerprint, completed_at=excluded.completed_at, phase='complete'",
            params![info.id.as_str(), info.head_commit, fingerprint, Utc::now().to_rfc3339()])
            .map_err(|e| AgentError::Database(format!("Complete index: {e}")))?;
        Ok(())
    }

    /// Record or update repository metadata.
    pub fn update_repo_info(&mut self, info: &RepoInfo) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                r#"
                INSERT INTO repositories (repo_id, name, root_path, head_commit, branch, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(repo_id) DO UPDATE SET
                    name = excluded.name,
                    root_path = excluded.root_path,
                    head_commit = excluded.head_commit,
                    branch = excluded.branch,
                    indexed_at = excluded.indexed_at
                "#,
                params![
                    info.id.as_str(),
                    info.name,
                    info.root_path.to_string_lossy(),
                    info.head_commit,
                    info.branch,
                    now
                ],
            )
            .map_err(|e| {
                AgentError::Database(format!("Failed to update repository info: {}", e))
            })?;

        Ok(())
    }

    /// Record that a repository was opened.
    ///
    /// Inserts a new row with the given fields. On conflict, updates only name,
    /// root path, and branch. `head_commit` and `indexed_at` of an existing row
    /// stay at the last successful index.
    pub fn note_repo_presence(&self, info: &RepoInfo) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                r#"
                INSERT INTO repositories (repo_id, name, root_path, head_commit, branch, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(repo_id) DO UPDATE SET
                    name = excluded.name,
                    root_path = excluded.root_path,
                    branch = excluded.branch
                "#,
                params![
                    info.id.as_str(),
                    info.name,
                    info.root_path.to_string_lossy(),
                    info.head_commit,
                    info.branch,
                    now
                ],
            )
            .map_err(|e| {
                AgentError::Database(format!("Failed to note repository presence: {}", e))
            })?;

        Ok(())
    }

    /// Save scanned files and populate FTS5 search index.
    pub fn save_scanned_files(
        &mut self,
        repo_id: &RepoId,
        scanned: &[ScannedFile],
    ) -> Result<usize> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| AgentError::Database(format!("Failed to start transaction: {}", e)))?;

        // Clear existing files and FTS for clean index sync
        tx.execute(
            "DELETE FROM files WHERE repo_id = ?1",
            params![repo_id.as_str()],
        )
        .map_err(|e| AgentError::Database(format!("Failed to clear old files: {}", e)))?;
        tx.execute(
            "DELETE FROM files_fts WHERE repo_id = ?1",
            params![repo_id.as_str()],
        )
        .map_err(|e| AgentError::Database(format!("Failed to clear FTS index: {}", e)))?;

        let mut count = 0;
        {
            let mut insert_file = tx
                .prepare(
                    r#"
                    INSERT INTO files (
                        repo_id, relative_path, file_hash, git_blob_id, size_bytes,
                        language, kind, is_test, is_doc, indexed_at,
                        raw_fingerprint, redacted_fingerprint
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare insert file failed: {}", e)))?;

            let mut insert_fts = tx
                .prepare(
                    r#"
                    INSERT INTO files_fts (repo_id, relative_path, content)
                    VALUES (?1, ?2, ?3)
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare insert FTS failed: {}", e)))?;

            for item in scanned {
                let f = &item.file;
                let now = f.indexed_at.to_rfc3339();
                let redacted_fp = redacted_file_fingerprint(&f.file_hash, item.content.as_deref());

                insert_file
                    .execute(params![
                        repo_id.as_str(),
                        f.relative_path,
                        f.file_hash,
                        f.git_blob_id,
                        f.size_bytes as i64,
                        f.language.as_str(),
                        f.kind.to_string(),
                        if f.is_test { 1 } else { 0 },
                        if f.is_doc { 1 } else { 0 },
                        now,
                        f.file_hash,
                        redacted_fp
                    ])
                    .map_err(|e| AgentError::Database(format!("Insert file error: {}", e)))?;

                // Add to FTS5 if content is present
                if let Some(ref text) = item.content {
                    insert_fts
                        .execute(params![repo_id.as_str(), f.relative_path, text])
                        .map_err(|e| AgentError::Database(format!("Insert FTS error: {}", e)))?;
                }

                count += 1;
            }
        }

        tx.commit().map_err(|e| {
            AgentError::Database(format!("Failed to commit index transaction: {}", e))
        })?;

        Ok(count)
    }

    /// Query statistics for a repository index.
    pub fn get_stats(&self, repo_id: &RepoId) -> Result<IndexStats> {
        let (head_commit, last_indexed_at): (Option<String>, Option<String>) = self
            .conn
            .query_row(
                "SELECT head_commit, completed_at FROM completed_index_state WHERE repo_id = ?1",
                params![repo_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| AgentError::Database(format!("Failed to read index state: {}", e)))?
            .unwrap_or((None, None));

        let last_indexed_dt = last_indexed_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        let (total_files, source_files, test_files, doc_files, manifest_files, config_files): (
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
        ) = self
            .conn
            .query_row(
                r#"
                SELECT
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN kind = 'source' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(is_test), 0),
                    COALESCE(SUM(is_doc), 0),
                    COALESCE(SUM(CASE WHEN kind = 'manifest' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'configuration' THEN 1 ELSE 0 END), 0)
                FROM files
                WHERE repo_id = ?1
                "#,
                params![repo_id.as_str()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .map_err(|e| AgentError::Database(format!("Failed to aggregate files: {}", e)))?;

        let (total_symbols, total_references): (i64, i64) = self
            .conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM symbols WHERE repo_id = ?1),
                    (SELECT COUNT(*) FROM symbol_references WHERE repo_id = ?1)",
                params![repo_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| {
                AgentError::Database(format!("Failed to aggregate symbol counts: {}", e))
            })?;

        Ok(IndexStats {
            repo_id: repo_id.to_string(),
            total_files: total_files as usize,
            source_files: source_files as usize,
            test_files: test_files as usize,
            doc_files: doc_files as usize,
            manifest_files: manifest_files as usize,
            config_files: config_files as usize,
            total_symbols: total_symbols as usize,
            total_references: total_references as usize,
            last_indexed_commit: head_commit,
            last_indexed_at: last_indexed_dt,
        })
    }

    /// List all indexed files for a repository.
    pub fn list_files(&self, repo_id: &RepoId) -> Result<Vec<IndexedFile>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT relative_path, file_hash, git_blob_id, size_bytes, language, kind, is_test, is_doc, indexed_at
                FROM files
                WHERE repo_id = ?1
                ORDER BY relative_path
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare list files error: {}", e)))?;

        let rows = stmt
            .query_map(params![repo_id.as_str()], |row| {
                let lang_str: String = row.get(4)?;
                let kind_str: String = row.get(5)?;
                let dt_str: String = row.get(8)?;

                let language = Language::from_name(&lang_str);
                let kind = match kind_str.as_str() {
                    "source" => FileKind::Source,
                    "test" => FileKind::Test,
                    "documentation" => FileKind::Documentation,
                    "manifest" => FileKind::Manifest,
                    "configuration" => FileKind::Configuration,
                    "data" => FileKind::Data,
                    _ => FileKind::Unknown,
                };
                let indexed_at = DateTime::parse_from_rfc3339(&dt_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(IndexedFile {
                    relative_path: row.get(0)?,
                    file_hash: row.get(1)?,
                    git_blob_id: row.get(2)?,
                    size_bytes: row.get::<_, i64>(3)? as u64,
                    language,
                    kind,
                    is_test: row.get::<_, i32>(6)? == 1,
                    is_doc: row.get::<_, i32>(7)? == 1,
                    indexed_at,
                })
            })
            .map_err(|e| AgentError::Database(format!("Query list files error: {}", e)))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("List files row error: {}", e)))
    }

    /// Execute FTS5 search query.
    ///
    /// With the `embeddings` feature, a local vector lookup may run first.
    /// It returns nothing until a separate model process fills `file_vectors`,
    /// and this method then uses FTS5.
    pub fn search_content(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        #[cfg(feature = "embeddings")]
        if let Some(hits) = crate::embed::semantic_hits(&self.conn, query, limit)? {
            if !hits.is_empty() {
                return Ok(hits);
            }
        }
        FtsSearcher::search(&self.conn, query, limit)
    }

    /// The phase stored for a completed index. Missing rows are `idle`.
    pub fn index_phase(&self, repo_id: &RepoId) -> Result<String> {
        let stored = self
            .completed_index(repo_id)?
            .map(|state| state.phase)
            .unwrap_or_default();
        Ok(agent_core::phase_label(&stored).to_string())
    }

    /// Project one decayed fact. The id must already start with `fact:`.
    pub fn project_fact_row(
        &self,
        repo_id: &RepoId,
        memory_id: &str,
        confidence: f64,
        updated_at: &str,
    ) -> Result<()> {
        if !memory_id.starts_with("fact:") || memory_id == "fact:" {
            return Err(AgentError::General(
                "only fact projections may be written to memory_index".into(),
            ));
        }
        if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
            return Err(AgentError::General(
                "fact confidence is outside 0..=1".into(),
            ));
        }
        self.conn
            .execute(
                "INSERT INTO memory_index (
                    memory_id, repo_id, status, confidence, author_kind, updated_at, jsonl_offset
                 ) VALUES (?1, ?2, 'projected', ?3, 'git-fact', ?4, NULL)
                 ON CONFLICT(memory_id) DO UPDATE SET
                    status = excluded.status,
                    confidence = excluded.confidence,
                    author_kind = excluded.author_kind,
                    updated_at = excluded.updated_at",
                params![memory_id, repo_id.as_str(), confidence, updated_at],
            )
            .map_err(|err| AgentError::Database(format!("fact projection failed: {err}")))?;
        Ok(())
    }

    pub fn projected_confidence(&self, repo_id: &RepoId, memory_id: &str) -> Result<Option<f64>> {
        self.conn
            .query_row(
                "SELECT confidence FROM memory_index WHERE repo_id = ?1 AND memory_id = ?2",
                params![repo_id.as_str(), memory_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| AgentError::Database(format!("fact projection read failed: {err}")))
    }

    /// Store an approval after the self-approval gate. agt still checks the hash.
    pub fn record_approval(
        &self,
        repo_id: &RepoId,
        request: &crate::approval::ApprovalRequest,
    ) -> Result<()> {
        crate::approval::admit_approval(request)?;
        self.conn
            .execute(
                "INSERT INTO approvals (
                    id, repo_id, agt_run_id, agt_command_id, args_hash, decision, approver, source, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    request.id,
                    repo_id.as_str(),
                    request.agt_run_id,
                    request.agt_command_id,
                    request.args_hash.trim().to_ascii_lowercase(),
                    request.decision,
                    request.approver,
                    request.source,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|err| AgentError::Database(format!("approval insert failed: {err}")))?;
        Ok(())
    }

    /// Approvals already stored for one repository, oldest first.
    pub fn list_approvals(&self, repo_id: &RepoId) -> Result<Vec<crate::approval::StoredApproval>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, agt_run_id, agt_command_id, args_hash, decision
                 FROM approvals WHERE repo_id = ?1 ORDER BY created_at",
            )
            .map_err(|err| AgentError::Database(format!("approval list failed: {err}")))?;
        let rows = stmt
            .query_map(params![repo_id.as_str()], |row| {
                Ok(crate::approval::StoredApproval {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    command_id: row.get(2)?,
                    args_hash: row.get(3)?,
                    decision: row.get(4)?,
                })
            })
            .map_err(|err| AgentError::Database(format!("approval list failed: {err}")))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| AgentError::Database(format!("approval row failed: {err}")))
    }

    /// Store an evidence package only when its digest and contract pass.
    pub fn ingest_evidence(
        &self,
        repo_id: &RepoId,
        package: &crate::evidence::EvidenceIngest,
    ) -> Result<()> {
        crate::evidence::admit_evidence(package)?;
        let digest = crate::evidence::digest_package(&package.package_json);
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT package_digest FROM evidence_packages WHERE repo_id = ?1 AND agt_run_id = ?2",
                params![repo_id.as_str(), package.agt_run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| AgentError::Database(format!("evidence lookup failed: {err}")))?;
        if let Some(existing) = existing {
            if existing.eq_ignore_ascii_case(&digest) {
                return Ok(());
            }
            return Err(AgentError::Security(
                "evidence run already has a different digest".into(),
            ));
        }
        let id = crate::evidence::digest_package(&format!(
            "{}:{}",
            repo_id.as_str(),
            package.agt_run_id
        ));
        self.conn
            .execute(
                "INSERT INTO evidence_packages (
                    id, repo_id, agt_run_id, task_type, interaction, outcome,
                    contract_digest, verification_digest, package_digest, package_json,
                    head_commit, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    id,
                    repo_id.as_str(),
                    package.agt_run_id,
                    package.task_type,
                    package.interaction,
                    package.outcome,
                    package.contract_digest,
                    package.verification_digest,
                    digest,
                    package.package_json,
                    package.head_commit,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|err| AgentError::Database(format!("evidence insert failed: {err}")))?;
        Ok(())
    }

    /// True when a catalog has files and the v5 full-text table has not been filled.
    pub fn fts_needs_rebuild(&self) -> Result<bool> {
        let files: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .map_err(|e| AgentError::Database(format!("files count failed: {e}")))?;
        let indexed: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files_fts", [], |row| row.get(0))
            .map_err(|e| AgentError::Database(format!("files_fts count failed: {e}")))?;
        Ok(files > 0 && indexed == 0)
    }

    /// Save extracted symbols and directed reference edges.
    pub fn save_symbols_and_references(
        &mut self,
        repo_id: &RepoId,
        symbols: &[Symbol],
        references: &[SymbolReference],
    ) -> Result<(usize, usize)> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| AgentError::Database(format!("Failed to start transaction: {}", e)))?;

        // Clear existing symbols and references for this repository
        tx.execute(
            "DELETE FROM symbols WHERE repo_id = ?1",
            params![repo_id.as_str()],
        )
        .map_err(|e| AgentError::Database(format!("Failed to clear old symbols: {}", e)))?;
        tx.execute(
            "DELETE FROM symbol_references WHERE repo_id = ?1",
            params![repo_id.as_str()],
        )
        .map_err(|e| AgentError::Database(format!("Failed to clear old references: {}", e)))?;

        let mut sym_count = 0;
        let mut ref_count = 0;

        {
            let mut insert_sym = tx
                .prepare(
                    r#"
                    INSERT INTO symbols (
                        id, repo_id, name, qualified_name, kind, file_path,
                        start_line, end_line, signature, doc_comment, fingerprint,
                        raw_fingerprint, redacted_fingerprint
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                    "#,
                )
                .map_err(|e| {
                    AgentError::Database(format!("Prepare insert symbol failed: {}", e))
                })?;

            for s in symbols {
                insert_sym
                    .execute(params![
                        s.id,
                        repo_id.as_str(),
                        s.name,
                        s.qualified_name,
                        s.kind.to_string(),
                        s.file_path,
                        s.start_line as i64,
                        s.end_line as i64,
                        s.signature,
                        s.doc_comment,
                        s.fingerprint,
                        s.fingerprint,
                        s.fingerprint
                    ])
                    .map_err(|e| AgentError::Database(format!("Insert symbol error: {}", e)))?;
                sym_count += 1;
            }

            let mut insert_ref = tx
                .prepare(
                    r#"
                    INSERT INTO symbol_references (
                        repo_id, source_file, source_symbol_name, target_name,
                        target_symbol_id, kind, line_number, source_symbol_id
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare insert ref failed: {}", e)))?;

            for r in references {
                insert_ref
                    .execute(params![
                        repo_id.as_str(),
                        r.source_file,
                        r.source_symbol_name,
                        r.target_name,
                        r.target_symbol_id,
                        r.kind.to_string(),
                        r.line_number as i64,
                        r.source_symbol_id
                    ])
                    .map_err(|e| AgentError::Database(format!("Insert reference error: {}", e)))?;
                ref_count += 1;
            }
        }

        tx.commit().map_err(|e| {
            AgentError::Database(format!("Failed to commit symbol transaction: {}", e))
        })?;

        Ok((sym_count, ref_count))
    }

    /// Look up symbols by name (exact or qualified match).
    pub fn find_symbols_by_name(&self, repo_id: &RepoId, name: &str) -> Result<Vec<Symbol>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT id, name, qualified_name, kind, file_path, start_line, end_line, signature, doc_comment, fingerprint
                FROM symbols
                WHERE repo_id = ?1 AND (name = ?2 OR qualified_name = ?2 OR qualified_name LIKE ?3 ESCAPE '\')
                ORDER BY file_path, start_line
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare find symbol error: {}", e)))?;

        let like_query = format!("%::{}", like_escape(name));
        let rows = stmt
            .query_map(params![repo_id.as_str(), name, like_query], |row| {
                let kind_str: String = row.get(3)?;
                Ok(Symbol {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    qualified_name: row.get(2)?,
                    kind: SymbolKind::from_str_kind(&kind_str),
                    file_path: row.get(4)?,
                    start_line: row.get::<_, i64>(5)? as usize,
                    end_line: row.get::<_, i64>(6)? as usize,
                    signature: row.get(7)?,
                    doc_comment: row.get(8)?,
                    fingerprint: row.get(9)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("Query find symbol error: {}", e)))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Find symbol row error: {}", e)))
    }

    /// Search symbols by partial name matching.
    pub fn search_symbols(
        &self,
        repo_id: &RepoId,
        term: &str,
        limit: usize,
    ) -> Result<Vec<Symbol>> {
        self.list_symbols_paginated(repo_id, Some(term), 0, limit)
    }

    /// Search or browse symbols with offset/limit pagination.
    pub fn list_symbols_paginated(
        &self,
        repo_id: &RepoId,
        term: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Symbol>> {
        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<Symbol> {
            let kind_str: String = row.get(3)?;
            Ok(Symbol {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                kind: SymbolKind::from_str_kind(&kind_str),
                file_path: row.get(4)?,
                start_line: row.get::<_, i64>(5)? as usize,
                end_line: row.get::<_, i64>(6)? as usize,
                signature: row.get(7)?,
                doc_comment: row.get(8)?,
                fingerprint: row.get(9)?,
            })
        };
        let term = term.map(str::trim).filter(|s| !s.is_empty());
        if let Some(term) = term {
            let mut stmt = self
                .conn
                .prepare_cached(
                    r#"
                    SELECT id, name, qualified_name, kind, file_path, start_line, end_line, signature, doc_comment, fingerprint
                    FROM symbols
                    WHERE repo_id = ?1 AND (name LIKE ?2 ESCAPE '\' OR qualified_name LIKE ?2 ESCAPE '\')
                    ORDER BY file_path, start_line
                    LIMIT ?3 OFFSET ?4
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare search symbols error: {}", e)))?;
            let like_query = format!("%{}%", like_escape(term));
            let rows = stmt
                .query_map(
                    params![repo_id.as_str(), like_query, limit as i64, offset as i64],
                    map_row,
                )
                .map_err(|e| AgentError::Database(format!("Query search symbols error: {}", e)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| AgentError::Database(format!("Search symbols row error: {}", e)))
        } else {
            let mut stmt = self
                .conn
                .prepare_cached(
                    r#"
                    SELECT id, name, qualified_name, kind, file_path, start_line, end_line, signature, doc_comment, fingerprint
                    FROM symbols
                    WHERE repo_id = ?1
                    ORDER BY file_path, start_line
                    LIMIT ?2 OFFSET ?3
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare list symbols error: {}", e)))?;
            let rows = stmt
                .query_map(
                    params![repo_id.as_str(), limit as i64, offset as i64],
                    map_row,
                )
                .map_err(|e| AgentError::Database(format!("Query list symbols error: {}", e)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| AgentError::Database(format!("List symbols row error: {}", e)))
        }
    }

    /// Symbols defined in one indexed file. Alias used by the staleness engine.
    pub fn find_symbols_by_file(&self, repo_id: &RepoId, file_path: &str) -> Result<Vec<Symbol>> {
        self.find_symbols_in_file(repo_id, file_path)
    }

    /// Find all symbols defined within a specific file.
    pub fn find_symbols_in_file(&self, repo_id: &RepoId, file_path: &str) -> Result<Vec<Symbol>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                r#"
                SELECT id, name, qualified_name, kind, file_path, start_line, end_line, signature, doc_comment, fingerprint
                FROM symbols
                WHERE repo_id = ?1 AND file_path = ?2
                ORDER BY start_line
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare find symbols in file error: {}", e)))?;

        let rows = stmt
            .query_map(params![repo_id.as_str(), file_path], |row| {
                let kind_str: String = row.get(3)?;
                Ok(Symbol {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    qualified_name: row.get(2)?,
                    kind: SymbolKind::from_str_kind(&kind_str),
                    file_path: row.get(4)?,
                    start_line: row.get::<_, i64>(5)? as usize,
                    end_line: row.get::<_, i64>(6)? as usize,
                    signature: row.get(7)?,
                    doc_comment: row.get(8)?,
                    fingerprint: row.get(9)?,
                })
            })
            .map_err(|e| {
                AgentError::Database(format!("Query find symbols in file error: {}", e))
            })?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("File symbols row error: {}", e)))
    }

    /// Query symbol dependencies (callers, callees, and file imports), scoped and deduplicated.
    pub fn find_symbol_deps(
        &self,
        repo_id: &RepoId,
        symbol_name: &str,
    ) -> Result<SymbolDepsResult> {
        let defined_in = self.find_symbols_by_name(repo_id, symbol_name)?;

        let mut target_names: BTreeSet<&str> = BTreeSet::new();
        target_names.insert(symbol_name);
        for def in &defined_in {
            target_names.insert(def.name.as_str());
            target_names.insert(def.qualified_name.as_str());
        }

        let mut seen_callers = HashSet::new();
        let mut callers = Vec::new();
        {
            let mut stmt_callers = self
                .conn
                .prepare_cached(
                    r#"
                SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number, source_symbol_id
                FROM symbol_references
                WHERE repo_id = ?1 AND target_name = ?2 AND kind = 'calls'
                ORDER BY source_file, line_number
                "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare callers query error: {}", e)))?;
            for candidate in target_names {
                let rows_callers = stmt_callers
                    .query_map(params![repo_id.as_str(), candidate], row_to_reference)
                    .map_err(|e| AgentError::Database(format!("Query callers error: {}", e)))?;
                for row in rows_callers {
                    let r =
                        row.map_err(|e| AgentError::Database(format!("Caller row error: {}", e)))?;
                    let key = (
                        r.source_file.clone(),
                        r.source_symbol_id.clone(),
                        r.target_name.clone(),
                        r.line_number,
                    );
                    if seen_callers.insert(key) {
                        callers.push(r);
                    }
                }
            }
        }
        callers.sort_by(|a, b| {
            a.source_file
                .cmp(&b.source_file)
                .then_with(|| a.line_number.cmp(&b.line_number))
        });

        let mut seen_callees = HashSet::new();
        let mut callees = Vec::new();
        if defined_in.is_empty() {
            let mut stmt_callees = self
                .conn
                .prepare_cached(
                    r#"
                SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number, source_symbol_id
                FROM symbol_references
                WHERE repo_id = ?1 AND source_symbol_name = ?2 AND kind = 'calls'
                ORDER BY line_number
                "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare callees query error: {}", e)))?;
            let rows = stmt_callees
                .query_map(params![repo_id.as_str(), symbol_name], row_to_reference)
                .map_err(|e| AgentError::Database(format!("Query callees error: {}", e)))?;
            for row in rows {
                callees.push(
                    row.map_err(|e| AgentError::Database(format!("Callee row error: {}", e)))?,
                );
            }
        } else {
            let mut stmt_callees = self
                .conn
                .prepare_cached(
                    r#"
                SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number, source_symbol_id
                FROM symbol_references
                WHERE repo_id = ?1
                  AND kind = 'calls'
                  AND (
                    source_symbol_id = ?2
                    OR (source_symbol_id IS NULL AND source_file = ?3 AND source_symbol_name = ?4)
                  )
                ORDER BY line_number
                "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare callees query error: {}", e)))?;
            for def in &defined_in {
                let rows = stmt_callees
                    .query_map(
                        params![repo_id.as_str(), def.id, def.file_path, def.name],
                        row_to_reference,
                    )
                    .map_err(|e| AgentError::Database(format!("Query callees error: {}", e)))?;
                for row in rows {
                    let r =
                        row.map_err(|e| AgentError::Database(format!("Callee row error: {}", e)))?;
                    let key = (
                        r.source_file.clone(),
                        r.source_symbol_id.clone(),
                        r.target_name.clone(),
                        r.line_number,
                    );
                    if seen_callees.insert(key) {
                        callees.push(r);
                    }
                }
            }
            callees.sort_by(|a, b| {
                a.source_file
                    .cmp(&b.source_file)
                    .then_with(|| a.line_number.cmp(&b.line_number))
            });
        }

        // Query imports once per unique file containing a matching symbol definition.
        let mut unique_files: BTreeSet<&str> = BTreeSet::new();
        for def in &defined_in {
            unique_files.insert(def.file_path.as_str());
        }
        let mut imports = Vec::new();
        if !unique_files.is_empty() {
            let mut stmt_imp = self
                .conn
                .prepare_cached(
                    r#"
                SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number, source_symbol_id
                FROM symbol_references
                WHERE repo_id = ?1 AND source_file = ?2 AND kind = 'imports'
                ORDER BY line_number
                "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare imports query error: {}", e)))?;
            for file_path in unique_files {
                let rows_imp = stmt_imp
                    .query_map(params![repo_id.as_str(), file_path], row_to_reference)
                    .map_err(|e| AgentError::Database(format!("Query imports error: {}", e)))?;
                for row in rows_imp {
                    imports.push(
                        row.map_err(|e| AgentError::Database(format!("Import row error: {}", e)))?,
                    );
                }
            }
        }

        Ok(SymbolDepsResult {
            symbol_name: symbol_name.to_string(),
            defined_in,
            callers,
            callees,
            imports,
        })
    }

    /// Targeted reference list for one file (`kind` optional filter, SQL-filtered).
    pub fn list_references_in_file(
        &self,
        repo_id: &RepoId,
        file_path: &str,
        kind: Option<ReferenceKind>,
    ) -> Result<Vec<SymbolReference>> {
        if let Some(kind) = kind {
            let mut stmt = self
                .conn
                .prepare_cached(
                    r#"
                    SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number, source_symbol_id
                    FROM symbol_references
                    WHERE repo_id = ?1 AND source_file = ?2 AND kind = ?3
                    ORDER BY line_number
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare file refs error: {}", e)))?;
            let rows = stmt
                .query_map(
                    params![repo_id.as_str(), file_path, kind.to_string()],
                    row_to_reference,
                )
                .map_err(|e| AgentError::Database(format!("Query file refs error: {}", e)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| AgentError::Database(format!("File ref row error: {}", e)))
        } else {
            let mut stmt = self
                .conn
                .prepare_cached(
                    r#"
                    SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number, source_symbol_id
                    FROM symbol_references
                    WHERE repo_id = ?1 AND source_file = ?2
                    ORDER BY line_number
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare file refs error: {}", e)))?;
            let rows = stmt
                .query_map(params![repo_id.as_str(), file_path], row_to_reference)
                .map_err(|e| AgentError::Database(format!("Query file refs error: {}", e)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| AgentError::Database(format!("File ref row error: {}", e)))
        }
    }

    /// SQL-filtered importers matching a file stem inside `target_name`.
    pub fn list_importers_of_file(
        &self,
        repo_id: &RepoId,
        stem: &str,
    ) -> Result<Vec<SymbolReference>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                r#"
                SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number, source_symbol_id
                FROM symbol_references
                WHERE repo_id = ?1 AND kind = 'imports' AND target_name LIKE ?2 ESCAPE '\'
                ORDER BY source_file, line_number
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare importers query error: {}", e)))?;
        let like_query = format!("%{}%", like_escape(stem));
        let rows = stmt
            .query_map(params![repo_id.as_str(), like_query], row_to_reference)
            .map_err(|e| AgentError::Database(format!("Query importers error: {}", e)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Importer row error: {}", e)))
    }

    /// Aggregate reference counts and top imported targets in SQL without loading all rows.
    pub fn reference_summary_by_kind(&self, repo_id: &RepoId) -> Result<ReferenceSummary> {
        let mut stmt_counts = self
            .conn
            .prepare_cached(
                "SELECT kind, COUNT(*) FROM symbol_references WHERE repo_id = ?1 GROUP BY kind",
            )
            .map_err(|e| AgentError::Database(format!("Prepare ref summary error: {}", e)))?;
        let mut calls_count = 0usize;
        let mut imports_count = 0usize;
        let mut tests_count = 0usize;
        let count_rows = stmt_counts
            .query_map(params![repo_id.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| AgentError::Database(format!("Query ref summary error: {}", e)))?;
        for row in count_rows {
            let (kind, count) =
                row.map_err(|e| AgentError::Database(format!("Ref summary row error: {}", e)))?;
            match kind.as_str() {
                "calls" => calls_count = count as usize,
                "imports" => imports_count = count as usize,
                "tests" => tests_count = count as usize,
                _ => {}
            }
        }

        let mut stmt_top = self
            .conn
            .prepare_cached(
                r#"
                SELECT target_name, COUNT(*) AS cnt
                FROM symbol_references
                WHERE repo_id = ?1 AND kind = 'imports'
                GROUP BY target_name
                ORDER BY cnt DESC, target_name ASC
                LIMIT 20
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare top imports error: {}", e)))?;
        let top_rows = stmt_top
            .query_map(params![repo_id.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
            })
            .map_err(|e| AgentError::Database(format!("Query top imports error: {}", e)))?;
        let top_imported = top_rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Top imports row error: {}", e)))?;

        Ok(ReferenceSummary {
            calls_count,
            imports_count,
            tests_count,
            top_imported,
        })
    }

    /// Read-only dataset ID lookup (never writes during read-only context compilation).
    pub fn find_dataset(&self, repo_id: &RepoId, name: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT id FROM datasets WHERE repo_id = ?1 AND name = ?2",
                params![repo_id.as_str(), name],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| AgentError::Database(format!("Dataset lookup failed: {e}")))
    }

    /// Look up symbols by exact IDs in batched SQL queries.
    pub fn find_symbols_by_ids(&self, repo_id: &RepoId, ids: &[String]) -> Result<Vec<Symbol>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut symbols = Vec::new();
        for chunk in ids.chunks(200) {
            let placeholders = (0..chunk.len())
                .map(|idx| format!("?{}", idx + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT id, name, qualified_name, kind, file_path, start_line, end_line, signature, doc_comment, fingerprint
                 FROM symbols
                 WHERE repo_id = ?1 AND id IN ({placeholders})"
            );
            let mut stmt = self.conn.prepare_cached(&sql).map_err(|e| {
                AgentError::Database(format!("Prepare find symbols by ids failed: {e}"))
            })?;
            let repo_str = repo_id.as_str();
            let mut bind_values: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + chunk.len());
            bind_values.push(&repo_str);
            for id in chunk {
                bind_values.push(id);
            }
            let rows = stmt
                .query_map(bind_values.as_slice(), |row| {
                    let kind_str: String = row.get(3)?;
                    Ok(Symbol {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        qualified_name: row.get(2)?,
                        kind: SymbolKind::from_str_kind(&kind_str),
                        file_path: row.get(4)?,
                        start_line: row.get::<_, i64>(5)? as usize,
                        end_line: row.get::<_, i64>(6)? as usize,
                        signature: row.get(7)?,
                        doc_comment: row.get(8)?,
                        fingerprint: row.get(9)?,
                    })
                })
                .map_err(|e| AgentError::Database(format!("Query symbols by ids failed: {e}")))?;
            for row in rows {
                symbols.push(
                    row.map_err(|e| AgentError::Database(format!("Symbol id row failed: {e}")))?,
                );
            }
        }
        Ok(symbols)
    }

    /// Return relative paths of files defining at least one symbol targeted by a `tests` reference.
    pub fn list_tested_file_paths(&self, repo_id: &RepoId) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                r#"
                SELECT DISTINCT s.file_path
                FROM symbol_references sr
                JOIN symbols s
                  ON s.repo_id = sr.repo_id
                 AND (sr.target_symbol_id = s.id OR sr.target_name = s.name)
                WHERE sr.repo_id = ?1 AND sr.kind = 'tests'
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare tested files query failed: {e}")))?;
        let rows = stmt
            .query_map(params![repo_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(|e| AgentError::Database(format!("Query tested files failed: {e}")))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Tested file row failed: {e}")))
    }

    /// Ensure an actor row exists in `actors` (v5 schema) and return its `id`.
    pub fn upsert_actor(
        &self,
        repo_id: &RepoId,
        kind: &str,
        display_name: &str,
        peer_uid: Option<i64>,
    ) -> Result<String> {
        let mut hasher = Sha256::new();
        hasher.update(repo_id.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(kind.as_bytes());
        hasher.update([0]);
        hasher.update(display_name.as_bytes());
        let id = hex::encode(hasher.finalize());
        self.conn
            .execute(
                "INSERT INTO actors (id, repo_id, kind, display_name, peer_uid, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(repo_id, kind, display_name) DO UPDATE SET
                    peer_uid = COALESCE(excluded.peer_uid, actors.peer_uid)",
                params![
                    id,
                    repo_id.as_str(),
                    kind,
                    display_name,
                    peer_uid,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|e| AgentError::Database(format!("Upsert actor failed: {e}")))?;
        self.conn
            .query_row(
                "SELECT id FROM actors WHERE repo_id = ?1 AND kind = ?2 AND display_name = ?3",
                params![repo_id.as_str(), kind, display_name],
                |r| r.get(0),
            )
            .map_err(|e| AgentError::Database(format!("Read actor id failed: {e}")))
    }

    /// Register an open pending tool invocation bound to a random confirmation nonce (#132).
    #[allow(clippy::too_many_arguments)]
    pub fn start_pending_invocation(
        &self,
        repo_id: &RepoId,
        actor_kind: &str,
        actor_name: &str,
        tool: &str,
        args_hash: &str,
        args_canonical: &str,
        confirmation_nonce: &str,
        pending_id: &str,
    ) -> Result<()> {
        let actor_id = self.upsert_actor(repo_id, actor_kind, actor_name, None)?;
        let meta = json!({
            "confirmation_nonce": confirmation_nonce,
            "status": "pending",
        })
        .to_string();
        self.conn
            .execute(
                "INSERT INTO tool_invocations (
                    id, repo_id, actor_id, tool, args_hash, args_canonical,
                    started_at, finished_at, is_error, error_code, warning_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, 0, NULL, ?8)",
                params![
                    pending_id,
                    repo_id.as_str(),
                    actor_id,
                    tool,
                    args_hash.trim().to_ascii_lowercase(),
                    args_canonical,
                    Utc::now().to_rfc3339(),
                    meta
                ],
            )
            .map_err(|e| AgentError::Database(format!("Insert pending invocation failed: {e}")))?;
        Ok(())
    }

    /// Record a completed MCP/CLI tool invocation and optional token budget into v5 telemetry (#153).
    #[allow(clippy::too_many_arguments)]
    pub fn record_completed_invocation(
        &self,
        repo_id: &RepoId,
        actor_kind: &str,
        actor_name: &str,
        tool: &str,
        args_hash: &str,
        args_canonical: &str,
        is_error: bool,
        error_code: Option<&str>,
        tokens: Option<u64>,
    ) -> Result<()> {
        let actor_id = self.upsert_actor(repo_id, actor_kind, actor_name, None)?;
        let now = Utc::now().to_rfc3339();
        let random_id = format!(
            "inv-{}",
            agent_core::contain::random_hex(8).unwrap_or_else(|_| Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
                .to_string())
        );
        let warning_json = tokens.map(|t| json!({ "tokens": t }).to_string());
        self.conn
            .execute(
                "INSERT INTO tool_invocations (
                    id, repo_id, actor_id, tool, args_hash, args_canonical,
                    started_at, finished_at, is_error, error_code, warning_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9, ?10)",
                params![
                    random_id,
                    repo_id.as_str(),
                    actor_id,
                    tool,
                    args_hash,
                    args_canonical,
                    now,
                    if is_error { 1 } else { 0 },
                    error_code,
                    warning_json
                ],
            )
            .map_err(|e| AgentError::Database(format!("Record tool invocation failed: {e}")))?;
        Ok(())
    }

    /// List open pending confirmations (`finished_at IS NULL`) with daemon-issued confirmation nonces.
    pub fn list_pending_confirmations(&self, repo_id: &RepoId) -> Result<Vec<PendingConfirmation>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, repo_id, tool, args_hash, args_canonical, started_at, warning_json
                 FROM tool_invocations
                 WHERE repo_id = ?1 AND finished_at IS NULL AND warning_json IS NOT NULL
                 ORDER BY started_at ASC",
            )
            .map_err(|e| {
                AgentError::Database(format!("Prepare pending confirmations failed: {e}"))
            })?;
        let rows = stmt
            .query_map(params![repo_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(|e| {
                AgentError::Database(format!("Query pending confirmations failed: {e}"))
            })?;
        let mut pending = Vec::new();
        for row in rows {
            let (id, row_repo, tool, args_hash, args_canonical, created_at, warning) = row
                .map_err(|e| {
                    AgentError::Database(format!("Pending confirmation row failed: {e}"))
                })?;
            let nonce = warning
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|v| {
                    v.get("confirmation_nonce")
                        .and_then(|n| n.as_str())
                        .map(str::to_string)
                })
                .filter(|n| !n.is_empty());
            if let Some(nonce) = nonce {
                pending.push(PendingConfirmation {
                    id,
                    repo_id: row_repo,
                    tool,
                    args_hash,
                    args_canonical,
                    nonce,
                    created_at,
                });
            }
        }
        Ok(pending)
    }

    /// Verify and atomically consume an open pending confirmation (`#132`), preventing replay.
    pub fn verify_and_consume_pending_confirmation(
        &self,
        pending_id: &str,
        args_hash: &str,
        presented_nonce: &str,
        decision: &str,
    ) -> Result<PendingConfirmation> {
        if decision != "allow" && decision != "deny" {
            return Err(AgentError::Security("invalid confirmation decision".into()));
        }
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| AgentError::Database(format!("Begin confirmation tx failed: {e}")))?;
        let result = (|| -> Result<PendingConfirmation> {
            let row: Option<(String, String, String, String, String, Option<String>)> = self
                .conn
                .query_row(
                    "SELECT repo_id, tool, args_hash, args_canonical, started_at, warning_json
                     FROM tool_invocations
                     WHERE id = ?1 AND finished_at IS NULL",
                    params![pending_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                        ))
                    },
                )
                .optional()
                .map_err(|e| {
                    AgentError::Database(format!("Read pending confirmation failed: {e}"))
                })?;
            let Some((repo_id, tool, expected_hash, args_canonical, created_at, warning)) = row
            else {
                return Err(AgentError::Security(
                    "no active pending approval matches this invocation id".into(),
                ));
            };
            let expected_nonce = warning
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|v| {
                    v.get("confirmation_nonce")
                        .and_then(|n| n.as_str())
                        .map(str::to_string)
                })
                .filter(|n| !n.is_empty())
                .ok_or_else(|| {
                    AgentError::Security("pending invocation has no confirmation nonce".into())
                })?;

            if !constant_time_bytes_eq(expected_nonce.as_bytes(), presented_nonce.as_bytes()) {
                return Err(AgentError::Security(
                    "confirmation nonce does not match the pending approval".into(),
                ));
            }
            if !expected_hash.eq_ignore_ascii_case(args_hash.trim()) {
                return Err(AgentError::Security(
                    "args_hash does not match the pending invocation".into(),
                ));
            }

            let finished_meta = json!({ "decision": decision }).to_string();
            let updated = self
                .conn
                .execute(
                    "UPDATE tool_invocations
                     SET finished_at = ?2, is_error = ?3, error_code = ?4, warning_json = ?5
                     WHERE id = ?1 AND finished_at IS NULL",
                    params![
                        pending_id,
                        Utc::now().to_rfc3339(),
                        if decision == "deny" { 1 } else { 0 },
                        if decision == "deny" {
                            Some("denied")
                        } else {
                            None
                        },
                        finished_meta
                    ],
                )
                .map_err(|e| {
                    AgentError::Database(format!("Consume pending confirmation failed: {e}"))
                })?;
            if updated != 1 {
                return Err(AgentError::Security(
                    "pending approval was already consumed".into(),
                ));
            }
            Ok(PendingConfirmation {
                id: pending_id.to_string(),
                repo_id,
                tool,
                args_hash: expected_hash,
                args_canonical,
                nonce: expected_nonce,
                created_at,
            })
        })();
        match result {
            Ok(confirmed) => {
                self.conn.execute_batch("COMMIT").map_err(|e| {
                    AgentError::Database(format!("Commit confirmation tx failed: {e}"))
                })?;
                Ok(confirmed)
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    pub fn list_actors(&self, repo_id: &RepoId) -> Result<Vec<StoredActor>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, kind, display_name, peer_uid, created_at
                 FROM actors WHERE repo_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| AgentError::Database(format!("Prepare list actors failed: {e}")))?;
        let rows = stmt
            .query_map(params![repo_id.as_str()], |row| {
                Ok(StoredActor {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    display_name: row.get(2)?,
                    peer_uid: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("Query actors failed: {e}")))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Actor row failed: {e}")))
    }

    pub fn list_token_counts(&self, repo_id: &RepoId) -> Result<Vec<ToolTokenCount>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT tool, warning_json FROM tool_invocations WHERE repo_id = ?1")
            .map_err(|e| AgentError::Database(format!("Prepare token counts failed: {e}")))?;
        let rows = stmt
            .query_map(params![repo_id.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| AgentError::Database(format!("Query token counts failed: {e}")))?;
        let mut by_tool: std::collections::BTreeMap<String, (usize, u64)> =
            std::collections::BTreeMap::new();
        for row in rows {
            let (tool, warning) =
                row.map_err(|e| AgentError::Database(format!("Token count row failed: {e}")))?;
            let tokens = warning
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|v| v.get("tokens").and_then(|n| n.as_u64()))
                .unwrap_or(0);
            let entry = by_tool.entry(tool).or_insert((0, 0));
            entry.0 += 1;
            entry.1 = entry.1.saturating_add(tokens);
        }
        Ok(by_tool
            .into_iter()
            .map(|(tool, (invocations, total_tokens))| ToolTokenCount {
                tool,
                invocations,
                total_tokens,
            })
            .collect())
    }

    pub fn list_evidence_summaries(&self, repo_id: &RepoId) -> Result<Vec<StoredEvidenceSummary>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, agt_run_id, task_type, interaction, outcome, package_digest, head_commit, created_at
                 FROM evidence_packages WHERE repo_id = ?1 ORDER BY created_at DESC",
            )
            .map_err(|e| AgentError::Database(format!("Prepare list evidence failed: {e}")))?;
        let rows = stmt
            .query_map(params![repo_id.as_str()], |row| {
                Ok(StoredEvidenceSummary {
                    id: row.get(0)?,
                    agt_run_id: row.get(1)?,
                    task_type: row.get(2)?,
                    interaction: row.get(3)?,
                    outcome: row.get(4)?,
                    package_digest: row.get(5)?,
                    head_commit: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("Query list evidence failed: {e}")))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Evidence row failed: {e}")))
    }

    /// Find test suites and test functions referencing or testing this symbol.
    pub fn find_tests_for_symbol(
        &self,
        repo_id: &RepoId,
        symbol_name: &str,
    ) -> Result<Vec<SymbolReference>> {
        let mut tests: Vec<SymbolReference> = Vec::new();

        // Test definitions named after the symbol, plus calls aimed at the symbol
        // from test files. Do not treat every call inside a similarly named test
        // (assert, unwrap, String::new) as a test of the symbol.
        let mut stmt = self.conn.prepare(
            r#"
            SELECT DISTINCT sr.source_file, sr.source_symbol_name, sr.target_name, sr.target_symbol_id, sr.kind, sr.line_number, sr.source_symbol_id
            FROM symbol_references sr
            JOIN files f ON sr.source_file = f.relative_path AND sr.repo_id = f.repo_id
            WHERE sr.repo_id = ?1
              AND (
                (
                  sr.kind = 'tests'
                  AND (
                    sr.target_name = ?2
                    OR sr.source_symbol_name = ?2
                    OR sr.source_symbol_name = ?3
                    OR sr.source_symbol_name LIKE ?4 ESCAPE '\'
                  )
                )
                OR (
                  sr.target_name = ?2
                  AND (
                    f.is_test = 1
                    OR IFNULL(sr.source_symbol_name, '') LIKE 'test\_%' ESCAPE '\'
                  )
                )
              )
            ORDER BY sr.source_file, sr.line_number
            "#,
        ).map_err(|e| AgentError::Database(format!("Prepare test query error: {}", e)))?;

        let exact_test_name = format!("test_{}", symbol_name);
        let test_name_prefix = format!("test_{}_%", like_escape(symbol_name));
        let rows = stmt
            .query_map(
                params![
                    repo_id.as_str(),
                    symbol_name,
                    exact_test_name,
                    test_name_prefix
                ],
                |r| {
                    let kind_str: String = r.get(4)?;
                    Ok(SymbolReference {
                        source_file: r.get(0)?,
                        source_symbol_name: r.get(1)?,
                        source_symbol_id: r.get(6)?,
                        target_name: r.get(2)?,
                        target_symbol_id: r.get(3)?,
                        kind: ReferenceKind::from_str_kind(&kind_str),
                        line_number: r.get::<_, i64>(5)? as usize,
                    })
                },
            )
            .map_err(|e| AgentError::Database(format!("Query tests error: {}", e)))?;

        for t in rows {
            let t = t.map_err(|e| AgentError::Database(format!("Test row error: {}", e)))?;
            if let Some(existing) = tests.iter_mut().find(|existing| {
                existing.source_file == t.source_file
                    && existing.source_symbol_name == t.source_symbol_name
                    && existing.target_name == t.target_name
            }) {
                if existing.kind != ReferenceKind::Tests && t.kind == ReferenceKind::Tests {
                    *existing = t;
                }
            } else {
                tests.push(t);
            }
        }

        Ok(tests)
    }

    /// Files with v1/v2 references must be re-extracted even if their contents are unchanged.
    pub fn files_missing_source_identity(&self, repo_id: &RepoId) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT source_file FROM symbol_references
             WHERE repo_id = ?1 AND source_symbol_name IS NOT NULL AND source_symbol_id IS NULL",
            )
            .map_err(|e| AgentError::Database(format!("Prepare reference upgrade failed: {e}")))?;
        let rows = stmt
            .query_map(params![repo_id.as_str()], |row| row.get(0))
            .map_err(|e| AgentError::Database(format!("Read reference upgrade failed: {e}")))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Reference upgrade row failed: {e}")))
    }

    /// Incrementally update modified/added files and delete removed files.
    pub fn update_incremental(
        &mut self,
        repo_id: &RepoId,
        updated_scanned: &[ScannedFile],
        deleted_paths: &[String],
        new_symbols: &[Symbol],
        new_references: &[SymbolReference],
        head_commit: Option<&str>,
    ) -> Result<(usize, usize, usize)> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| AgentError::Database(format!("Failed to start transaction: {}", e)))?;

        let mut files_updated = 0;
        let mut files_deleted = 0;

        // 1. Delete removed files and their symbols/refs/FTS
        {
            let mut del_file = tx
                .prepare_cached("DELETE FROM files WHERE repo_id = ?1 AND relative_path = ?2")
                .map_err(|e| AgentError::Database(format!("Prepare delete file error: {}", e)))?;
            let mut del_fts = tx
                .prepare_cached("DELETE FROM files_fts WHERE repo_id = ?1 AND relative_path = ?2")
                .map_err(|e| AgentError::Database(format!("Prepare delete FTS error: {}", e)))?;
            let mut del_syms = tx
                .prepare_cached("DELETE FROM symbols WHERE repo_id = ?1 AND file_path = ?2")
                .map_err(|e| {
                    AgentError::Database(format!("Prepare delete symbols error: {}", e))
                })?;
            let mut del_refs = tx
                .prepare_cached(
                    "DELETE FROM symbol_references WHERE repo_id = ?1 AND source_file = ?2",
                )
                .map_err(|e| AgentError::Database(format!("Prepare delete refs error: {}", e)))?;

            for path in deleted_paths {
                del_file
                    .execute(params![repo_id.as_str(), path])
                    .map_err(|e| AgentError::Database(format!("Delete file error: {}", e)))?;
                del_fts
                    .execute(params![repo_id.as_str(), path])
                    .map_err(|e| AgentError::Database(format!("Delete FTS error: {}", e)))?;
                del_syms
                    .execute(params![repo_id.as_str(), path])
                    .map_err(|e| AgentError::Database(format!("Delete symbols error: {}", e)))?;
                del_refs
                    .execute(params![repo_id.as_str(), path])
                    .map_err(|e| AgentError::Database(format!("Delete refs error: {}", e)))?;
                files_deleted += 1;
            }
        }

        // 2. Clear old symbols and references and upsert modified files using cached statements
        {
            let mut clear_syms = tx
                .prepare_cached("DELETE FROM symbols WHERE repo_id = ?1 AND file_path = ?2")
                .map_err(|e| AgentError::Database(format!("Prepare clear symbols error: {}", e)))?;
            let mut clear_refs = tx
                .prepare_cached(
                    "DELETE FROM symbol_references WHERE repo_id = ?1 AND source_file = ?2",
                )
                .map_err(|e| AgentError::Database(format!("Prepare clear refs error: {}", e)))?;
            let mut clear_fts = tx
                .prepare_cached("DELETE FROM files_fts WHERE repo_id = ?1 AND relative_path = ?2")
                .map_err(|e| AgentError::Database(format!("Prepare clear FTS error: {}", e)))?;
            let mut upsert_file = tx
                .prepare_cached(
                    r#"
                    INSERT INTO files (
                        repo_id, relative_path, file_hash, git_blob_id, size_bytes,
                        language, kind, is_test, is_doc, indexed_at,
                        raw_fingerprint, redacted_fingerprint
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                    ON CONFLICT(repo_id, relative_path) DO UPDATE SET
                        file_hash = excluded.file_hash,
                        git_blob_id = excluded.git_blob_id,
                        size_bytes = excluded.size_bytes,
                        language = excluded.language,
                        kind = excluded.kind,
                        is_test = excluded.is_test,
                        is_doc = excluded.is_doc,
                        indexed_at = excluded.indexed_at,
                        raw_fingerprint = excluded.raw_fingerprint,
                        redacted_fingerprint = excluded.redacted_fingerprint
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare upsert file error: {}", e)))?;
            let mut insert_fts = tx
                .prepare_cached(
                    "INSERT INTO files_fts (repo_id, relative_path, content) VALUES (?1, ?2, ?3)",
                )
                .map_err(|e| AgentError::Database(format!("Prepare insert FTS error: {}", e)))?;

            for item in updated_scanned {
                let path = &item.file.relative_path;
                clear_syms
                    .execute(params![repo_id.as_str(), path])
                    .map_err(|e| AgentError::Database(format!("Clear old symbols error: {}", e)))?;
                clear_refs
                    .execute(params![repo_id.as_str(), path])
                    .map_err(|e| AgentError::Database(format!("Clear old refs error: {}", e)))?;
                clear_fts
                    .execute(params![repo_id.as_str(), path])
                    .map_err(|e| AgentError::Database(format!("Clear old FTS error: {}", e)))?;

                let f = &item.file;
                let now = f.indexed_at.to_rfc3339();
                let redacted_fp = redacted_file_fingerprint(&f.file_hash, item.content.as_deref());
                upsert_file
                    .execute(params![
                        repo_id.as_str(),
                        f.relative_path,
                        f.file_hash,
                        f.git_blob_id,
                        f.size_bytes as i64,
                        f.language.as_str(),
                        f.kind.to_string(),
                        if f.is_test { 1 } else { 0 },
                        if f.is_doc { 1 } else { 0 },
                        now,
                        f.file_hash,
                        redacted_fp
                    ])
                    .map_err(|e| AgentError::Database(format!("Upsert file error: {}", e)))?;

                if let Some(ref text) = item.content {
                    insert_fts
                        .execute(params![repo_id.as_str(), f.relative_path, text])
                        .map_err(|e| AgentError::Database(format!("Insert FTS error: {}", e)))?;
                }

                files_updated += 1;
            }
        }

        // 3. Insert new symbols
        let mut sym_count = 0;
        {
            let mut insert_sym = tx
                .prepare_cached(
                    r#"
                INSERT OR REPLACE INTO symbols (
                    id, repo_id, name, qualified_name, kind, file_path,
                    start_line, end_line, signature, doc_comment, fingerprint,
                    raw_fingerprint, redacted_fingerprint
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare insert symbol error: {}", e)))?;

            for s in new_symbols {
                insert_sym
                    .execute(params![
                        s.id,
                        repo_id.as_str(),
                        s.name,
                        s.qualified_name,
                        s.kind.to_string(),
                        s.file_path,
                        s.start_line as i64,
                        s.end_line as i64,
                        s.signature,
                        s.doc_comment,
                        s.fingerprint,
                        s.fingerprint,
                        s.fingerprint
                    ])
                    .map_err(|e| AgentError::Database(format!("Insert sym error: {}", e)))?;
                sym_count += 1;
            }

            let mut insert_ref = tx
                .prepare(
                    r#"
                INSERT INTO symbol_references (
                    repo_id, source_file, source_symbol_name, target_name,
                    target_symbol_id, kind, line_number, source_symbol_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare insert ref error: {}", e)))?;

            for r in new_references {
                insert_ref
                    .execute(params![
                        repo_id.as_str(),
                        r.source_file,
                        r.source_symbol_name,
                        r.target_name,
                        r.target_symbol_id,
                        r.kind.to_string(),
                        r.line_number as i64,
                        r.source_symbol_id
                    ])
                    .map_err(|e| AgentError::Database(format!("Insert ref error: {}", e)))?;
            }
        }

        // 4. Update repository indexed timestamp
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE repositories SET head_commit = ?1, indexed_at = ?2 WHERE repo_id = ?3",
            params![head_commit, now, repo_id.as_str()],
        )
        .map_err(|e| AgentError::Database(format!("Update repo commit error: {}", e)))?;

        tx.commit()
            .map_err(|e| AgentError::Database(format!("Commit incremental tx error: {}", e)))?;

        Ok((files_updated, sym_count, files_deleted))
    }
}

fn stored_schema_version(conn: &Connection) -> Result<Option<i32>> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| AgentError::Database(format!("schema version lookup failed: {e}")))?;
    if exists == 0 {
        return Ok(None);
    }
    match conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
        row.get(0)
    }) {
        Ok(version) => Ok(Some(version)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AgentError::Database(format!(
            "schema version read failed: {e}"
        ))),
    }
}

fn pre_v5_sidecar(db_path: &Path) -> PathBuf {
    let name = db_path
        .file_name()
        .map(|name| {
            let mut next = name.to_os_string();
            next.push(".pre-v5");
            next
        })
        .unwrap_or_else(|| std::ffi::OsString::from(".pre-v5"));
    db_path.with_file_name(name)
}

fn restrict_sqlite_files(path: &Path) {
    let mode = fs::Permissions::from_mode(0o600);
    let _ = fs::set_permissions(path, mode);
    for suffix in ["-wal", "-shm"] {
        let extra = PathBuf::from(format!("{}{suffix}", path.display()));
        if extra.exists() {
            let _ = fs::set_permissions(&extra, fs::Permissions::from_mode(0o600));
        }
    }
}

fn like_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn row_to_reference(r: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolReference> {
    let kind_str: String = r.get(4)?;
    Ok(SymbolReference {
        source_file: r.get(0)?,
        source_symbol_name: r.get(1)?,
        source_symbol_id: r.get(6)?,
        target_name: r.get(2)?,
        target_symbol_id: r.get(3)?,
        kind: ReferenceKind::from_str_kind(&kind_str),
        line_number: r.get::<_, i64>(5)? as usize,
    })
}

fn redacted_file_fingerprint(raw_hash: &str, redacted_content: Option<&str>) -> String {
    match redacted_content {
        Some(text) if text.contains("[REDACTED]") => {
            let mut hasher = Sha256::new();
            hasher.update(text.as_bytes());
            hex::encode(hasher.finalize())
        }
        _ => raw_hash.to_string(),
    }
}

fn constant_time_bytes_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}
