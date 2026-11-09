use crate::{
    budget::{estimate_tokens, BudgetManager},
    ranking::TaskKeywords,
    score::{is_entry_path, score_file, ScoreBreakdown, ScoreInput},
    types::{
        BudgetReport, DependencyNode, EvidencePackage, GitContextSummary, RecentCommit, ScoredFile,
        ScoredSymbol, ScoredTest,
    },
};
use agent_core::{
    config::RepoConfig,
    contain::{read_indexed_bytes, validate_evidence_file},
    paths::StoragePaths,
    types::{MemoryStatus, ReferenceKind, SecretPattern},
    AgentError, Result,
};
use agent_git::GitRepo;
use agent_index::{IndexDatabase, RepoScanner};
use agent_memory::{MemoryStore, StalenessEngine};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct ContextCompiler;

impl ContextCompiler {
    /// Compile a task-focused, token-budgeted evidence package.
    pub fn compile<P: AsRef<Path>>(
        repo_root: P,
        task: &str,
        token_budget: usize,
    ) -> Result<EvidencePackage> {
        // Reject an impossible envelope before scanning evidence. A zero budget
        // cannot represent even the JSON metadata of a successful response.
        let mut minimum = empty_package(task, token_budget);
        enforce_budget(&mut minimum)?;
        let git_repo = GitRepo::open(repo_root)?;
        let root = git_repo.root_path();
        let max_bytes = RepoConfig::load_or_default(root).max_file_size_kb * 1024;
        let repo_info = git_repo.info()?;
        let repo_id = &repo_info.id;

        let db_path = StoragePaths::repo_db_path(repo_id)?;
        if !db_path.exists() {
            return Err(AgentError::General(
                "Repository is not indexed yet. Run '02 init' or '02 index' first.".to_string(),
            ));
        }

        let db = IndexDatabase::open(&db_path)?;
        let store = MemoryStore::for_repo(repo_id)?;

        // 1. Git staleness & diff detection
        let diff_files = git_repo.get_diff_files()?;
        let scored_diffs: Vec<String> = diff_files
            .iter()
            .filter(|path| !SecretPattern::is_secret(Path::new(path.as_str())))
            .cloned()
            .collect();
        StalenessEngine::evaluate(root, repo_id, &git_repo, &db, &store)?;

        let head_commit_str = repo_info
            .head_commit
            .clone()
            .unwrap_or_else(|| "none".to_string());
        let short_head = if head_commit_str.len() >= 7 {
            &head_commit_str[..7]
        } else {
            &head_commit_str
        };

        let completed = db.completed_index(repo_id)?;
        let index_fresh = completed.is_some_and(|state| {
            state.head_commit == repo_info.head_commit
                && state.fingerprint
                    == RepoScanner::new(root, RepoConfig::load_or_default(root)).fingerprint()
        });
        let staleness_str = if index_fresh {
            format!("fresh against indexed snapshot ({})", short_head)
        } else {
            format!(
                "index stale against HEAD ({}) [{} modified tracked files]; run 02 index",
                short_head,
                diff_files.len()
            )
        };

        // 2. Parse task keywords
        let keywords = TaskKeywords::parse(task);

        // 3. Multi-signal Candidate Collection & Scoring
        let mut file_scores: HashMap<String, (f64, Vec<String>)> = HashMap::new();
        let mut candidate_symbols: Vec<ScoredSymbol> = Vec::new();
        let mut candidate_deps: Vec<DependencyNode> = Vec::new();
        let mut candidate_tests: Vec<ScoredTest> = Vec::new();
        let mut seen_symbols: HashSet<String> = HashSet::new();

        // Signal A: Working tree diff
        let diff_set: HashSet<&str> = scored_diffs.iter().map(|s| s.as_str()).collect();
        for df in &scored_diffs {
            let entry = file_scores.entry(df.clone()).or_insert((0.0, Vec::new()));
            entry.0 += 8.0;
            entry.1.push("modified in working tree".to_string());
        }

        // Signal B: Exact & partial symbol match
        for term in &keywords.terms {
            let mut matched_syms = Vec::new();
            if let Ok(syms) = db.find_symbols_by_name(repo_id, term) {
                for s in syms {
                    matched_syms.push((s, 10.0, "exact symbol match in task".to_string()));
                }
            }
            if let Ok(syms) = db.search_symbols(repo_id, term, 25) {
                for s in syms {
                    matched_syms.push((s, 7.0, format!("partial symbol match with '{}'", term)));
                }
            }

            for (sym, base_score, reason) in matched_syms {
                if SecretPattern::is_secret(Path::new(&sym.file_path)) {
                    continue;
                }
                let mut sym_score = base_score;
                let mut reasons = vec![reason];

                if diff_set.contains(sym.file_path.as_str()) {
                    sym_score += 8.0;
                    reasons.push("symbol in dirty working-tree file".to_string());
                }

                // Boost file containing this symbol
                let entry = file_scores
                    .entry(sym.file_path.clone())
                    .or_insert((0.0, Vec::new()));
                entry.0 += sym_score;
                entry.1.push(format!("defines symbol '{}'", sym.name));

                if seen_symbols.insert(sym.id.to_string()) {
                    candidate_symbols.push(ScoredSymbol {
                        symbol: sym.clone(),
                        score: sym_score,
                        reasons,
                    });
                }

                // Signal C: Dependencies & callers
                if let Ok(deps) = db.find_symbol_deps(repo_id, &sym.name) {
                    for r in deps.callers {
                        let caller_file = r.source_file.clone();
                        let caller_name = r
                            .source_symbol_name
                            .clone()
                            .unwrap_or_else(|| "<top-level>".to_string());

                        let entry = file_scores
                            .entry(caller_file.clone())
                            .or_insert((0.0, Vec::new()));
                        entry.0 += 5.0;
                        entry
                            .1
                            .push(format!("calls '{}' from '{}'", sym.name, caller_name));

                        candidate_deps.push(DependencyNode {
                            caller: caller_name,
                            callee: sym.name.clone(),
                            relationship: r.kind.to_string(),
                            file_path: caller_file,
                            line: r.line_number,
                        });
                    }

                    for r in deps.callees {
                        let entry = file_scores
                            .entry(r.source_file.clone())
                            .or_insert((0.0, Vec::new()));
                        entry.0 += 4.0;
                        entry.1.push(format!("called by '{}'", sym.name));

                        candidate_deps.push(DependencyNode {
                            caller: sym.name.clone(),
                            callee: r.target_name,
                            relationship: r.kind.to_string(),
                            file_path: r.source_file,
                            line: r.line_number,
                        });
                    }
                }

                // Signal D: Relevant tests
                if let Ok(tests) = db.find_tests_for_symbol(repo_id, &sym.name) {
                    for t in tests {
                        let test_file = t.source_file.clone();
                        let entry = file_scores
                            .entry(test_file.clone())
                            .or_insert((0.0, Vec::new()));
                        entry.0 += 6.0;
                        entry.1.push(format!("tests symbol '{}'", sym.name));

                        candidate_tests.push(ScoredTest {
                            test_file,
                            test_name: t.source_symbol_name,
                            target_symbol: sym.name.clone(),
                            score: 6.0,
                        });
                    }
                }
            }
        }

        // Signal E: BM25 Full-text search
        if let Ok(search_hits) = db.search_content(task, 15) {
            for (idx, hit) in search_hits.into_iter().enumerate() {
                let bm25_boost = 6.0 - (idx as f64 * 0.3).min(4.0);
                let entry = file_scores
                    .entry(hit.relative_path.clone())
                    .or_insert((0.0, Vec::new()));
                entry.0 += bm25_boost;
                entry
                    .1
                    .push(format!("BM25 search match (rank #{})", idx + 1));
            }
        }

        // Signal F: Prior verified memories
        let memories = store.load_all()?;
        let mut relevant_memories = Vec::new();
        let mut diagnostic_memories = Vec::new();
        for mem in memories {
            if estimate_tokens(&mem.claim) > token_budget {
                continue;
            }
            let mut matched = false;
            let claim_lower = mem.claim.to_lowercase();
            for term in &keywords.terms {
                if claim_lower.contains(term) {
                    matched = true;
                    break;
                }
            }
            if !matched {
                for ev in &mem.evidence {
                    if file_scores.contains_key(&ev.file)
                        || ev
                            .symbols
                            .iter()
                            .any(|s| keywords.terms.contains(&s.to_lowercase()))
                    {
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                continue;
            }
            let fresh = mem.status == MemoryStatus::Fresh && mem.confidence > 0.0;
            if fresh {
                for ev in &mem.evidence {
                    if validate_evidence_file(root, Path::new(&ev.file), max_bytes).is_err() {
                        continue;
                    }
                    let entry = file_scores
                        .entry(ev.file.clone())
                        .or_insert((0.0, Vec::new()));
                    entry.0 += 4.0;
                    entry.1.push(format!("supported by memory '{}'", mem.id));
                }
                relevant_memories.push(mem);
            } else {
                diagnostic_memories.push(mem);
            }
        }

        // Signal G: Recent commits touching relevant files
        let all_recent = git_repo.get_recent_commits(15).unwrap_or_default();
        let mut relevant_commits = Vec::new();
        for c in all_recent {
            relevant_commits.push(RecentCommit {
                commit_id: c.id,
                author: SecretPattern::redact(&c.author),
                summary: SecretPattern::redact(&c.summary),
                timestamp: c.timestamp,
            });
        }

        // Sort files by score descending
        let mut sorted_files: Vec<(String, f64, Vec<String>)> = file_scores
            .into_iter()
            .map(|(path, (score, reasons))| (path, score, reasons))
            .collect();
        sorted_files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Sort symbols by score descending
        candidate_symbols.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 4. Token Budget Allocation & Snippet Generation
        let mut budget_manager = BudgetManager::new(token_budget);

        // Estimate header, git context, memory tokens
        let mut base_metadata = format!("# {}\n{}\n{}", task, staleness_str, head_commit_str);
        for m in &relevant_memories {
            base_metadata.push_str(&m.claim);
        }
        for d in &candidate_deps {
            base_metadata.push_str(&d.caller);
            base_metadata.push_str(&d.callee);
        }
        for t in &candidate_tests {
            base_metadata.push_str(&t.test_file);
        }
        let base_tokens = estimate_tokens(&base_metadata);
        budget_manager.consume(base_tokens.min(token_budget / 3));

        let candidate_files_count = sorted_files.len();
        // Read and budget only the files that can be returned.
        sorted_files.truncate(15);
        let mut returned_files = Vec::new();
        let mut total_candidate_tokens = base_tokens;

        for (path, score, reasons) in sorted_files {
            if budget_manager.remaining() <= 100 {
                break;
            }

            let full_path = root.join(&path);
            let content_opt = read_indexed_bytes(root, &full_path, max_bytes)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .map(|text| SecretPattern::redact(&text));

            if let Some(ref content) = content_opt {
                let file_tokens = estimate_tokens(content);
                total_candidate_tokens += file_tokens;

                let snippet =
                    if budget_manager.can_fit(file_tokens) && content.lines().count() <= 80 {
                        budget_manager.consume(file_tokens);
                        Some(content.clone())
                    } else if budget_manager.remaining() > 100 {
                        // Extract focused excerpt around matched symbols
                        let target_lines: Vec<usize> = candidate_symbols
                            .iter()
                            .filter(|s| s.symbol.file_path == path)
                            .map(|s| s.symbol.start_line)
                            .collect();

                        let excerpt =
                            extract_excerpt(content, &target_lines, budget_manager.remaining());
                        let excerpt_tokens = estimate_tokens(&excerpt);
                        budget_manager.consume(excerpt_tokens);
                        Some(excerpt)
                    } else {
                        None
                    };

                returned_files.push(ScoredFile {
                    relative_path: path,
                    score,
                    reasons,
                    snippet,
                });
            } else {
                returned_files.push(ScoredFile {
                    relative_path: path,
                    score,
                    reasons,
                    snippet: None,
                });
            }
        }

        candidate_symbols.truncate(12);
        for scored in &mut candidate_symbols {
            if let Some(signature) = scored.symbol.signature.as_mut() {
                *signature = bound_text(&SecretPattern::redact(signature), 64);
            }
        }
        candidate_deps.truncate(10);
        candidate_tests.truncate(8);

        let git_summary = GitContextSummary {
            head_commit: repo_info.head_commit,
            branch: repo_info.branch,
            modified_files: scored_diffs,
            recent_commits: relevant_commits,
        };

        let budget_report = BudgetReport {
            candidate_tokens: total_candidate_tokens,
            returned_tokens: budget_manager.used,
            budget_limit: token_budget,
            candidate_files: candidate_files_count,
            returned_files: returned_files.len(),
        };

        let mut package = EvidencePackage {
            task: SecretPattern::redact(task),
            repo_id: repo_id.to_string(),
            staleness: staleness_str,
            relevant_files: returned_files,
            relevant_symbols: candidate_symbols,
            relevant_tests: candidate_tests,
            dependencies: candidate_deps,
            verified_memories: relevant_memories,
            diagnostic_memories,
            git_context: git_summary,
            budget: budget_report,
        };
        enforce_budget(&mut package)?;
        Ok(package)
    }

    /// Rank indexed files for a task.
    ///
    /// `compile` is still the path that redacts and packs evidence. This method
    /// is the multi-signal ranking kernel used by navigation tools.
    pub fn rank<P: AsRef<Path>>(repo_root: P, task: &str) -> Result<Vec<ScoreBreakdown>> {
        let git_repo = GitRepo::open(repo_root)?;
        let repo_info = git_repo.info()?;
        let repo_id = &repo_info.id;
        let db_path = StoragePaths::repo_db_path(repo_id)?;
        if !db_path.exists() {
            return Err(AgentError::General(
                "Repository is not indexed yet. Run '02 init' or '02 index' first.".to_string(),
            ));
        }
        let db = IndexDatabase::open(&db_path)?;
        let keywords = TaskKeywords::parse(task);
        let mut exact_ids = HashSet::new();
        let mut exact_counts: HashMap<String, usize> = HashMap::new();
        let mut partial_counts: HashMap<String, usize> = HashMap::new();
        for term in &keywords.terms {
            if let Ok(symbols) = db.find_symbols_by_name(repo_id, term) {
                for symbol in symbols {
                    exact_ids.insert(symbol.id.clone());
                    *exact_counts.entry(symbol.file_path).or_insert(0) += 1;
                }
            }
            if let Ok(symbols) = db.search_symbols(repo_id, term, 25) {
                for symbol in symbols {
                    if exact_ids.contains(&symbol.id) {
                        continue;
                    }
                    *partial_counts.entry(symbol.file_path).or_insert(0) += 1;
                }
            }
        }

        let mut tested_files = HashSet::new();
        if let Ok(references) = db.list_references(repo_id.as_str()) {
            let symbols = db.list_symbols(repo_id.as_str()).unwrap_or_default();
            let symbol_files: HashMap<&str, &str> = symbols
                .iter()
                .map(|symbol| (symbol.name.as_str(), symbol.file_path.as_str()))
                .collect();
            for reference in &references {
                if reference.kind == ReferenceKind::Tests {
                    if let Some(path) = symbol_files.get(reference.target_name.as_str()) {
                        tested_files.insert((*path).to_string());
                    }
                }
            }
        }

        let mut touches: HashMap<String, usize> = HashMap::new();
        if let Ok(commits) = git_repo.get_recent_commits(15) {
            for commit in commits {
                if let Ok(paths) = git_repo.paths_touched_by_commit(&commit.id) {
                    for path in paths {
                        *touches.entry(path).or_insert(0) += 1;
                    }
                }
            }
        }
        let completed = db.completed_index(repo_id)?;
        let index_stale =
            !completed.is_some_and(|state| state.head_commit == repo_info.head_commit);

        let mut ranked = Vec::new();
        for file in db.list_files(repo_id)? {
            if SecretPattern::is_secret(Path::new(&file.relative_path)) {
                continue;
            }
            let test_distance = if file.is_test
                && (exact_counts.contains_key(&file.relative_path)
                    || partial_counts.contains_key(&file.relative_path))
            {
                Some(0)
            } else if tested_files.contains(&file.relative_path) {
                Some(1)
            } else {
                None
            };
            ranked.push(score_file(&ScoreInput {
                path: &file.relative_path,
                exact_symbols: exact_counts.get(&file.relative_path).copied().unwrap_or(0),
                partial_symbols: partial_counts
                    .get(&file.relative_path)
                    .copied()
                    .unwrap_or(0),
                test_distance,
                commits_touching: touches.get(&file.relative_path).copied().unwrap_or(0),
                kind: &file.kind.to_string(),
                is_entry: is_entry_path(&file.relative_path),
                index_stale,
            }));
        }
        ranked.sort_by(|left, right| {
            right
                .total
                .total_cmp(&left.total)
                .then_with(|| left.path.cmp(&right.path))
        });
        Ok(ranked)
    }
}

fn empty_package(task: &str, token_budget: usize) -> EvidencePackage {
    EvidencePackage {
        task: SecretPattern::redact(task),
        repo_id: String::new(),
        staleness: String::new(),
        relevant_files: Vec::new(),
        relevant_symbols: Vec::new(),
        relevant_tests: Vec::new(),
        dependencies: Vec::new(),
        verified_memories: Vec::new(),
        diagnostic_memories: Vec::new(),
        git_context: GitContextSummary {
            head_commit: None,
            branch: None,
            modified_files: Vec::new(),
            recent_commits: Vec::new(),
        },
        budget: BudgetReport {
            candidate_tokens: 0,
            returned_tokens: 0,
            budget_limit: token_budget,
            candidate_files: 0,
            returned_files: 0,
        },
    }
}

fn bound_text(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens.saturating_mul(4);
    let mut end = text.len().min(max_chars);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn payload_tokens(package: &EvidencePackage) -> usize {
    estimate_tokens(&package.serialized_payload())
}

/// Stabilize the self-reported usage (its decimal digits are part of the payload).
fn update_usage(package: &mut EvidencePackage) -> usize {
    package.budget.returned_files = package.relevant_files.len();
    package.budget.returned_tokens = 0;
    loop {
        let tokens = payload_tokens(package);
        if tokens == package.budget.returned_tokens {
            return tokens;
        }
        package.budget.returned_tokens = tokens;
    }
}

fn removable_items(package: &EvidencePackage) -> usize {
    package
        .relevant_files
        .iter()
        .filter(|file| file.snippet.is_some())
        .count()
        + package.relevant_symbols.len()
        + package.relevant_files.len()
        + package.git_context.recent_commits.len()
        + package.verified_memories.len()
        + package.diagnostic_memories.len()
        + package.dependencies.len()
        + package.relevant_tests.len()
        + usize::from(!package.git_context.modified_files.is_empty())
}

/// Keep the existing removal priority while pruning whole groups in one pass.
fn drop_budget_items(package: &mut EvidencePackage, mut count: usize) {
    for file in package.relevant_files.iter_mut().rev() {
        if count == 0 {
            return;
        }
        if file.snippet.take().is_some() {
            count -= 1;
        }
    }
    macro_rules! trim {
        ($items:expr) => {{
            let drop = count.min($items.len());
            $items.truncate($items.len() - drop);
            count -= drop;
            if count == 0 {
                return;
            }
        }};
    }
    trim!(package.relevant_symbols);
    trim!(package.relevant_files);
    trim!(package.git_context.recent_commits);
    trim!(package.verified_memories);
    trim!(package.diagnostic_memories);
    trim!(package.dependencies);
    trim!(package.relevant_tests);
    if count > 0 {
        package.git_context.modified_files.clear();
    }
}

fn enforce_budget(package: &mut EvidencePackage) -> Result<()> {
    let limit = package.budget.budget_limit;
    if update_usage(package) <= limit {
        return Ok(());
    }
    let items = removable_items(package);
    let mut minimum = package.clone();
    drop_budget_items(&mut minimum, items);
    let required = update_usage(&mut minimum);
    if required > limit {
        return Err(AgentError::General(format!(
            "Context metadata requires at least {required} estimated tokens; budget is {limit}. Shorten the task or increase the budget."
        )));
    }
    // Search the smallest number of removals that fits. Each probe serializes
    // once per usage stabilization, instead of serializing after every item.
    let mut low = 1;
    let mut high = items;
    while low < high {
        let middle = low + (high - low) / 2;
        let mut candidate = package.clone();
        drop_budget_items(&mut candidate, middle);
        if update_usage(&mut candidate) <= limit {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    drop_budget_items(package, low);
    update_usage(package);
    Ok(())
}

/// Extract focused window around line numbers, respecting remaining token allowance.
fn extract_excerpt(content: &str, target_lines: &[usize], max_tokens: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut included_lines = HashSet::new();

    if target_lines.is_empty() {
        // Take the top lines up to token limit
        let take_count = (max_tokens * 3).min(lines.len());
        for i in 0..take_count {
            included_lines.insert(i + 1);
        }
    } else {
        // Window of 5 lines before and 10 lines after each target symbol
        for &t in target_lines {
            let start = t.saturating_sub(5).max(1);
            let end = (t + 10).min(lines.len());
            for l in start..=end {
                included_lines.insert(l);
            }
        }
    }

    let mut line_nums: Vec<usize> = included_lines.into_iter().collect();
    line_nums.sort();

    let mut result = String::new();
    let mut last_line = 0;

    for &num in &line_nums {
        if estimate_tokens(&result) >= max_tokens {
            break;
        }
        if last_line != 0
            && num > last_line + 1
            && !push_bounded(&mut result, "    ...\n", max_tokens)
        {
            break;
        }
        if num <= lines.len() {
            let rendered = format!("{:4}: {}\n", num, lines[num - 1]);
            if !push_bounded(&mut result, &rendered, max_tokens) {
                break;
            }
        }
        last_line = num;
    }

    result
}

fn push_bounded(out: &mut String, text: &str, max_tokens: usize) -> bool {
    let used = estimate_tokens(out);
    if used >= max_tokens {
        return false;
    }
    let room = max_tokens - used;
    let piece = bound_text(text, room);
    if piece.is_empty() {
        return false;
    }
    out.push_str(&piece);
    true
}

#[cfg(test)]
mod budget_regressions {
    use super::*;
    use agent_core::{EvidenceMemory, MemoryKind};
    use chrono::Utc;

    fn large_package(count: usize) -> EvidencePackage {
        let mut package = empty_package("architecture", 2000);
        let now = Utc::now();
        package.verified_memories = (0..count)
            .map(|index| EvidenceMemory {
                id: format!("mem_{index}"),
                claim: "documented architecture constraint ".repeat(20),
                kind: MemoryKind::ArchitecturalFact,
                evidence: Vec::new(),
                valid_at: "fixture".into(),
                confidence: 1.0,
                status: MemoryStatus::Fresh,
                created_at: now,
                updated_at: now,
            })
            .collect();
        package
    }

    #[test]
    fn rejects_zero_and_oversized_task_metadata() {
        for (task, limit) in [("short".to_string(), 0), ("🌀\\\"\n".repeat(1000), 200)] {
            let error = enforce_budget(&mut empty_package(&task, limit)).unwrap_err();
            assert!(error.to_string().contains("Context metadata requires"));
        }
    }

    #[test]
    fn budgets_the_final_redacted_serialization_and_keeps_maximal_evidence() {
        let token = format!("ghp_{}", "a".repeat(36));
        let mut package = large_package(50);
        package.task = format!("architecture {token}");
        let original = package.clone();
        enforce_budget(&mut package).unwrap();
        let payload = package.serialized_payload();
        assert!(!payload.contains(&token));
        serde_json::from_str::<serde_json::Value>(&payload).unwrap();
        assert_eq!(package.budget.returned_tokens, estimate_tokens(&payload));
        assert!(package.budget.returned_tokens <= package.budget.budget_limit);
        assert!(!package.verified_memories.is_empty());
        let removals = original.verified_memories.len() - package.verified_memories.len();
        let mut one_more = original;
        drop_budget_items(&mut one_more, removals - 1);
        assert!(update_usage(&mut one_more) > package.budget.budget_limit);
    }

    #[test]
    #[ignore = "comparison benchmark; run explicitly with --ignored --nocapture"]
    fn benchmark_budget_pruning() {
        let original = large_package(512);
        let mut sequential = original.clone();
        let start = std::time::Instant::now();
        // Reproduce the previous sequential serialization loop on identical
        // redacted data. This isolates pruning from other compiler changes.
        for _ in 0..8192 {
            let tokens = payload_tokens(&sequential);
            if tokens <= sequential.budget.budget_limit {
                if sequential.budget.returned_tokens == tokens {
                    break;
                }
                sequential.budget.returned_tokens = tokens;
            } else {
                drop_budget_items(&mut sequential, 1);
                sequential.budget.returned_files = sequential.relevant_files.len();
            }
        }
        let sequential_us = start.elapsed().as_micros();
        let mut optimized = original;
        let start = std::time::Instant::now();
        enforce_budget(&mut optimized).unwrap();
        let optimized_us = start.elapsed().as_micros();
        assert_eq!(
            sequential.serialized_payload(),
            optimized.serialized_payload()
        );
        println!("budget_pruning: sequential_us={sequential_us} optimized_us={optimized_us} memories=512 retained={}", optimized.verified_memories.len());
    }
}
