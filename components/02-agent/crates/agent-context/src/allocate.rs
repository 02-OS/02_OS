/// One candidate the budget allocator can keep or drop.
#[derive(Debug, Clone)]
pub struct BudgetItem {
    pub id: String,
    pub score: f64,
    pub tokens: usize,
}

/// Greedy allocation. Higher score wins. Equal scores prefer the smaller item.
/// Items that do not fit are skipped, and a later smaller item may still fit.
pub fn allocate(items: &[BudgetItem], budget: usize, max_items: usize) -> Vec<String> {
    if budget == 0 || max_items == 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&left, &right| {
        items[right]
            .score
            .total_cmp(&items[left].score)
            .then_with(|| items[left].tokens.cmp(&items[right].tokens))
            .then_with(|| items[left].id.cmp(&items[right].id))
    });
    let mut used = 0;
    let mut chosen = Vec::new();
    for index in order {
        if chosen.len() == max_items {
            break;
        }
        let item = &items[index];
        if item.tokens > budget.saturating_sub(used) {
            continue;
        }
        used += item.tokens;
        chosen.push(item.id.clone());
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_prefers_score_and_still_fills_a_gap() {
        let items = vec![
            BudgetItem {
                id: "huge".into(),
                score: 10.0,
                tokens: 100,
            },
            BudgetItem {
                id: "best".into(),
                score: 9.0,
                tokens: 40,
            },
            BudgetItem {
                id: "fits".into(),
                score: 8.0,
                tokens: 30,
            },
            BudgetItem {
                id: "rest".into(),
                score: 1.0,
                tokens: 10,
            },
        ];
        assert_eq!(allocate(&items, 70, 3), vec!["best", "fits"]);
        assert!(allocate(&items, 0, 3).is_empty());
    }
}
