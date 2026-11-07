use agent_core::{error::AgentError, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub relative_path: String,
    pub snippet: String,
    pub rank: f64,
}

pub struct FtsSearcher;

impl FtsSearcher {
    /// Execute an FTS5 search query against the repository index using BM25 ranking.
    pub fn search(conn: &Connection, raw_query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // Sanitize search query for SQLite FTS5
        let sanitized = sanitize_fts5_query(raw_query);
        if sanitized.trim().is_empty() {
            return Ok(Vec::new());
        }

        let sql = r#"
            SELECT 
                relative_path, 
                snippet(files_fts, 1, '<b>', '</b>', '...', 16) AS snippet,
                bm25(files_fts) AS rank
            FROM files_fts
            WHERE files_fts MATCH ?1
            ORDER BY rank
            LIMIT ?2
        "#;

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| AgentError::Database(format!("Failed to prepare FTS statement: {}", e)))?;

        let rows = stmt
            .query_map(rusqlite::params![sanitized, limit as i64], |row| {
                Ok(SearchResult {
                    relative_path: row.get(0)?,
                    snippet: row.get(1)?,
                    rank: row.get(2)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("FTS query error: {}", e)))?;

        let mut results = Vec::new();
        for res in rows.flatten() {
            results.push(res);
        }

        Ok(results)
    }
}

/// Sanitize query string for FTS5 (escape special characters like `*`, `"`, etc.).
fn sanitize_fts5_query(input: &str) -> String {
    let words: Vec<&str> = input
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| !w.is_empty())
        .collect();

    if words.is_empty() {
        return String::new();
    }

    // Join with OR or NEAR
    words
        .into_iter()
        .map(|w| format!("\"{}\"", w))
        .collect::<Vec<_>>()
        .join(" ")
}
