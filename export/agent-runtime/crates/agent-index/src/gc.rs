use crate::db::IndexDatabase;
use agent_core::{AgentError, Result};
use rusqlite::params;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct GcReport {
    pub orphan_directories: usize,
    pub datapoints_removed: usize,
    pub databases_vacuumed: usize,
}

impl IndexDatabase {
    pub fn begin_immediate(&self) -> Result<()> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| AgentError::Database(format!("Begin failed: {e}")))
    }

    pub fn commit_tx(&self) -> Result<()> {
        self.conn
            .execute_batch("COMMIT")
            .map_err(|e| AgentError::Database(format!("Commit failed: {e}")))
    }

    pub fn rollback_tx(&self) {
        let _ = self.conn.execute_batch("ROLLBACK");
    }

    pub fn stored_root_paths(&self) -> Result<Vec<PathBuf>> {
        let mut stmt = self
            .conn
            .prepare("SELECT root_path FROM repositories")
            .map_err(|e| AgentError::Database(format!("Prepare roots failed: {e}")))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| AgentError::Database(format!("Query roots failed: {e}")))?;
        let mut paths = Vec::new();
        for row in rows {
            let path = row.map_err(|e| AgentError::Database(format!("Root row failed: {e}")))?;
            paths.push(PathBuf::from(path));
        }
        Ok(paths)
    }

    /// Drop file and chunk datapoints whose path is no longer in the catalog.
    pub fn purge_catalog_orphans(&mut self, dataset_id: &str) -> Result<usize> {
        let ids = self.orphan_datapoint_ids(dataset_id)?;
        self.delete_datapoints(&ids)
    }

    /// Keep the newest `keep` versions of each datapoint path.
    pub fn prune_old_versions(&mut self, keep: i64) -> Result<usize> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| AgentError::Database(format!("Prune transaction failed: {e}")))?;
        let ids = {
            let mut stmt = tx
                .prepare(
                    r#"
                    SELECT id FROM (
                        SELECT id, ROW_NUMBER() OVER (
                            PARTITION BY dataset_id, kind, path
                            ORDER BY version DESC
                        ) AS rn
                        FROM datapoints
                    ) WHERE rn > ?1
                    "#,
                )
                .map_err(|e| AgentError::Database(format!("Prepare prune failed: {e}")))?;
            let rows = stmt
                .query_map(params![keep], |row| row.get::<_, String>(0))
                .map_err(|e| AgentError::Database(format!("Query prune failed: {e}")))?;
            let mut ids = Vec::new();
            for row in rows {
                ids.push(row.map_err(|e| AgentError::Database(format!("Prune row failed: {e}")))?);
            }
            ids
        };
        let removed = delete_ids(&tx, &ids)?;
        tx.commit()
            .map_err(|e| AgentError::Database(format!("Prune commit failed: {e}")))?;
        Ok(removed)
    }

    pub fn gc_database(&mut self) -> Result<usize> {
        let datasets = self.dataset_ids()?;
        let mut removed = 0;
        for dataset_id in datasets {
            removed += self.purge_catalog_orphans(&dataset_id)?;
        }
        removed += self.prune_old_versions(3)?;
        Ok(removed)
    }

    pub fn vacuum(&self) -> Result<()> {
        self.conn
            .execute_batch("VACUUM")
            .map_err(|e| AgentError::Database(format!("VACUUM failed: {e}")))
    }

    fn dataset_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM datasets")
            .map_err(|e| AgentError::Database(format!("Prepare datasets failed: {e}")))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| AgentError::Database(format!("Query datasets failed: {e}")))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(|e| AgentError::Database(format!("Dataset row failed: {e}")))?);
        }
        Ok(ids)
    }

    fn orphan_datapoint_ids(&self, dataset_id: &str) -> Result<Vec<String>> {
        let mut files = self
            .conn
            .prepare("SELECT relative_path FROM files")
            .map_err(|e| AgentError::Database(format!("Prepare catalog paths failed: {e}")))?;
        let catalog: std::collections::HashSet<String> = files
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| AgentError::Database(format!("Query catalog paths failed: {e}")))?
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| AgentError::Database(format!("Catalog path row failed: {e}")))?;
        drop(files);

        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT id, kind, path FROM datapoints
                WHERE dataset_id = ?1
                  AND kind IN ('file', 'chunk')
                  AND path != ''
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare orphan datapoints failed: {e}")))?;
        let rows = stmt
            .query_map(params![dataset_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| AgentError::Database(format!("Query orphan datapoints failed: {e}")))?;
        let mut ids = Vec::new();
        for row in rows {
            let (id, kind, path) =
                row.map_err(|e| AgentError::Database(format!("Orphan row failed: {e}")))?;
            let owner = crate::graph::datapoint_file_path(&kind, &path);
            if !catalog.contains(owner) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    fn delete_datapoints(&mut self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let tx = self
            .conn
            .transaction()
            .map_err(|e| AgentError::Database(format!("Delete transaction failed: {e}")))?;
        let removed = delete_ids(&tx, ids)?;
        tx.commit()
            .map_err(|e| AgentError::Database(format!("Delete commit failed: {e}")))?;
        Ok(removed)
    }
}

fn delete_ids(tx: &rusqlite::Transaction<'_>, ids: &[String]) -> Result<usize> {
    let mut removed = 0;
    for id in ids {
        tx.execute(
            "DELETE FROM graph_nodes WHERE id = ?1 OR datapoint_id = ?1",
            params![id],
        )
        .map_err(|e| AgentError::Database(format!("Node delete failed: {e}")))?;
        removed += tx
            .execute("DELETE FROM datapoints WHERE id = ?1", params![id])
            .map_err(|e| AgentError::Database(format!("Datapoint delete failed: {e}")))?;
    }
    Ok(removed)
}

/// Remove orphan repo directories, prune datapoints, and vacuum each live database.
pub fn gc_repos(data_dir: &Path) -> Result<GcReport> {
    let repos = data_dir.join("repos");
    if !repos.is_dir() {
        return Ok(GcReport::default());
    }
    let mut report = GcReport::default();
    for entry in fs::read_dir(&repos).map_err(AgentError::Io)? {
        let entry = entry.map_err(AgentError::Io)?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let db_path = dir.join("index.sqlite");
        if !db_path.is_file() {
            let empty = fs::read_dir(&dir).map_err(AgentError::Io)?.next().is_none();
            if empty {
                fs::remove_dir_all(&dir).map_err(AgentError::Io)?;
                report.orphan_directories += 1;
            }
            continue;
        }
        let mut db = IndexDatabase::open(&db_path)?;
        let roots = db.stored_root_paths()?;
        if !roots.is_empty() && roots.iter().all(|path| !path.exists()) {
            drop(db);
            fs::remove_dir_all(&dir).map_err(AgentError::Io)?;
            report.orphan_directories += 1;
            continue;
        }
        report.datapoints_removed += db.gc_database()?;
        db.vacuum()?;
        report.databases_vacuumed += 1;
    }
    Ok(report)
}
