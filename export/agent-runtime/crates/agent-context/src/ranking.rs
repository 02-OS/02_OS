use std::collections::HashSet;

/// Extracted search tokens and keywords from a developer task prompt.
#[derive(Debug, Clone)]
pub struct TaskKeywords {
    pub raw: String,
    pub terms: Vec<String>,
}

impl TaskKeywords {
    pub fn parse(task: &str) -> Self {
        let stop_words: HashSet<&'static str> = [
            "a",
            "an",
            "the",
            "and",
            "or",
            "but",
            "if",
            "then",
            "else",
            "when",
            "at",
            "by",
            "for",
            "with",
            "about",
            "against",
            "between",
            "into",
            "through",
            "during",
            "before",
            "after",
            "above",
            "below",
            "to",
            "from",
            "up",
            "down",
            "in",
            "out",
            "on",
            "off",
            "over",
            "under",
            "again",
            "further",
            "then",
            "once",
            "here",
            "there",
            "all",
            "any",
            "both",
            "each",
            "few",
            "more",
            "most",
            "other",
            "some",
            "such",
            "no",
            "nor",
            "not",
            "only",
            "own",
            "same",
            "so",
            "than",
            "too",
            "very",
            "can",
            "will",
            "just",
            "should",
            "now",
            "fix",
            "update",
            "implement",
            "add",
            "make",
            "create",
            "modify",
            "bug",
            "issue",
        ]
        .into_iter()
        .collect();

        // Split on non-alphanumeric except underscores and hyphens
        let mut terms = Vec::new();
        let cleaned = task.replace(
            [
                '/', '\\', ':', ';', ',', '.', '(', ')', '[', ']', '{', '}', '"', '\'', '`',
            ],
            " ",
        );

        for word in cleaned.split_whitespace() {
            let lower = word.to_lowercase();
            if lower.len() >= 2 && !stop_words.contains(lower.as_str()) {
                terms.push(lower.clone());
            }

            // Also split hyphenated terms: refresh-token -> refresh, token
            if word.contains('-') || word.contains('_') {
                for sub in word.split(['-', '_']) {
                    let sub_lower = sub.to_lowercase();
                    if sub_lower.len() >= 2 && !stop_words.contains(sub_lower.as_str()) {
                        terms.push(sub_lower);
                    }
                }
            }
        }

        terms.sort();
        terms.dedup();

        Self {
            raw: task.to_string(),
            terms,
        }
    }
}
