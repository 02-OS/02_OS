use agent_core::{
    paths::StoragePaths, AgentError, Dataset, EdgeKind, Ontology, Provenance, RepoConfig, RepoInfo,
    Result, Session, SourceKind,
};
use agent_git::GitRepo;
use agent_index::{
    commit_node_id, file_node_id, name_node_id, session_node_id, symbol_node_id, GraphExportFormat,
    IndexDatabase, NewDataPoint, PutPoint, RepoScanner, ScannedFile,
};
use agent_parser::CodeExtractor;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Object-safe stage. Built-in stages also implement this so callers can register their own.
pub trait Task {
    fn name(&self) -> &'static str;
    fn run(&self, ctx: &mut TaskContext) -> Result<()>;
}

pub struct Pipeline {
    stages: Vec<Box<dyn Task>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    pub fn push(&mut self, task: impl Task + 'static) {
        self.stages.push(Box::new(task));
    }

    pub fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        for stage in &self.stages {
            stage.run(ctx)?;
        }
        Ok(())
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TaskRegistry {
    stages: Vec<Box<dyn Task>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    pub fn register(&mut self, task: impl Task + 'static) {
        self.stages.push(Box::new(task));
    }

    pub fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        for stage in &self.stages {
            stage.run(ctx)?;
        }
        Ok(())
    }
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RepoHandle {
    pub root: PathBuf,
    pub git: GitRepo,
    pub info: RepoInfo,
    pub db: IndexDatabase,
    pub config: RepoConfig,
}

impl RepoHandle {
    pub fn open(root: &Path) -> Result<Self> {
        let git = GitRepo::open(root)?;
        let info = git.info()?;
        let db_path = StoragePaths::repo_db_path(&info.id)?;
        let mut db = IndexDatabase::open(&db_path)?;
        db.update_repo_info(&info)?;
        Ok(Self {
            root: root.to_path_buf(),
            git,
            info,
            db,
            config: RepoConfig::load_or_default(root),
        })
    }

    pub fn open_with_db(root: &Path, mut db: IndexDatabase) -> Result<Self> {
        let git = GitRepo::open(root)?;
        let info = git.info()?;
        db.update_repo_info(&info)?;
        Ok(Self {
            root: root.to_path_buf(),
            git,
            info,
            db,
            config: RepoConfig::load_or_default(root),
        })
    }

    fn context(
        &mut self,
        dataset_name: Option<&str>,
        full: bool,
        sources: Vec<SourceKind>,
        text: Option<String>,
    ) -> Result<TaskContext<'_>> {
        let name = dataset_name.unwrap_or("default");
        let dataset = self.db.ensure_dataset(&self.info.id, name)?;
        let ontology = Ontology::load(&self.root)?;
        Ok(TaskContext {
            root: &self.root,
            git: &self.git,
            info: &self.info,
            db: &mut self.db,
            config: &self.config,
            ontology,
            dataset,
            full,
            sources,
            text,
            scanned: Vec::new(),
            files_updated: 0,
            datapoints_inserted: 0,
            datapoints_unchanged: 0,
            nodes_written: 0,
            edges_written: 0,
            chunks_written: 0,
            symbols_written: 0,
        })
    }
}

pub struct TaskContext<'a> {
    pub root: &'a Path,
    pub git: &'a GitRepo,
    pub info: &'a RepoInfo,
    pub db: &'a mut IndexDatabase,
    pub config: &'a RepoConfig,
    pub ontology: Ontology,
    pub dataset: Dataset,
    pub full: bool,
    pub sources: Vec<SourceKind>,
    pub text: Option<String>,
    pub scanned: Vec<ScannedFile>,
    pub files_updated: usize,
    pub datapoints_inserted: usize,
    pub datapoints_unchanged: usize,
    pub nodes_written: usize,
    pub edges_written: usize,
    pub chunks_written: usize,
    pub symbols_written: usize,
}

pub struct AddRequest {
    pub sources: Vec<SourceKind>,
    pub dataset_name: Option<String>,
    pub text: Option<String>,
    pub full: bool,
}

pub struct AddReport {
    pub dataset_id: String,
    pub files_updated: usize,
    pub datapoints_inserted: usize,
    pub datapoints_unchanged: usize,
}

