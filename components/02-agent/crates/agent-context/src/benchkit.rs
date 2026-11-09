use std::time::Instant;

/// One measurement from the skeleton benchmark suite.
///
/// This is a harness, not a claim that a number is a release gate.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: &'static str,
    pub elapsed_ms: u128,
    pub units: f64,
    pub unit_name: &'static str,
}

pub fn time_call<T>(
    name: &'static str,
    unit_name: &'static str,
    units: f64,
    call: impl FnOnce() -> T,
) -> (T, Sample) {
    let started = Instant::now();
    let value = call();
    let sample = Sample {
        name,
        elapsed_ms: started.elapsed().as_millis(),
        units,
        unit_name,
    };
    (value, sample)
}

/// Share of returned tokens that the fixture marked useful. `1.0` is exact.
pub fn token_efficiency(useful_tokens: usize, returned_tokens: usize) -> f64 {
    if returned_tokens == 0 {
        return 0.0;
    }
    (useful_tokens as f64 / returned_tokens as f64).min(1.0)
}

/// Fraction of expected paths present in the returned ranking.
pub fn context_recall(expected: &[&str], returned: &[&str]) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    let hits = expected
        .iter()
        .filter(|path| returned.contains(path))
        .count();
    hits as f64 / expected.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn efficiency_and_recall_are_bounded() {
        assert_eq!(token_efficiency(25, 100), 0.25);
        assert_eq!(token_efficiency(200, 100), 1.0);
        assert_eq!(token_efficiency(1, 0), 0.0);
        assert_eq!(context_recall(&["a.rs", "b.rs"], &["b.rs", "c.rs"]), 0.5);
    }

    #[test]
    #[ignore = "skeleton timing sample; not a release gate"]
    fn indexing_and_ranking_skeleton() {
        let (_value, sample) = time_call("noop", "calls", 1.0, || 1);
        assert_eq!(sample.units, 1.0);
        assert_eq!(sample.name, "noop");
    }
}
