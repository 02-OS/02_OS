use crate::db::IndexDatabase;
use agent_core::{
    content_id, AgentError, DataPoint, Dataset, NodeSet, Provenance, RepoId, Result, Session,
    SourceKind,
};
use chrono::{DateTime, Utc};
use rusqlite::params;

pub struct InsertedPoint {
    pub id: String,
    pub version: u32,
    pub previous_id: Option<String>,
}

pub enum PutPoint {
    Unchanged { id: String },
    Inserted(InsertedPoint),
}

pub struct NewDataPoint {
    pub repo_id: RepoId,
    pub dataset_id: String,
    pub kind: String,
    pub path: String,
    pub content: String,
    pub provenance: Provenance,
    pub confidence: f64,
}

impl IndexDatabase {
    pub fn ensure_dataset(&mut self, repo_id: &RepoId, name: &str) -> Result<Dataset> {
        let dataset = Dataset::named(repo_id, name, Utc::now());
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM datasets WHERE repo_id = ?1 AND name = ?2",
                params![repo_id.as_str(), name],
                |row| row.get(0),
            )
            .ok();
        if existing.is_some() {
            let created_at = self
                .conn
                .query_row(
                    "SELECT created_at FROM datasets WHERE id = ?1",
                    params![dataset.id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|e| AgentError::Database(format!("Dataset read failed: {}", e)))?;
            let created_at = DateTime::parse_from_rfc3339(&created_at)
                .map(|value| value.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            return Ok(Dataset::named(repo_id, name, created_at));
        }

        self.conn
            .execute(
                "INSERT INTO datasets (id, repo_id, name, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    dataset.id,
                    repo_id.as_str(),
                    dataset.name,
                    dataset.created_at.to_rfc3339()
                ],
            )
            .map_err(|e| AgentError::Database(format!("Dataset insert failed: {}", e)))?;
        Ok(dataset)
    }