pub struct CognifyReport {
    pub dataset_id: String,
    pub nodes: usize,
    pub edges: usize,
    pub chunks: usize,
    pub symbols: usize,
}

struct SyncCatalog;
struct RecordFiles;
struct RecordHistory;
struct RecordNote;
struct ProjectGraph;
struct WriteChunks;
struct LinkCommits;

impl Task for SyncCatalog {
    fn name(&self) -> &'static str {
        "sync-catalog"
    }

    fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        if ctx.scanned.is_empty() {
            load_scan(ctx)?;
        }
        let (symbols, references) = if ctx.full {
            extract_symbols(&ctx.scanned)
        } else {
            let existing = ctx.db.list_files(&ctx.info.id)?;
            let existing_map: HashMap<String, String> = existing
                .into_iter()
                .map(|file| (file.relative_path, file.file_hash))
                .collect();
            let mut current = HashSet::new();
            let mut changed = Vec::new();
            for item in &ctx.scanned {
                current.insert(item.file.relative_path.clone());
                let dirty = match existing_map.get(&item.file.relative_path) {
                    Some(hash) => hash != &item.file.file_hash,
                    None => true,
                };
                if dirty {
                    changed.push(item.clone());
                }
            }
            let deleted: Vec<String> = existing_map
                .keys()
                .filter(|path| !current.contains(*path))
                .cloned()
                .collect();
            let extracted = extract_symbols(&changed);
            let (updated, _, _) = ctx.db.update_incremental(
                &ctx.info.id,
                &changed,
                &deleted,
                &extracted.0,
                &extracted.1,
                ctx.info.head_commit.as_deref(),
            )?;
            ctx.files_updated = updated;
            ctx.symbols_written = extracted.0.len();
            ctx.db.purge_catalog_orphans(&ctx.dataset.id)?;
            return Ok(());
        };
        ctx.symbols_written = symbols.len();
        ctx.files_updated = ctx.db.save_scanned_files(&ctx.info.id, &ctx.scanned)?;
        ctx.db
            .save_symbols_and_references(&ctx.info.id, &symbols, &references)?;
        ctx.db.purge_catalog_orphans(&ctx.dataset.id)?;
        Ok(())
    }
}

impl Task for RecordFiles {
    fn name(&self) -> &'static str {
        "record-files"
    }

    fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        if ctx.scanned.is_empty() {
            load_scan(ctx)?;
        }
        let commit = ctx.info.head_commit.clone();
        let files: Vec<ScannedFile> = ctx
            .scanned
            .iter()
            .filter(|item| include_file(&ctx.sources, &item.file))
            .cloned()
            .collect();
        for item in files {
            let Some(content) = item.content else {
                continue;
            };
            let source_kind = if is_document(&item.file) {
                SourceKind::Document
            } else {
                SourceKind::Worktree
            };
            remember_point(
                ctx,
                "file",
                &item.file.relative_path,
                &content,
                Provenance {
                    source_kind,
                    commit_id: commit.clone(),
                    path: item.file.relative_path.clone(),
                    start_byte: Some(0),
                    end_byte: Some(content.len() as u64),
                    symbol_fingerprint: None,
                },
            )?;
        }
        Ok(())
    }
}

impl Task for RecordHistory {
    fn name(&self) -> &'static str {
        "record-history"
    }

    fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        for commit in ctx.git.get_recent_commits(20)? {
            let content = format!("{} {}", commit.short_id, commit.summary);
            let commit_id = commit.id.clone();
            remember_point(
                ctx,
                "commit",
                &commit_id,
                &content,
                Provenance {
                    source_kind: SourceKind::GitHistory,
                    commit_id: Some(commit_id.clone()),
                    path: String::new(),
                    start_byte: None,
                    end_byte: None,
                    symbol_fingerprint: None,
                },
            )?;
        }
        Ok(())
    }
}

