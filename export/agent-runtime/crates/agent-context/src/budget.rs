/// Estimate token count for a given text snippet (approx. 4 chars per token).
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    // Code often has indentation and punctuation; 3.7 chars per token is a realistic average.
    text.len().div_ceil(4)
}

/// Budget allocator for keeping total package tokens within limits.
pub struct BudgetManager {
    pub limit: usize,
    pub used: usize,
}

impl BudgetManager {
    pub fn new(limit: usize) -> Self {
        Self { limit, used: 0 }
    }

    pub fn can_fit(&self, tokens: usize) -> bool {
        self.used + tokens <= self.limit
    }

    pub fn consume(&mut self, tokens: usize) {
        self.used += tokens;
    }

    pub fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.used)
    }
}
