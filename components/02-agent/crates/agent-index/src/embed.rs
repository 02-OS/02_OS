use crate::search::SearchResult;
use agent_core::Result;
use rusqlite::{params, Connection};

/// Semantic search is compiled only with `--features embeddings`.
///
/// There is no in-process model. A missing or populated `file_vectors` table
/// still returns `None`, and the caller uses FTS5.
pub fn semantic_hits(
    conn: &Connection,
    _query: &str,
    _limit: usize,
) -> Result<Option<Vec<SearchResult>>> {
    let present: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'file_vectors'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if present == 0 {
        return Ok(None);
    }
    let _rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM file_vectors", params![], |row| {
            row.get(0)
        })
        .unwrap_or(0);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndexDatabase;

    #[test]
    fn populated_vectors_still_fall_back_to_fts() {
        let db = IndexDatabase::open_in_memory().unwrap();
        db.conn
            .execute("CREATE TABLE file_vectors (path TEXT)", [])
            .unwrap();
        db.conn
            .execute("INSERT INTO file_vectors (path) VALUES ('src/a.rs')", [])
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO files_fts (repo_id, relative_path, content) VALUES ('r', 'src/a.rs', 'refresh token')",
                [],
            )
            .unwrap();
        assert!(semantic_hits(&db.conn, "refresh", 5).unwrap().is_none());
        let hits = db.search_content("refresh", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].relative_path, "src/a.rs");
    }
}