impl Task for RecordNote {
    fn name(&self) -> &'static str {
        "record-note"
    }

    fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        let text = ctx
            .text
            .clone()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AgentError::Config("--text is required for session and text sources".to_string())
            })?;
        let max_bytes = ctx.config.max_file_size_kb.saturating_mul(1024) as usize;
        if text.len() > max_bytes {
            return Err(AgentError::Config(format!(
                "text payload is {} bytes, over the {} KB limit",
                text.len(),
                ctx.config.max_file_size_kb
            )));
        }
        let text = agent_core::SecretPattern::redact(&text);
        let session_note = ctx.sources.contains(&SourceKind::SessionNote);
        let source_kind = if session_note {
            SourceKind::SessionNote
        } else {
            SourceKind::PastedText
        };
        let session_id = agent_core::content_id(
            &ctx.dataset.id,
            "session",
            "",
            &format!("{}:{text}", chrono::Utc::now().timestamp_millis()),
        );
        if session_note {
            let session = Session {
                id: session_id.clone(),
                repo_id: ctx.info.id.clone(),
                dataset_id: ctx.dataset.id.clone(),
                label: Some("agent-session".to_string()),
                ephemeral: true,
                created_at: chrono::Utc::now(),
            };
            ctx.db.ensure_session(&session)?;
            let node = session_node_id(&session.id);
            if ctx.ontology.allows_node("session") {
                ctx.db.upsert_node(
                    ctx.info.id.as_str(),
                    &ctx.dataset.id,
                    &node,
                    "session",
                    session.label.as_deref().unwrap_or("session"),
                    None,
                )?;
                ctx.nodes_written += 1;
            }
        }
        let path = if session_note {
            session_id.clone()
        } else {
            String::new()
        };
        let point_id = remember_point(
            ctx,
            if session_note {
                "session_note"
            } else {
                "pasted_text"
            },
            &path,
            &text,
            Provenance {
                source_kind,
                commit_id: ctx.info.head_commit.clone(),
                path: path.clone(),
                start_byte: Some(0),
                end_byte: Some(text.len() as u64),
                symbol_fingerprint: None,
            },
        )?;
        if session_note {
            write_edge(
                ctx,
                &session_node_id(&session_id),
                &point_id,
                EdgeKind::Owns,
            )?;
        }
        Ok(())
    }
}

impl Task for ProjectGraph {
    fn name(&self) -> &'static str {
        "project-graph"
    }

    fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        if ctx.scanned.is_empty() {
            load_scan(ctx)?;
        }
        let symbols = ctx.db.list_symbols(ctx.info.id.as_str())?;
        let references = ctx.db.list_references(ctx.info.id.as_str())?;
        ctx.db.clear_projected_edges(&ctx.dataset.id)?;

        let keep: Vec<String> = ctx
            .scanned
            .iter()
            .map(|item| item.file.relative_path.clone())
            .collect();
        ctx.db.delete_file_nodes_not_in(&ctx.dataset.id, &keep)?;

        let mut by_file_name: HashMap<(String, String), String> = HashMap::new();
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for item in &ctx.scanned {
            if !ctx.ontology.allows_node("file") {
                continue;
            }
            let id = file_node_id(&ctx.dataset.id, &item.file.relative_path);
            ctx.db.upsert_node(
                ctx.info.id.as_str(),
                &ctx.dataset.id,
                &id,
                "file",
                &item.file.relative_path,
                None,
            )?;
            ctx.nodes_written += 1;
            let set = ctx.db.ensure_node_set(
                &ctx.info.id,
                &format!("lang:{}", item.file.language.as_str()),
            )?;
            ctx.db.add_node_set_member(&set.id, &id)?;
        }

        for symbol in &symbols {
            if !ctx.ontology.allows_node("symbol") {
                continue;
            }
            let id = symbol_node_id(&ctx.dataset.id, &symbol.id);
            ctx.db.upsert_node(
                ctx.info.id.as_str(),
                &ctx.dataset.id,
                &id,
                "symbol",
                &symbol.name,
                None,
            )?;
            ctx.nodes_written += 1;
            by_file_name.insert((symbol.file_path.clone(), symbol.name.clone()), id.clone());
            by_name
                .entry(symbol.name.clone())
                .or_default()
                .push(id.clone());
            let file_id = file_node_id(&ctx.dataset.id, &symbol.file_path);
            ctx.db.upsert_node(
                ctx.info.id.as_str(),
                &ctx.dataset.id,
                &file_id,
                "file",
                &symbol.file_path,
                None,
            )?;
            write_edge(ctx, &file_id, &id, EdgeKind::Owns)?;
        }

        for reference in &references {
            let kind = match reference.kind {
                agent_core::ReferenceKind::Calls => EdgeKind::Calls,
                agent_core::ReferenceKind::Imports => EdgeKind::Imports,
                agent_core::ReferenceKind::Tests => EdgeKind::Tests,
                _ => continue,
            };
            let src = reference
                .source_symbol_name
                .as_ref()
                .and_then(|name| {
                    by_file_name
                        .get(&(reference.source_file.clone(), name.clone()))
                        .cloned()
                })
                .unwrap_or_else(|| file_node_id(&ctx.dataset.id, &reference.source_file));
            ctx.db.upsert_node(
                ctx.info.id.as_str(),
                &ctx.dataset.id,
                &src,
                "symbol",
                reference
                    .source_symbol_name
                    .as_deref()
                    .unwrap_or(reference.source_file.as_str()),
                None,
            )?;
            let (targets, payload) =
                resolve_reference_targets(ctx, reference, &by_name, &by_file_name)?;
            for dst in targets {
                write_edge_with_payload(ctx, &src, &dst, kind, payload.as_deref())?;
            }
        }
        Ok(())
    }
}