    pub fn put_datapoint(&mut self, point: NewDataPoint) -> Result<PutPoint> {
        let id = content_id(&point.dataset_id, &point.kind, &point.path, &point.content);
        let previous: Option<(String, i64)> = self
            .conn
            .query_row(
                r#"
                SELECT id, version FROM datapoints
                WHERE dataset_id = ?1 AND kind = ?2 AND path = ?3
                ORDER BY version DESC LIMIT 1
                "#,
                params![point.dataset_id, point.kind, point.path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((previous_id, _)) = &previous {
            if previous_id == &id {
                return Ok(PutPoint::Unchanged { id });
            }
        }

        let version = previous
            .as_ref()
            .map(|(_, version)| *version as u32 + 1)
            .unwrap_or(1);
        let now = Utc::now().to_rfc3339();
        let already_stored: Option<i64> = self
            .conn
            .query_row(
                "SELECT version FROM datapoints WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();
        if already_stored.is_some() {
            // A → B → A reuses the original content row and makes it the latest version.
            self.conn
                .execute(
                    r#"
                    UPDATE datapoints SET
                        version = ?1,
                        repo_id = ?2,
                        dataset_id = ?3,
                        kind = ?4,
                        source_kind = ?5,
                        commit_id = ?6,
                        path = ?7,
                        start_byte = ?8,
                        end_byte = ?9,
                        symbol_fingerprint = ?10,
                        confidence = ?11,
                        updated_at = ?12
                    WHERE id = ?13
                    "#,
                    params![
                        version as i64,
                        point.repo_id.as_str(),
                        point.dataset_id,
                        point.kind,
                        point.provenance.source_kind.as_str(),
                        point.provenance.commit_id,
                        point.path,
                        point.provenance.start_byte.map(|value| value as i64),
                        point.provenance.end_byte.map(|value| value as i64),
                        point.provenance.symbol_fingerprint,
                        point.confidence,
                        now,
                        id
                    ],
                )
                .map_err(|e| AgentError::Database(format!("Datapoint revive failed: {}", e)))?;
            return Ok(PutPoint::Inserted(InsertedPoint {
                id,
                version,
                previous_id: previous.map(|(previous_id, _)| previous_id),
            }));
        }
        self.conn
            .execute(
                r#"
                INSERT INTO datapoints (
                    id, repo_id, dataset_id, version, kind, content_hash, content,
                    source_kind, commit_id, path, start_byte, end_byte,
                    symbol_fingerprint, confidence, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                params![
                    id,
                    point.repo_id.as_str(),
                    point.dataset_id,
                    version as i64,
                    point.kind,
                    id,
                    point.content,
                    point.provenance.source_kind.as_str(),
                    point.provenance.commit_id,
                    point.path,
                    point.provenance.start_byte.map(|value| value as i64),
                    point.provenance.end_byte.map(|value| value as i64),
                    point.provenance.symbol_fingerprint,
                    point.confidence,
                    now,
                    now
                ],
            )
            .map_err(|e| AgentError::Database(format!("Datapoint insert failed: {}", e)))?;

        Ok(PutPoint::Inserted(InsertedPoint {
            id,
            version,
            previous_id: previous.map(|(previous_id, _)| previous_id),
        }))
    }

    pub fn list_datapoints(&self, dataset_id: &str) -> Result<Vec<DataPoint>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
                SELECT id, repo_id, dataset_id, version, kind, content_hash, content,
                       source_kind, commit_id, path, start_byte, end_byte,
                       symbol_fingerprint, confidence, created_at, updated_at
                FROM datapoints WHERE dataset_id = ?1 ORDER BY path, version
                "#,
            )
            .map_err(|e| AgentError::Database(format!("Prepare datapoints failed: {}", e)))?;
        let rows = stmt
            .query_map(params![dataset_id], |row| {
                let source = row.get::<_, String>(7)?;
                let created = row.get::<_, String>(14)?;
                let updated = row.get::<_, String>(15)?;
                Ok(DataPoint {
                    id: row.get(0)?,
                    repo_id: RepoId::new(row.get::<_, String>(1)?),
                    dataset_id: row.get(2)?,
                    version: row.get::<_, i64>(3)? as u32,
                    kind: row.get(4)?,
                    content_hash: row.get(5)?,
                    content: row.get(6)?,
                    provenance: Provenance {
                        source_kind: SourceKind::parse(&source).unwrap_or(SourceKind::Worktree),
                        commit_id: row.get(8)?,
                        path: row.get(9)?,
                        start_byte: row.get::<_, Option<i64>>(10)?.map(|value| value as u64),
                        end_byte: row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
                        symbol_fingerprint: row.get(12)?,
                    },
                    confidence: row.get(13)?,
                    created_at: parse_time(&created),
                    updated_at: parse_time(&updated),
                })
            })
            .map_err(|e| AgentError::Database(format!("Query datapoints failed: {}", e)))?;

        let mut points = Vec::new();
        for point in rows {
            points.push(
                point.map_err(|e| AgentError::Database(format!("Datapoint row failed: {}", e)))?,
            );
        }
        Ok(points)
    }

    pub fn ensure_session(&mut self, session: &Session) -> Result<()> {
        self.conn
            .execute(
                r#"
                INSERT OR IGNORE INTO sessions
                    (id, repo_id, dataset_id, label, ephemeral, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    session.id,
                    session.repo_id.as_str(),
                    session.dataset_id,
                    session.label,
                    if session.ephemeral { 1 } else { 0 },
                    session.created_at.to_rfc3339()
                ],
            )
            .map_err(|e| AgentError::Database(format!("Session insert failed: {}", e)))?;
        Ok(())
    }

    pub fn ensure_node_set(&mut self, repo_id: &RepoId, name: &str) -> Result<NodeSet> {
        let set = NodeSet::named(repo_id, name);
        self.conn
            .execute(
                "INSERT OR IGNORE INTO node_sets (id, repo_id, name) VALUES (?1, ?2, ?3)",
                params![set.id, repo_id.as_str(), set.name],
            )
            .map_err(|e| AgentError::Database(format!("Node set insert failed: {}", e)))?;
        Ok(set)
    }

    pub fn add_node_set_member(&mut self, node_set_id: &str, node_id: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO node_set_members (node_set_id, node_id) VALUES (?1, ?2)",
                params![node_set_id, node_id],
            )
            .map_err(|e| AgentError::Database(format!("Node set member insert failed: {}", e)))?;
        Ok(())
    }
}

fn parse_time(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
