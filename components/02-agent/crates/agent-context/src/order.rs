use std::collections::HashMap;

/// Order evidence files for `compile`.
///
/// The first key is the score `compile` already computed. Context tests lock
/// that order. The ranking kernel breaks ties, and the path orders whatever
/// remains so a hash map cannot change which file fits a tight budget.
pub fn order_compiled_files(
    files: &mut [(String, f64, Vec<String>)],
    rank_total: &HashMap<String, f64>,
) {
    files.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| {
                let right_rank = rank_total.get(&right.0).copied().unwrap_or(0.0);
                let left_rank = rank_total.get(&left.0).copied().unwrap_or(0.0);
                right_rank.total_cmp(&left_rank)
            })
            .then_with(|| left.0.cmp(&right.0))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_order_keeps_compile_score_ahead_of_rank() {
        let mut files = vec![
            ("src/low.rs".to_string(), 4.0, Vec::new()),
            ("src/a.rs".to_string(), 8.0, Vec::new()),
            ("src/z.rs".to_string(), 20.0, Vec::new()),
            ("src/b.rs".to_string(), 8.0, Vec::new()),
            ("src/m.rs".to_string(), 5.0, Vec::new()),
            ("src/k.rs".to_string(), 5.0, Vec::new()),
        ];
        let mut rank = HashMap::new();
        rank.insert("src/low.rs".to_string(), 100.0);
        rank.insert("src/a.rs".to_string(), 1.0);
        rank.insert("src/z.rs".to_string(), 0.0);
        rank.insert("src/b.rs".to_string(), 9.0);
        rank.insert("src/m.rs".to_string(), 2.0);
        rank.insert("src/k.rs".to_string(), 2.0);

        order_compiled_files(&mut files, &rank);

        let paths: Vec<&str> = files.iter().map(|file| file.0.as_str()).collect();
        assert_eq!(
            paths,
            [
                "src/z.rs",
                "src/b.rs",
                "src/a.rs",
                "src/k.rs",
                "src/m.rs",
                "src/low.rs",
            ]
        );
    }
}