fn resolve_reference_targets(
    ctx: &mut TaskContext,
    reference: &agent_core::SymbolReference,
    by_name: &HashMap<String, Vec<String>>,
    by_file_name: &HashMap<(String, String), String>,
) -> Result<(Vec<String>, Option<String>)> {
    match reference.kind {
        agent_core::ReferenceKind::Imports => resolve_import_targets(ctx, reference, by_name),
        agent_core::ReferenceKind::Calls | agent_core::ReferenceKind::Tests => {
            resolve_call_targets(ctx, reference, by_name, by_file_name)
        }
        _ => Ok((Vec::new(), None)),
    }
}

fn resolve_call_targets(
    ctx: &mut TaskContext,
    reference: &agent_core::SymbolReference,
    by_name: &HashMap<String, Vec<String>>,
    by_file_name: &HashMap<(String, String), String>,
) -> Result<(Vec<String>, Option<String>)> {
    let local = (reference.source_file.clone(), reference.target_name.clone());
    if let Some(id) = by_file_name.get(&local) {
        return Ok((vec![id.clone()], None));
    }
    if let Some(ids) = by_name.get(&reference.target_name) {
        if !ids.is_empty() {
            return Ok((ids.clone(), None));
        }
    }
    let synthetic = ensure_name_node(ctx, &reference.target_name)?;
    Ok((vec![synthetic], Some(reference.target_name.clone())))
}

fn resolve_import_targets(
    ctx: &mut TaskContext,
    reference: &agent_core::SymbolReference,
    by_name: &HashMap<String, Vec<String>>,
) -> Result<(Vec<String>, Option<String>)> {
    let raw = reference.target_name.clone();
    let short = import_symbol_name(&raw);
    let hits = by_name.get(&short).map(Vec::as_slice).unwrap_or(&[]);
    if hits.len() == 1 {
        return Ok((vec![hits[0].clone()], Some(raw)));
    }
    let synthetic = ensure_name_node(ctx, &short)?;
    Ok((vec![synthetic], Some(raw)))
}

fn import_symbol_name(target: &str) -> String {
    let segment = target
        .rsplit([':', '/', '.', ' ', '{', ','])
        .find(|part| !part.trim().is_empty())
        .unwrap_or(target);
    segment
        .trim()
        .trim_matches(['"', '\'', '`', '{', '}', ';', ','])
        .trim()
        .to_string()
}

fn ensure_name_node(ctx: &mut TaskContext, name: &str) -> Result<String> {
    let id = name_node_id(&ctx.dataset.id, name);
    ctx.db.upsert_node(
        ctx.info.id.as_str(),
        &ctx.dataset.id,
        &id,
        "name",
        name,
        None,
    )?;
    ctx.nodes_written += 1;
    Ok(id)
}

