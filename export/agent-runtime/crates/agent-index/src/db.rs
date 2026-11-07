use crate::scanner::ScannedFile;
use crate::schema::{CURRENT_SCHEMA_VERSION, SCHEMA_V1_SQL, SCHEMA_V2_SQL};
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
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct IndexDatabase {
    pub(crate) conn: Connection,
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
        let tx = self
            .conn
            .transaction()
            .map_err(|e| AgentError::Database(format!("Failed to start transaction: {}", e)))?;

        tx.execute_batch(SCHEMA_V1_SQL)
            .map_err(|e| AgentError::Database(format!("Schema initialization failed: {}", e)))?;
        tx.execute_batch(SCHEMA_V2_SQL)
            .map_err(|e| AgentError::Database(format!("Schema v2 initialization failed: {}", e)))?;
        crate::graph::migrate_unscoped_commit_nodes(&tx)?;

        let version: Option<i32> = tx
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .ok();

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
            Some(_) => {}
        }

        tx.commit()
            .map_err(|e| AgentError::Database(format!("Failed to commit schema: {}", e)))?;

        Ok(())
    }

    pub fn schema_version(&self) -> Result<i32> {
        self.conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .map_err(|e| AgentError::Database(format!("Schema version read failed: {}", e)))
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
        tx.execute("DELETE FROM files_fts", [])
            .map_err(|e| AgentError::Database(format!("Failed to clear FTS index: {}", e)))?;

        let mut count = 0;
        {
            let mut insert_file = tx
                .prepare(
                    r#"
                    INSERT INTO files (
                        repo_id, relative_path, file_hash, git_blob_id, size_bytes,
                        language, kind, is_test, is_doc, indexed_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare insert file failed: {}", e)))?;

            let mut insert_fts = tx
                .prepare(
                    r#"
                    INSERT INTO files_fts (relative_path, content)
                    VALUES (?1, ?2)
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare insert FTS failed: {}", e)))?;

            for item in scanned {
                let f = &item.file;
                let now = f.indexed_at.to_rfc3339();

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
                        now
                    ])
                    .map_err(|e| AgentError::Database(format!("Insert file error: {}", e)))?;

                // Add to FTS5 if content is present
                if let Some(ref text) = item.content {
                    insert_fts
                        .execute(params![f.relative_path, text])
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
                "SELECT head_commit, indexed_at FROM repositories WHERE repo_id = ?1",
                params![repo_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((None, None));

        let last_indexed_dt = last_indexed_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        let total_files: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE repo_id = ?1",
                params![repo_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let source_files: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE repo_id = ?1 AND kind = 'source'",
                params![repo_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let test_files: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE repo_id = ?1 AND is_test = 1",
                params![repo_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let doc_files: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE repo_id = ?1 AND is_doc = 1",
                params![repo_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let manifest_files: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE repo_id = ?1 AND kind = 'manifest'",
                params![repo_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let config_files: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE repo_id = ?1 AND kind = 'configuration'",
                params![repo_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let total_symbols: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM symbols WHERE repo_id = ?1",
                params![repo_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let total_references: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM symbol_references WHERE repo_id = ?1",
                params![repo_id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);

        Ok(IndexStats {
            repo_id: repo_id.to_string(),
            total_files,
            source_files,
            test_files,
            doc_files,
            manifest_files,
            config_files,
            total_symbols,
            total_references,
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

        let mut files = Vec::new();
        for f in rows.flatten() {
            files.push(f);
        }
        Ok(files)
    }

    /// Execute FTS5 search query.
    pub fn search_content(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        FtsSearcher::search(&self.conn, query, limit)
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
                        start_line, end_line, signature, doc_comment, fingerprint
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                        target_symbol_id, kind, line_number
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
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
                        r.line_number as i64
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

        let mut symbols = Vec::new();
        for s in rows.flatten() {
            symbols.push(s);
        }
        Ok(symbols)
    }

    /// Search symbols by partial name matching.
    pub fn search_symbols(
        &self,
        repo_id: &RepoId,
        term: &str,
        limit: usize,
    ) -> Result<Vec<Symbol>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT id, name, qualified_name, kind, file_path, start_line, end_line, signature, doc_comment, fingerprint
                FROM symbols
                WHERE repo_id = ?1 AND (name LIKE ?2 ESCAPE '\' OR qualified_name LIKE ?2 ESCAPE '\')
                ORDER BY file_path, start_line
                LIMIT ?3
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare search symbols error: {}", e)))?;

        let like_query = format!("%{}%", like_escape(term));
        let rows = stmt
            .query_map(params![repo_id.as_str(), like_query, limit as i64], |row| {
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
            .map_err(|e| AgentError::Database(format!("Query search symbols error: {}", e)))?;

        let mut symbols = Vec::new();
        for s in rows.flatten() {
            symbols.push(s);
        }
        Ok(symbols)
    }

    /// Symbols defined in one indexed file. Alias used by the staleness engine.
    pub fn find_symbols_by_file(&self, repo_id: &RepoId, file_path: &str) -> Result<Vec<Symbol>> {
        self.find_symbols_in_file(repo_id, file_path)
    }

    /// Find all symbols defined within a specific file.
    pub fn find_symbols_in_file(&self, repo_id: &RepoId, file_path: &str) -> Result<Vec<Symbol>> {
        let mut stmt = self
            .conn
            .prepare(
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

        let mut symbols = Vec::new();
        for s in rows.flatten() {
            symbols.push(s);
        }
        Ok(symbols)
    }

    /// Query symbol dependencies (callers, callees, and file imports).
    pub fn find_symbol_deps(
        &self,
        repo_id: &RepoId,
        symbol_name: &str,
    ) -> Result<SymbolDepsResult> {
        let defined_in = self.find_symbols_by_name(repo_id, symbol_name)?;

        // Find callers: references where target_name == symbol_name and kind == 'calls'
        let mut callers = Vec::new();
        let mut stmt_callers = self
            .conn
            .prepare(
                r#"
            SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number
            FROM symbol_references
            WHERE repo_id = ?1 AND target_name = ?2 AND kind = 'calls'
            ORDER BY source_file, line_number
            "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare callers query error: {}", e)))?;

        let rows_callers = stmt_callers
            .query_map(params![repo_id.as_str(), symbol_name], |r| {
                let kind_str: String = r.get(4)?;
                Ok(SymbolReference {
                    source_file: r.get(0)?,
                    source_symbol_name: r.get(1)?,
                    target_name: r.get(2)?,
                    target_symbol_id: r.get(3)?,
                    kind: ReferenceKind::from_str_kind(&kind_str),
                    line_number: r.get::<_, i64>(5)? as usize,
                })
            })
            .map_err(|e| AgentError::Database(format!("Query callers error: {}", e)))?;

        for c in rows_callers.flatten() {
            callers.push(c);
        }

        // Find callees: references originating from source_symbol_name == symbol_name and kind == 'calls'
        let mut callees = Vec::new();
        let mut stmt_callees = self
            .conn
            .prepare(
                r#"
            SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number
            FROM symbol_references
            WHERE repo_id = ?1 AND source_symbol_name = ?2 AND kind = 'calls'
            ORDER BY line_number
            "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare callees query error: {}", e)))?;

        let rows_callees = stmt_callees
            .query_map(params![repo_id.as_str(), symbol_name], |r| {
                let kind_str: String = r.get(4)?;
                Ok(SymbolReference {
                    source_file: r.get(0)?,
                    source_symbol_name: r.get(1)?,
                    target_name: r.get(2)?,
                    target_symbol_id: r.get(3)?,
                    kind: ReferenceKind::from_str_kind(&kind_str),
                    line_number: r.get::<_, i64>(5)? as usize,
                })
            })
            .map_err(|e| AgentError::Database(format!("Query callees error: {}", e)))?;

        for c in rows_callees.flatten() {
            callees.push(c);
        }

        // Find file imports: imports declared in the files where symbol is defined
        let mut imports = Vec::new();
        for def in &defined_in {
            let mut stmt_imp = self.conn.prepare(
                r#"
                SELECT source_file, source_symbol_name, target_name, target_symbol_id, kind, line_number
                FROM symbol_references
                WHERE repo_id = ?1 AND source_file = ?2 AND kind = 'imports'
                ORDER BY line_number
                "#,
            ).map_err(|e| AgentError::Database(format!("Prepare imports query error: {}", e)))?;

            let rows_imp = stmt_imp
                .query_map(params![repo_id.as_str(), def.file_path], |r| {
                    let kind_str: String = r.get(4)?;
                    Ok(SymbolReference {
                        source_file: r.get(0)?,
                        source_symbol_name: r.get(1)?,
                        target_name: r.get(2)?,
                        target_symbol_id: r.get(3)?,
                        kind: ReferenceKind::from_str_kind(&kind_str),
                        line_number: r.get::<_, i64>(5)? as usize,
                    })
                })
                .map_err(|e| AgentError::Database(format!("Query imports error: {}", e)))?;

            for i in rows_imp.flatten() {
                imports.push(i);
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
            SELECT DISTINCT sr.source_file, sr.source_symbol_name, sr.target_name, sr.target_symbol_id, sr.kind, sr.line_number
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
                        target_name: r.get(2)?,
                        target_symbol_id: r.get(3)?,
                        kind: ReferenceKind::from_str_kind(&kind_str),
                        line_number: r.get::<_, i64>(5)? as usize,
                    })
                },
            )
            .map_err(|e| AgentError::Database(format!("Query tests error: {}", e)))?;

        for t in rows.flatten() {
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
        for path in deleted_paths {
            tx.execute(
                "DELETE FROM files WHERE repo_id = ?1 AND relative_path = ?2",
                params![repo_id.as_str(), path],
            )
            .map_err(|e| AgentError::Database(format!("Delete file error: {}", e)))?;

            tx.execute(
                "DELETE FROM files_fts WHERE relative_path = ?1",
                params![path],
            )
            .map_err(|e| AgentError::Database(format!("Delete FTS error: {}", e)))?;

            tx.execute(
                "DELETE FROM symbols WHERE repo_id = ?1 AND file_path = ?2",
                params![repo_id.as_str(), path],
            )
            .map_err(|e| AgentError::Database(format!("Delete symbols error: {}", e)))?;

            tx.execute(
                "DELETE FROM symbol_references WHERE repo_id = ?1 AND source_file = ?2",
                params![repo_id.as_str(), path],
            )
            .map_err(|e| AgentError::Database(format!("Delete refs error: {}", e)))?;

            files_deleted += 1;
        }

        // 2. Clear old symbols and references for modified files
        for item in updated_scanned {
            let path = &item.file.relative_path;
            tx.execute(
                "DELETE FROM symbols WHERE repo_id = ?1 AND file_path = ?2",
                params![repo_id.as_str(), path],
            )
            .map_err(|e| AgentError::Database(format!("Clear old symbols error: {}", e)))?;

            tx.execute(
                "DELETE FROM symbol_references WHERE repo_id = ?1 AND source_file = ?2",
                params![repo_id.as_str(), path],
            )
            .map_err(|e| AgentError::Database(format!("Clear old refs error: {}", e)))?;

            tx.execute(
                "DELETE FROM files_fts WHERE relative_path = ?1",
                params![path],
            )
            .map_err(|e| AgentError::Database(format!("Clear old FTS error: {}", e)))?;

            // Upsert file
            let f = &item.file;
            let now = f.indexed_at.to_rfc3339();
            tx.execute(
                r#"
                INSERT INTO files (
                    repo_id, relative_path, file_hash, git_blob_id, size_bytes,
                    language, kind, is_test, is_doc, indexed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(relative_path) DO UPDATE SET
                    file_hash = excluded.file_hash,
                    git_blob_id = excluded.git_blob_id,
                    size_bytes = excluded.size_bytes,
                    language = excluded.language,
                    kind = excluded.kind,
                    is_test = excluded.is_test,
                    is_doc = excluded.is_doc,
                    indexed_at = excluded.indexed_at
                "#,
                params![
                    repo_id.as_str(),
                    f.relative_path,
                    f.file_hash,
                    f.git_blob_id,
                    f.size_bytes as i64,
                    f.language.as_str(),
                    f.kind.to_string(),
                    if f.is_test { 1 } else { 0 },
                    if f.is_doc { 1 } else { 0 },
                    now
                ],
            )
            .map_err(|e| AgentError::Database(format!("Upsert file error: {}", e)))?;

            if let Some(ref text) = item.content {
                tx.execute(
                    "INSERT INTO files_fts (relative_path, content) VALUES (?1, ?2)",
                    params![f.relative_path, text],
                )
                .map_err(|e| AgentError::Database(format!("Insert FTS error: {}", e)))?;
            }

            files_updated += 1;
        }

        // 3. Insert new symbols
        let mut sym_count = 0;
        {
            let mut insert_sym = tx
                .prepare(
                    r#"
                INSERT OR REPLACE INTO symbols (
                    id, repo_id, name, qualified_name, kind, file_path,
                    start_line, end_line, signature, doc_comment, fingerprint
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                    target_symbol_id, kind, line_number
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
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
                        r.line_number as i64
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
