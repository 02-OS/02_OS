use crate::facts::Fact;
use agent_core::{SecretPattern, Symbol};
use agent_git::FileDiffHunks;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Build git-anchored facts from changed line ranges.
///
/// The claim records the path and line. It does not copy hunk text.
pub fn facts_from_hunks(repo_id: &str, commit_id: &str, hunks: &[FileDiffHunks]) -> Vec<Fact> {
    let mut facts = Vec::new();
    for file in hunks {
        if SecretPattern::is_secret(Path::new(&file.file_path)) || file.hunks.is_empty() {
            continue;
        }
        let line = file.hunks[0].new_start;
        let claim = SecretPattern::redact(&format!(
            "edited {} around line {} ({} hunks)",
            file.file_path,
            line,
            file.hunks.len()
        ));
        facts.push(make_fact(
            repo_id,
            commit_id,
            &file.file_path,
            None,
            &claim,
            0.6,
            "diff",
        ));
        if facts.len() == 200 {
            break;
        }
    }
    facts
}

/// Build git-anchored facts from symbols that the parser already extracted.
pub fn facts_from_symbols(repo_id: &str, commit_id: &str, symbols: &[Symbol]) -> Vec<Fact> {
    let mut facts = Vec::new();
    for symbol in symbols {
        if SecretPattern::is_secret(Path::new(&symbol.file_path)) {
            continue;
        }
        let claim = SecretPattern::redact(&format!(
            "symbol {} ({}) is defined in {}",
            symbol.name, symbol.kind, symbol.file_path
        ));
        facts.push(make_fact(
            repo_id,
            commit_id,
            &symbol.file_path,
            Some(symbol.name.clone()),
            &claim,
            0.85,
            "ast",
        ));
        if facts.len() == 200 {
            break;
        }
    }
    facts
}

fn make_fact(
    repo_id: &str,
    commit_id: &str,
    path: &str,
    symbol: Option<String>,
    claim: &str,
    confidence: f64,
    source: &str,
) -> Fact {
    let now = Utc::now().to_rfc3339();
    let symbol_key = symbol.clone().unwrap_or_default();
    Fact {
        id: fact_id(&[repo_id, path, &symbol_key, source, commit_id, claim]),
        repo_id: repo_id.to_string(),
        claim: claim.to_string(),
        relative_path: path.to_string(),
        symbol,
        commit_id: commit_id.to_string(),
        base_confidence: confidence,
        confidence,
        source: source.to_string(),
        anchored_at: now.clone(),
        decayed_at: now,
    }
}

fn fact_id(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::SymbolKind;
    use agent_git::DiffHunk;

    #[test]
    fn secret_paths_produce_no_facts_and_symbols_keep_their_commit() {
        let hunks = vec![FileDiffHunks {
            file_path: ".env".into(),
            is_deleted: false,
            is_new: false,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
            }],
        }];
        assert!(facts_from_hunks("repo", "abc1234", &hunks).is_empty());
        let symbol = Symbol {
            id: "s".into(),
            name: "main".into(),
            qualified_name: "main".into(),
            kind: SymbolKind::Function,
            file_path: "src/main.rs".into(),
            start_line: 1,
            end_line: 3,
            signature: None,
            doc_comment: None,
            fingerprint: "fp".into(),
        };
        let facts = facts_from_symbols("repo", "abc1234", &[symbol]);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].commit_id, "abc1234");
        assert_eq!(facts[0].source, "ast");
        assert!(facts[0].claim.contains("main"));
    }
}