impl Task for WriteChunks {
    fn name(&self) -> &'static str {
        "write-chunks"
    }

    fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        if ctx.scanned.is_empty() {
            load_scan(ctx)?;
        }
        let commit = ctx.info.head_commit.clone();
        let files: Vec<ScannedFile> = ctx.scanned.clone();
        for item in files {
            let Some(content) = item.content else {
                continue;
            };
            if content.is_empty() {
                continue;
            }
            ctx.db
                .delete_chunks(&ctx.dataset.id, &item.file.relative_path)?;
            let file_id = file_node_id(&ctx.dataset.id, &item.file.relative_path);
            if ctx.ontology.allows_node("file") {
                ctx.db.upsert_node(
                    ctx.info.id.as_str(),
                    &ctx.dataset.id,
                    &file_id,
                    "file",
                    &item.file.relative_path,
                    None,
                )?;
            }
            let source_kind = if is_document(&item.file) {
                SourceKind::Document
            } else {
                SourceKind::Worktree
            };
            for window in chunk_windows(&content) {
                let stored_path = format!(
                    "{}#{}-{}",
                    item.file.relative_path, window.start, window.end
                );
                let point_id = remember_point(
                    ctx,
                    "chunk",
                    &stored_path,
                    "",
                    Provenance {
                        source_kind,
                        commit_id: commit.clone(),
                        path: item.file.relative_path.clone(),
                        start_byte: Some(window.start as u64),
                        end_byte: Some(window.end as u64),
                        symbol_fingerprint: None,
                    },
                )?;
                if ctx.ontology.allows_node("chunk") {
                    ctx.db.upsert_node(
                        ctx.info.id.as_str(),
                        &ctx.dataset.id,
                        &point_id,
                        "chunk",
                        &item.file.relative_path,
                        Some(&point_id),
                    )?;
                    ctx.nodes_written += 1;
                }
                write_edge(ctx, &file_id, &point_id, EdgeKind::Owns)?;
                ctx.chunks_written += 1;
            }
        }
        Ok(())
    }
}

impl Task for LinkCommits {
    fn name(&self) -> &'static str {
        "link-commits"
    }

    fn run(&self, ctx: &mut TaskContext) -> Result<()> {
        if !ctx.ontology.allows_edge(EdgeKind::TouchedInCommit) {
            return Ok(());
        }
        let commits: Vec<_> = ctx
            .db
            .list_datapoints(&ctx.dataset.id)?
            .into_iter()
            .filter(|point| point.kind == "commit")
            .collect();
        for point in commits {
            let Some(commit_id) = point.provenance.commit_id.clone() else {
                continue;
            };
            let node = commit_node_id(&ctx.dataset.id, &commit_id);
            if ctx.ontology.allows_node("commit") {
                ctx.db.upsert_node(
                    ctx.info.id.as_str(),
                    &ctx.dataset.id,
                    &node,
                    "commit",
                    &point.content,
                    Some(&point.id),
                )?;
                ctx.nodes_written += 1;
            }
            let paths = ctx
                .git
                .paths_touched_by_commit(&commit_id)
                .unwrap_or_default();
            for path in paths {
                if !ctx
                    .scanned
                    .iter()
                    .any(|item| item.file.relative_path == path)
                {
                    continue;
                }
                let file_id = file_node_id(&ctx.dataset.id, &path);
                ctx.db.upsert_node(
                    ctx.info.id.as_str(),
                    &ctx.dataset.id,
                    &file_id,
                    "file",
                    &path,
                    None,
                )?;
                write_edge(ctx, &node, &file_id, EdgeKind::TouchedInCommit)?;
            }
        }
        Ok(())
    }
}

