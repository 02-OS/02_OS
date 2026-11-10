use serde::{Deserialize, Serialize};

/// One file scored for a task. The total is the sum of the five signals.
///
/// Symbol weights match the delivery compiler: exact `+10`, partial `+7`.
/// Counts above 3 do not add more, so one chatty file cannot drown the rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub path: String,
    pub symbol_match: f64,
    pub test_proximity: f64,
    pub recency: f64,
    pub architectural: f64,
    pub staleness_penalty: f64,
    pub total: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScoreInput<'a> {
    pub path: &'a str,
    pub exact_symbols: usize,
    pub partial_symbols: usize,
    /// `Some(0)` when this test file matches the task. `Some(1)` when a test
    /// references a symbol defined here. `None` when no test relationship exists.
    pub test_distance: Option<u8>,
    pub commits_touching: usize,
    pub kind: &'a str,
    pub is_entry: bool,
    pub index_stale: bool,
}

pub fn score_file(input: &ScoreInput<'_>) -> ScoreBreakdown {
    let mut reasons = Vec::new();
    let symbol_match =
        (input.exact_symbols.min(3) as f64) * 10.0 + (input.partial_symbols.min(3) as f64) * 7.0;
    if input.exact_symbols > 0 {
        reasons.push(format!(
            "exact symbol match x{}",
            input.exact_symbols.min(3)
        ));
    }
    if input.partial_symbols > 0 {
        reasons.push(format!(
            "partial symbol match x{}",
            input.partial_symbols.min(3)
        ));
    }

    let test_proximity = match input.test_distance {
        Some(0) => 6.0,
        Some(1) => 4.0,
        Some(_) => 2.0,
        None => 0.0,
    };
    if test_proximity > 0.0 {
        reasons.push(format!("test proximity {test_proximity}"));
    }

    let recency = (input.commits_touching as f64).min(4.0);
    if recency > 0.0 {
        reasons.push(format!(
            "touched by {} recent commits",
            input.commits_touching.min(4)
        ));
    }

    let architectural = if input.is_entry {
        5.0
    } else if input.kind == "manifest" {
        3.0
    } else if input.kind == "documentation" {
        1.0
    } else {
        0.0
    };
    if architectural > 0.0 {
        reasons.push(format!("architectural importance {architectural}"));
    }

    let staleness_penalty = if input.index_stale { 3.0 } else { 0.0 };
    if staleness_penalty > 0.0 {
        reasons.push("index is stale against HEAD".to_string());
    }

    let total =
        (symbol_match + test_proximity + recency + architectural - staleness_penalty).max(0.0);
    ScoreBreakdown {
        path: input.path.to_string(),
        symbol_match,
        test_proximity,
        recency,
        architectural,
        staleness_penalty,
        total,
        reasons,
    }
}

pub fn is_entry_path(path: &str) -> bool {
    matches!(
        path.rsplit('/').next(),
        Some("main.rs" | "lib.rs" | "mod.rs")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_symbol_outranks_a_stale_partial_match() {
        let exact = score_file(&ScoreInput {
            path: "src/auth.rs",
            exact_symbols: 1,
            partial_symbols: 0,
            test_distance: Some(1),
            commits_touching: 1,
            kind: "source",
            is_entry: false,
            index_stale: false,
        });
        let stale = score_file(&ScoreInput {
            path: "src/other.rs",
            exact_symbols: 0,
            partial_symbols: 1,
            test_distance: None,
            commits_touching: 0,
            kind: "source",
            is_entry: false,
            index_stale: true,
        });
        assert_eq!(exact.symbol_match, 10.0);
        assert_eq!(exact.test_proximity, 4.0);
        assert!(exact.total > stale.total);
        assert_eq!(stale.staleness_penalty, 3.0);
        assert_eq!(stale.total, 4.0);
    }
}
