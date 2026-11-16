use agent_core::types::{EvidenceMemory, Symbol};
use agent_core::{truncate_chars, SecretPattern};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredFact {
    pub id: String,
    pub claim: String,
    pub relative_path: String,
    pub symbol: Option<String>,
    pub effective_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredFile {
    pub relative_path: String,
    pub score: f64,
    pub reasons: Vec<String>,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredSymbol {
    pub symbol: Symbol,
    pub score: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredTest {
    pub test_file: String,
    pub test_name: Option<String>,
    pub target_symbol: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyNode {
    pub caller: String,
    pub callee: String,
    pub relationship: String,
    pub file_path: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitContextSummary {
    pub head_commit: Option<String>,
    pub branch: Option<String>,
    pub modified_files: Vec<String>,
    pub recent_commits: Vec<RecentCommit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentCommit {
    pub commit_id: String,
    pub author: String,
    pub summary: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetReport {
    pub candidate_tokens: usize,
    pub returned_tokens: usize,
    pub budget_limit: usize,
    pub candidate_files: usize,
    pub returned_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePackage {
    pub task: String,
    pub repo_id: String,
    pub staleness: String,
    pub relevant_files: Vec<ScoredFile>,
    pub relevant_symbols: Vec<ScoredSymbol>,
    pub relevant_tests: Vec<ScoredTest>,
    pub dependencies: Vec<DependencyNode>,
    pub verified_memories: Vec<EvidenceMemory>,
    #[serde(default)]
    pub diagnostic_memories: Vec<EvidenceMemory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relevant_facts: Vec<ScoredFact>,
    pub git_context: GitContextSummary,
    pub budget: BudgetReport,
}

impl EvidencePackage {
    /// Pretty JSON actually delivered by the CLI `--json` path and the MCP context tool.
    pub fn serialized_payload(&self) -> String {
        // Redact text before encoding so escaped secrets are handled correctly
        // and a credential pattern can never consume JSON structure.
        let mut safe = self.clone();
        safe.task = SecretPattern::redact(&safe.task);
        for memory in safe
            .verified_memories
            .iter_mut()
            .chain(&mut safe.diagnostic_memories)
        {
            memory.claim = SecretPattern::redact(&memory.claim);
        }
        for fact in &mut safe.relevant_facts {
            fact.claim = SecretPattern::redact(&fact.claim);
        }
        for sym in &mut safe.relevant_symbols {
            if let Some(doc) = sym.symbol.doc_comment.as_mut() {
                if let Cow::Owned(redacted) = SecretPattern::redact_cow(doc) {
                    *doc = redacted;
                }
            }
        }
        serde_json::to_string_pretty(&safe).unwrap_or_else(|_| "{}".to_string())
    }

    /// Render evidence package as a clean, structured Markdown document for LLM consumption.
    pub fn to_markdown(&self) -> String {
        if self.budget.budget_limit == 0 {
            return String::new();
        }
        let mut md = String::new();
        md.push_str(&format!("# Repository Evidence: {}\n\n", self.task));

        md.push_str(&format!("**Staleness:** {}\n", self.staleness));
        if let Some(ref h) = self.git_context.head_commit {
            md.push_str(&format!("**HEAD Commit:** {}\n", h));
        }
        md.push_str(&format!(
            "**Context Budget:** {} / {} tokens (candidate pool: {} tokens)\n\n",
            self.budget.returned_tokens, self.budget.budget_limit, self.budget.candidate_tokens
        ));

        // Git context
        if !self.git_context.modified_files.is_empty() {
            md.push_str("## Working Tree (Dirty Files)\n");
            for f in &self.git_context.modified_files {
                md.push_str(&format!("- `{}`\n", f));
            }
            md.push('\n');
        }

        // Verified Prior Memories
        if !self.verified_memories.is_empty() {
            md.push_str("## Prior Verified Context\n");
            for m in &self.verified_memories {
                md.push_str(&format!(
                    "- **[{}]** {} (confidence: {:.2})\n",
                    m.id, m.claim, m.confidence
                ));
                for ev in &m.evidence {
                    md.push_str(&format!(
                        "  - source: `{}` (commit {})\n",
                        ev.file,
                        truncate_chars(&ev.commit, 7)
                    ));
                }
            }
            md.push('\n');
        }

        if !self.diagnostic_memories.is_empty() {
            md.push_str("## Diagnostic Context (not verified)\n");
            for m in &self.diagnostic_memories {
                md.push_str(&format!(
                    "- **[{}]** {} (status: {}, confidence: {:.2})\n",
                    m.id, m.claim, m.status, m.confidence
                ));
            }
            md.push('\n');
        }

        if !self.relevant_facts.is_empty() {
            md.push_str("## Anchored Facts\n");
            for f in &self.relevant_facts {
                md.push_str(&format!(
                    "- `{}`: {} (confidence: {:.2})\n",
                    f.relative_path, f.claim, f.effective_confidence
                ));
            }
            md.push('\n');
        }

        // Relevant Implementation Files
        if !self.relevant_files.is_empty() {
            md.push_str("## Relevant Implementation Files\n");
            for f in &self.relevant_files {
                let reasons_str = f.reasons.join(", ");
                md.push_str(&format!(
                    "- `{}` (score: {:.1}; {})\n",
                    f.relative_path, f.score, reasons_str
                ));
                if let Some(ref snippet) = f.snippet {
                    md.push_str("```\n");
                    md.push_str(snippet);
                    if !snippet.ends_with('\n') {
                        md.push('\n');
                    }
                    md.push_str("```\n");
                }
            }
            md.push('\n');
        }

        // Key Symbols
        if !self.relevant_symbols.is_empty() {
            md.push_str("## Relevant Symbols\n");
            for s in &self.relevant_symbols {
                let raw = s.symbol.signature.as_deref().unwrap_or(&s.symbol.name);
                let sig = bound_display(raw, 64);
                md.push_str(&format!(
                    "- `{}` in `{}:{}` ({:?})\n",
                    sig, s.symbol.file_path, s.symbol.start_line, s.symbol.kind
                ));
                if let Some(ref doc) = s.symbol.doc_comment {
                    let doc_summary = truncate_chars(doc.trim(), 160);
                    if !doc_summary.is_empty() {
                        md.push_str(&format!("  - doc: {}\n", doc_summary));
                    }
                }
            }
            md.push('\n');
        }

        // Dependencies & Call Graphs
        if !self.dependencies.is_empty() {
            md.push_str("## Symbol Dependencies & Call Graph\n");
            for d in &self.dependencies {
                md.push_str(&format!(
                    "- `{}` -> `{}` ({}) at `{}:{}`\n",
                    d.caller, d.callee, d.relationship, d.file_path, d.line
                ));
            }
            md.push('\n');
        }

        // Tests
        if !self.relevant_tests.is_empty() {
            md.push_str("## Relevant Tests\n");
            for t in &self.relevant_tests {
                md.push_str(&format!(
                    "- `{}` (references `{}`)\n",
                    t.test_file, t.target_symbol
                ));
            }
            md.push('\n');
        }

        // Recent Commits
        if !self.git_context.recent_commits.is_empty() {
            md.push_str("## Recent Related Commits\n");
            for c in &self.git_context.recent_commits {
                md.push_str(&format!(
                    "- `{}` {} ({})\n",
                    truncate_chars(&c.commit_id, 7),
                    SecretPattern::redact(&c.summary),
                    SecretPattern::redact(&c.author)
                ));
            }
            md.push('\n');
        }

        SecretPattern::redact(&md)
    }
}

fn bound_display(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens.saturating_mul(4);
    truncate_chars(text, max_chars).to_string()
}