pub fn add_pipeline(
    handle: &mut RepoHandle,
    request: &AddRequest,
    extra: &TaskRegistry,
) -> Result<AddReport> {
    let needs_text = request
        .sources
        .iter()
        .any(|source| matches!(source, SourceKind::SessionNote | SourceKind::PastedText));
    if needs_text && request.text.as_deref().unwrap_or("").is_empty() {
        return Err(AgentError::Config(
            "--text is required for session and text sources".to_string(),
        ));
    }
    let mut ctx = handle.context(
        request.dataset_name.as_deref(),
        request.full,
        request.sources.clone(),
        request.text.clone(),
    )?;
    let mut pipeline = Pipeline::new();
    let wants_files = request
        .sources
        .iter()
        .any(|source| matches!(source, SourceKind::Worktree | SourceKind::Document));
    if wants_files {
        pipeline.push(SyncCatalog);
        pipeline.push(RecordFiles);
    }
    if request.sources.contains(&SourceKind::GitHistory) {
        pipeline.push(RecordHistory);
    }
    if needs_text {
        pipeline.push(RecordNote);
    }
    pipeline.run(&mut ctx)?;
    extra.run(&mut ctx)?;
    Ok(AddReport {
        dataset_id: ctx.dataset.id.clone(),
        files_updated: ctx.files_updated,
        datapoints_inserted: ctx.datapoints_inserted,
        datapoints_unchanged: ctx.datapoints_unchanged,
    })
}

pub fn cognify_pipeline(
    handle: &mut RepoHandle,
    full: bool,
    dataset_name: Option<&str>,
    extra: &TaskRegistry,
) -> Result<CognifyReport> {
    let mut ctx = handle.context(
        dataset_name,
        full,
        vec![SourceKind::Worktree, SourceKind::Document],
        None,
    )?;
    SyncCatalog.run(&mut ctx)?;
    run_projection(&mut ctx)?;
    extra.run(&mut ctx)?;
    Ok(CognifyReport {
        dataset_id: ctx.dataset.id.clone(),
        nodes: ctx.nodes_written,
        edges: ctx.edges_written,
        chunks: ctx.chunks_written,
        symbols: ctx.symbols_written,
    })
}

/// Scan once, update the catalog, and project the graph.
pub fn index_pipeline(
    handle: &mut RepoHandle,
    full: bool,
    extra: &TaskRegistry,
) -> Result<AddReport> {
    let mut ctx = handle.context(
        None,
        full,
        vec![SourceKind::Worktree, SourceKind::Document],
        None,
    )?;
    SyncCatalog.run(&mut ctx)?;
    RecordFiles.run(&mut ctx)?;
    run_projection(&mut ctx)?;
    extra.run(&mut ctx)?;
    Ok(AddReport {
        dataset_id: ctx.dataset.id.clone(),
        files_updated: ctx.files_updated,
        datapoints_inserted: ctx.datapoints_inserted,
        datapoints_unchanged: ctx.datapoints_unchanged,
    })
}

fn run_projection(ctx: &mut TaskContext) -> Result<()> {
    ctx.db.begin_immediate()?;
    let outcome = project_stages(ctx);
    match outcome {
        Ok(()) => ctx.db.commit_tx(),
        Err(err) => {
            ctx.db.rollback_tx();
            Err(err)
        }
    }
}

fn project_stages(ctx: &mut TaskContext) -> Result<()> {
    ProjectGraph.run(ctx)?;
    WriteChunks.run(ctx)?;
    LinkCommits.run(ctx)?;
    Ok(())
}

pub fn export_graph(
    handle: &RepoHandle,
    dataset_name: Option<&str>,
    format: GraphExportFormat,
) -> Result<String> {
    let name = dataset_name.unwrap_or("default");
    let dataset = Dataset::named(&handle.info.id, name, chrono::Utc::now());
    handle.db.export_graph(&dataset.id, format)
}

fn load_scan(ctx: &mut TaskContext) -> Result<()> {
    let scanner = RepoScanner::new(ctx.root, ctx.config.clone());
    let mut scanned = scanner.scan();
    for item in &mut scanned {
        if let Ok(Some(blob)) = ctx.git.get_blob_id(&item.file.relative_path) {
            item.file.git_blob_id = Some(blob);
        }
    }
    ctx.scanned = scanned;
    Ok(())
}

