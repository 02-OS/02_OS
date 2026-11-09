use crate::decay::{decayed_confidence, DEFAULT_HALF_LIFE_DAYS};
use agent_core::{paths::StoragePaths, AgentError, RepoId, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A fact tied to a commit, a path, and an optional symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub repo_id: String,
    pub claim: String,
    pub relative_path: String,
    pub symbol: Option<String>,
    pub commit_id: String,
    pub base_confidence: f64,
    pub confidence: f64,
    pub source: String,
    pub anchored_at: String,
    pub decayed_at: String,
}

/// SQLite catalog of git-anchored facts. Evidence memories stay in `memories.jsonl`.
pub struct FactStore {
    conn: Connection,
    path: Option<PathBuf>,
}

impl FactStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .map_err(|err| AgentError::Database(format!("facts database open failed: {err}")))?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|err| AgentError::Database(format!("facts pragma failed: {err}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS facts (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL,
                claim TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                symbol TEXT,
                commit_id TEXT NOT NULL,
                base_confidence REAL NOT NULL,
                confidence REAL NOT NULL,
                source TEXT NOT NULL,
                anchored_at TEXT NOT NULL,
                decayed_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_facts_repo_path ON facts (repo_id, relative_path);",
        )
        .map_err(|err| AgentError::Database(format!("facts schema failed: {err}")))?;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions)?;
        Ok(Self {
            conn,
            path: Some(path.to_path_buf()),
        })
    }

    pub fn for_repo(repo_id: &RepoId) -> Result<Self> {
        Self::open(StoragePaths::repo_facts_path(repo_id)?)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn upsert(&self, fact: &Fact) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO facts (
                    id, repo_id, claim, relative_path, symbol, commit_id,
                    base_confidence, confidence, source, anchored_at, decayed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET
                    claim = excluded.claim,
                    confidence = excluded.confidence,
                    base_confidence = excluded.base_confidence,
                    commit_id = excluded.commit_id,
                    anchored_at = excluded.anchored_at,
                    decayed_at = excluded.decayed_at",
                params![
                    fact.id,
                    fact.repo_id,
                    fact.claim,
                    fact.relative_path,
                    fact.symbol,
                    fact.commit_id,
                    fact.base_confidence,
                    fact.confidence,
                    fact.source,
                    fact.anchored_at,
                    fact.decayed_at
                ],
            )
            .map_err(|err| AgentError::Database(format!("fact upsert failed: {err}")))?;
        Ok(())
    }

    pub fn list(&self, repo_id: &str) -> Result<Vec<Fact>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, repo_id, claim, relative_path, symbol, commit_id,
                        base_confidence, confidence, source, anchored_at, decayed_at
                 FROM facts WHERE repo_id = ?1 ORDER BY relative_path, id",
            )
            .map_err(|err| AgentError::Database(format!("fact list failed: {err}")))?;
        let rows = stmt
            .query_map(params![repo_id], read_fact)
            .map_err(|err| AgentError::Database(format!("fact list failed: {err}")))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| AgentError::Database(format!("fact row failed: {err}")))
    }

    /// Materialize decay since each row's `decayed_at`. Returns the number of rows changed.
    pub fn decay(&self, now: DateTime<Utc>, half_life_days: f64) -> Result<usize> {
        if half_life_days <= 0.0 {
            return Err(AgentError::Config(
                "confidence half-life must be positive".into(),
            ));
        }
        let facts = self.list_all()?;
        let mut changed = 0;
        for fact in facts {
            let updated = DateTime::parse_from_rfc3339(&fact.decayed_at)
                .map(|stamp| stamp.with_timezone(&Utc))
                .unwrap_or(now);
            let next = decayed_confidence(fact.confidence, updated, now, half_life_days);
            if (next - fact.confidence).abs() < 0.000_000_1 {
                continue;
            }
            self.conn
                .execute(
                    "UPDATE facts SET confidence = ?1, decayed_at = ?2 WHERE id = ?3",
                    params![next, now.to_rfc3339(), fact.id],
                )
                .map_err(|err| AgentError::Database(format!("fact decay failed: {err}")))?;
            changed += 1;
        }
        Ok(changed)
    }

    /// Confidence a reader should use, including time passed since the last decay write.
    pub fn effective_confidence(&self, fact: &Fact, now: DateTime<Utc>) -> f64 {
        let updated = DateTime::parse_from_rfc3339(&fact.decayed_at)
            .map(|stamp| stamp.with_timezone(&Utc))
            .unwrap_or(now);
        decayed_confidence(fact.confidence, updated, now, DEFAULT_HALF_LIFE_DAYS)
    }

    fn list_all(&self) -> Result<Vec<Fact>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, repo_id, claim, relative_path, symbol, commit_id,
                        base_confidence, confidence, source, anchored_at, decayed_at
                 FROM facts",
            )
            .map_err(|err| AgentError::Database(format!("fact list failed: {err}")))?;
        let rows = stmt
            .query_map([], read_fact)
            .map_err(|err| AgentError::Database(format!("fact list failed: {err}")))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| AgentError::Database(format!("fact row failed: {err}")))
    }
}

fn read_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<Fact> {
    Ok(Fact {
        id: row.get(0)?,
        repo_id: row.get(1)?,
        claim: row.get(2)?,
        relative_path: row.get(3)?,
        symbol: row.get(4)?,
        commit_id: row.get(5)?,
        base_confidence: row.get(6)?,
        confidence: row.get(7)?,
        source: row.get(8)?,
        anchored_at: row.get(9)?,
        decayed_at: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::facts_from_symbols;
    use agent_core::Symbol;
    use agent_core::SymbolKind;
    use chrono::Duration;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn facts_round_trip_and_decay_is_materialized() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts.sqlite");
        let store = FactStore::open(&path).unwrap();
        let symbol = Symbol {
            id: "s".into(),
            name: "open".into(),
            qualified_name: "open".into(),
            kind: SymbolKind::Function,
            file_path: "src/lib.rs".into(),
            start_line: 1,
            end_line: 4,
            signature: None,
            doc_comment: None,
            fingerprint: "fp".into(),
        };
        let mut fact = facts_from_symbols("repo", "deadbee", &[symbol]).remove(0);
        fact.decayed_at = (Utc::now() - Duration::days(30)).to_rfc3339();
        fact.confidence = 0.8;
        store.upsert(&fact).unwrap();
        let changed = store.decay(Utc::now(), 30.0).unwrap();
        assert_eq!(changed, 1);
        let stored = store.list("repo").unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].commit_id, "deadbee");
        assert!(
            (stored[0].confidence - 0.4).abs() < 0.02,
            "{}",
            stored[0].confidence
        );
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
