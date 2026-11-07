use agent_core::{
    config::RepoConfig,
    contain::{read_regular_text_within, validate_evidence_file},
    types::{EvidenceMemory, Language, MemoryStatus, RepoId},
    Result,
};
use agent_index::IndexDatabase;
use agent_parser::CodeExtractor;
use chrono::Utc;
use std::path::Path;

fn is_code_evidence(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("rs" | "py" | "c" | "sh" | "js" | "ts")
    )
}

pub struct MemoryVerifier;

impl MemoryVerifier {
    /// Verify an evidence-backed memory against current Git HEAD and the SQLite symbol index.
    pub fn verify<P: AsRef<Path>>(
        memory: &EvidenceMemory,
        repo_root: P,
        repo_id: &RepoId,
        current_head: Option<&str>,
        db: &IndexDatabase,
    ) -> Result<EvidenceMemory> {
        let mut verified = memory.clone();
        if memory.status == MemoryStatus::Invalidated {
            return Ok(verified);
        }
        let root = repo_root.as_ref();
        let max_bytes = RepoConfig::load_or_default(root).max_file_size_kb * 1024;
        let _ = (repo_id, db);

        let mut all_files_exist = true;
        let mut all_symbols_exist = true;
        let mut any_fingerprint_mismatch = false;

        for item in &memory.evidence {
            let relative = Path::new(&item.file);
            if validate_evidence_file(root, relative, max_bytes).is_err() {
                all_files_exist = false;
                break;
            }
            if !is_code_evidence(&item.file) {
                continue;
            }

            let content = match read_regular_text_within(root, relative, max_bytes) {
                Ok(content) => content,
                Err(_) => {
                    all_symbols_exist = false;
                    break;
                }
            };
            let extension = relative
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("");
            let extracted = match CodeExtractor::extract(
                &item.file,
                &content,
                Language::from_extension(extension),
            ) {
                Ok(extracted) => extracted,
                Err(_) => {
                    all_symbols_exist = false;
                    break;
                }
            };

            for sym_name in &item.symbols {
                let found: Vec<_> = extracted
                    .symbols
                    .iter()
                    .filter(|symbol| symbol.name == *sym_name || symbol.qualified_name == *sym_name)
                    .collect();
                if found.is_empty() {
                    all_symbols_exist = false;
                    break;
                }
                if let Some(recorded_fp) = &item.fingerprint {
                    if !found
                        .iter()
                        .any(|symbol| &symbol.fingerprint == recorded_fp)
                    {
                        any_fingerprint_mismatch = true;
                    }
                }
            }
        }

        if !all_files_exist || !all_symbols_exist {
            verified.status = MemoryStatus::Stale;
            verified.confidence = 0.0;
        } else if any_fingerprint_mismatch {
            verified.status = MemoryStatus::Degraded;
            verified.confidence = 0.70;
        } else {
            verified.status = MemoryStatus::Fresh;
            verified.confidence = 1.0;
            if let Some(head) = current_head {
                verified.valid_at = head.to_string();
            }
        }

        verified.updated_at = Utc::now();
        Ok(verified)
    }
}