fn extract_symbols(
    items: &[ScannedFile],
) -> (Vec<agent_core::Symbol>, Vec<agent_core::SymbolReference>) {
    let mut symbols = Vec::new();
    let mut references = Vec::new();
    for item in items {
        if item.file.kind != agent_core::FileKind::Source && !item.file.is_test {
            continue;
        }
        let Some(text) = &item.content else {
            continue;
        };
        if let Ok(extracted) =
            CodeExtractor::extract(&item.file.relative_path, text, item.file.language)
        {
            symbols.extend(extracted.symbols);
            references.extend(extracted.references);
        }
    }
    (symbols, references)
}

fn include_file(sources: &[SourceKind], file: &agent_core::IndexedFile) -> bool {
    let worktree = sources.contains(&SourceKind::Worktree);
    let docs = sources.contains(&SourceKind::Document);
    match (worktree, docs) {
        (false, false) => false,
        (_, true) if is_document(file) => true,
        (true, false) => true,
        (true, true) => true,
        (false, true) => false,
    }
}

fn is_document(file: &agent_core::IndexedFile) -> bool {
    file.kind == agent_core::FileKind::Documentation
        || file.is_doc
        || is_doc_path(&file.relative_path)
}

fn is_doc_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.starts_with("docs/") || lower.contains("/docs/") || lower.contains("adr")
}

struct ChunkWindow {
    start: usize,
    end: usize,
}

fn chunk_windows(content: &str) -> Vec<ChunkWindow> {
    if content.is_empty() {
        return Vec::new();
    }
    let pieces: Vec<&str> = content.split_inclusive('\n').collect();
    let mut offsets = Vec::with_capacity(pieces.len());
    let mut cursor = 0usize;
    for piece in &pieces {
        offsets.push(cursor);
        cursor += piece.len();
    }
    let window = 80usize;
    let step = 70usize;
    let mut start_line = 0usize;
    let mut windows = Vec::new();
    while start_line < pieces.len() {
        let end_line = (start_line + window).min(pieces.len());
        let start = offsets[start_line];
        let end = if end_line == pieces.len() {
            content.len()
        } else {
            offsets[end_line]
        };
        windows.push(ChunkWindow { start, end });
        if end_line == pieces.len() {
            break;
        }
        start_line += step;
    }
    windows
}

fn remember_point(
    ctx: &mut TaskContext,
    kind: &str,
    path: &str,
    content: &str,
    provenance: Provenance,
) -> Result<String> {
    let label = if provenance.path.is_empty() {
        kind.to_string()
    } else {
        provenance.path.clone()
    };
    let stored_path = path.to_string();
    match ctx.db.put_datapoint(NewDataPoint {
        repo_id: ctx.info.id.clone(),
        dataset_id: ctx.dataset.id.clone(),
        kind: kind.to_string(),
        path: stored_path,
        content: content.to_string(),
        provenance,
        confidence: 1.0,
    })? {
        PutPoint::Unchanged { id } => {
            ctx.datapoints_unchanged += 1;
            Ok(id)
        }
        PutPoint::Inserted(inserted) => {
            ctx.datapoints_inserted += 1;
            ctx.db.upsert_node(
                ctx.info.id.as_str(),
                &ctx.dataset.id,
                &inserted.id,
                kind,
                &label,
                Some(&inserted.id),
            )?;
            if let Some(previous) = inserted.previous_id {
                ctx.db.upsert_node(
                    ctx.info.id.as_str(),
                    &ctx.dataset.id,
                    &previous,
                    kind,
                    &label,
                    Some(&previous),
                )?;
                write_edge(ctx, &inserted.id, &previous, EdgeKind::Supersedes)?;
            }
            Ok(inserted.id)
        }
    }
}

fn write_edge(ctx: &mut TaskContext, src: &str, dst: &str, kind: EdgeKind) -> Result<()> {
    write_edge_with_payload(ctx, src, dst, kind, None)
}

fn write_edge_with_payload(
    ctx: &mut TaskContext,
    src: &str,
    dst: &str,
    kind: EdgeKind,
    payload: Option<&str>,
) -> Result<()> {
    if !ctx.ontology.allows_edge(kind) || src == dst {
        return Ok(());
    }
    if ctx.db.upsert_edge_with_payload(
        ctx.info.id.as_str(),
        &ctx.dataset.id,
        src,
        dst,
        kind,
        payload,
    )? {
        ctx.edges_written += 1;
    }
    Ok(())
}
