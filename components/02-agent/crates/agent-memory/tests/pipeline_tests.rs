use agent_core::Result;
use agent_core::{EdgeKind, SourceKind};
use agent_index::{file_node_id, symbol_node_id, GraphExportFormat, IndexDatabase};
use agent_memory::{
    add_pipeline, cognify_pipeline, export_graph, index_pipeline, AddRequest, CognifyReport,
    RepoHandle, Task, TaskContext, TaskRegistry,
};
use git2::Repository;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

fn git_repo(root: &std::path::Path) {
    let repo = Repository::init(root).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn target() {}\npub fn caller() { target(); }\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/note.md"),
        "# Note\nA documented decision.\n",
    )
    .unwrap();
    fs::write(root.join(".env"), "TOKEN=secret\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("src/lib.rs")).unwrap();
    index
        .add_path(std::path::Path::new("docs/note.md"))
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
}

struct CountingTask {
    hits: &'static AtomicUsize,
}

impl Task for CountingTask {
    fn name(&self) -> &'static str {
        "counting"
    }

    fn run(&self, _ctx: &mut TaskContext) -> Result<()> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn add_cognify_export_and_supersede() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_repo(root);
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();

    static HITS: AtomicUsize = AtomicUsize::new(0);
    let mut extra = TaskRegistry::new();
    extra.register(CountingTask { hits: &HITS });

    let first = add_pipeline(
        &mut handle,
        &AddRequest {
            sources: vec![
                SourceKind::Worktree,
                SourceKind::Document,
                SourceKind::GitHistory,
            ],
            dataset_name: None,
            text: None,
            full: false,
        },
        &extra,
    )
    .unwrap();
    assert!(first.files_updated >= 2);
    assert!(first.datapoints_inserted >= 2);
    assert_eq!(HITS.load(Ordering::SeqCst), 1);

    let again = add_pipeline(
        &mut handle,
        &AddRequest {
            sources: vec![SourceKind::Worktree, SourceKind::Document],
            dataset_name: None,
            text: None,
            full: false,
        },
        &TaskRegistry::new(),
    )
    .unwrap();
    assert_eq!(again.datapoints_inserted, 0);
    assert!(again.datapoints_unchanged >= 2);

    let report: CognifyReport =
        cognify_pipeline(&mut handle, false, None, &TaskRegistry::new()).unwrap();
    assert!(report.chunks >= 1, "chunks {}", report.chunks);
    assert!(report.edges >= 1, "edges {}", report.edges);
    let symbols = handle.db.list_symbols(handle.info.id.as_str()).unwrap();
    assert!(symbols.len() >= 2, "symbols {}", symbols.len());

    let points = handle.db.list_datapoints(&first.dataset_id).unwrap();
    assert!(points.iter().all(|point| point.provenance.path != ".env"));
    assert!(points.iter().any(|point| point.kind == "commit"));

    let edges = handle.db.list_edges(&first.dataset_id).unwrap();
    assert!(edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::Calls || edge.kind == EdgeKind::Owns));
    assert!(edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::TouchedInCommit));

    let worktree_only_dir = tempfile::tempdir().unwrap();
    git_repo(worktree_only_dir.path());
    fs::create_dir_all(worktree_only_dir.path().join("docs")).unwrap();
    commit_file(
        worktree_only_dir.path(),
        "docs/arch.md",
        "# Docs should stay out of worktree-only add",
    );
    let wdb = IndexDatabase::open(worktree_only_dir.path().join("index.sqlite")).unwrap();
    let mut whandle = RepoHandle::open_with_db(worktree_only_dir.path(), wdb).unwrap();
    let worktree_report = add_pipeline(
        &mut whandle,
        &AddRequest {
            sources: vec![SourceKind::Worktree],
            dataset_name: None,
            text: None,
            full: false,
        },
        &TaskRegistry::new(),
    )
    .unwrap();
    let wpoints = whandle
        .db
        .list_datapoints(&worktree_report.dataset_id)
        .unwrap();
    assert!(wpoints
        .iter()
        .all(|p| !p.provenance.path.ends_with(".md")
            && p.provenance.source_kind == SourceKind::Worktree));

    let caller = symbols
        .iter()
        .find(|symbol| symbol.name == "caller")
        .unwrap();
    let start = symbol_node_id(&first.dataset_id, &caller.id);
    let reached = handle.db.traverse(&first.dataset_id, &start, 2).unwrap();
    assert!(reached
        .iter()
        .any(|(id, depth)| id != &start && *depth >= 1));

    let file = file_node_id(&first.dataset_id, "src/lib.rs");
    let from_file = handle.db.traverse(&first.dataset_id, &file, 2).unwrap();
    assert!(from_file.len() > 1);

    for format in ["json", "dot", "mermaid"] {
        let document =
            export_graph(&handle, None, GraphExportFormat::parse(format).unwrap()).unwrap();
        assert!(document.contains("caller"), "{format}: {document}");
    }

    let pasted = add_pipeline(
        &mut handle,
        &AddRequest {
            sources: vec![SourceKind::PastedText],
            dataset_name: None,
            text: Some("first note".into()),
            full: false,
        },
        &TaskRegistry::new(),
    )
    .unwrap();
    assert_eq!(pasted.datapoints_inserted, 1);
    add_pipeline(
        &mut handle,
        &AddRequest {
            sources: vec![SourceKind::PastedText],
            dataset_name: None,
            text: Some("second note".into()),
            full: false,
        },
        &TaskRegistry::new(),
    )
    .unwrap();
    let edges = handle.db.list_edges(&first.dataset_id).unwrap();
    assert!(edges.iter().any(|edge| edge.kind == EdgeKind::Supersedes));
}

#[test]
fn ontology_can_reject_call_edges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_repo(root);
    fs::create_dir_all(root.join(".02agent")).unwrap();
    fs::write(
        root.join(".02agent/ontology.toml"),
        "edge_kinds = [\"owns\"]\n",
    )
    .unwrap();
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    add_pipeline(
        &mut handle,
        &AddRequest {
            sources: vec![SourceKind::Worktree],
            dataset_name: None,
            text: None,
            full: true,
        },
        &TaskRegistry::new(),
    )
    .unwrap();
    let report = cognify_pipeline(&mut handle, true, None, &TaskRegistry::new()).unwrap();
    let edges = handle.db.list_edges(&report.dataset_id).unwrap();
    assert!(edges.iter().all(|edge| edge.kind != EdgeKind::Calls));
    assert!(edges.iter().any(|edge| edge.kind == EdgeKind::Owns));
}

#[test]
fn traversal_refuses_excessive_depth() {
    let dir = tempfile::tempdir().unwrap();
    git_repo(dir.path());
    let db = IndexDatabase::open(dir.path().join("index.sqlite")).unwrap();
    let handle = RepoHandle::open_with_db(dir.path(), db).unwrap();
    let error = handle
        .db
        .traverse("dataset:missing:default", "start", 9)
        .unwrap_err();
    assert!(error.to_string().contains("depth"));
}

fn commit_file(root: &std::path::Path, relative: &str, body: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, body).unwrap();
    let repo = Repository::open(root).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new(relative)).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, relative, &tree, &[&parent])
        .unwrap();
}

#[test]
fn index_pipeline_resolves_calls_imports_and_purges_removed_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_repo(root);
    commit_file(
        root,
        "src/other.rs",
        "pub fn shared() {}\npub struct SharedName;\n",
    );
    commit_file(
        root,
        "src/third.rs",
        "pub fn fanout() { shared(); }\nuse crate::b::SharedName;\nuse crate::missing::Missing;\n",
    );
    fs::write(
        root.join("src/lib.rs"),
        "pub struct Widget;\npub fn unique_target() {}\npub fn shared() {}\npub fn caller() { unique_target(); shared(); }\nuse crate::models::Widget;\n",
    )
    .unwrap();
    commit_file(root, "src/also.rs", "pub struct SharedName;\n");
    commit_file(root, "src/gone.rs", "pub fn gone() { unique_target(); }\n");

    static HITS: AtomicUsize = AtomicUsize::new(0);
    let mut extra = TaskRegistry::new();
    extra.register(CountingTask { hits: &HITS });
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    let added = index_pipeline(&mut handle, true, &extra).unwrap();
    assert_eq!(HITS.load(Ordering::SeqCst), 1);

    let symbols = handle.db.list_symbols(handle.info.id.as_str()).unwrap();
    let edges = handle.db.list_edges(&added.dataset_id).unwrap();
    let id_of = |name: &str, file: &str| {
        let symbol = symbols
            .iter()
            .find(|item| item.name == name && item.file_path == file)
            .unwrap_or_else(|| panic!("missing {name} in {file}"));
        symbol_node_id(&added.dataset_id, &symbol.id)
    };
    let caller = id_of("caller", "src/lib.rs");
    let caller_dsts: Vec<_> = edges
        .iter()
        .filter(|edge| edge.src_id == caller && edge.kind == EdgeKind::Calls)
        .map(|edge| edge.dst_id.clone())
        .collect();
    assert!(caller_dsts.contains(&id_of("unique_target", "src/lib.rs")));
    assert!(caller_dsts.contains(&id_of("shared", "src/lib.rs")));
    assert!(!caller_dsts.contains(&id_of("shared", "src/other.rs")));

    let fanout = id_of("fanout", "src/third.rs");
    let fan_dsts: Vec<_> = edges
        .iter()
        .filter(|edge| edge.src_id == fanout && edge.kind == EdgeKind::Calls)
        .map(|edge| edge.dst_id.clone())
        .collect();
    assert!(fan_dsts.contains(&id_of("shared", "src/lib.rs")));
    assert!(fan_dsts.contains(&id_of("shared", "src/other.rs")));

    let lib = file_node_id(&added.dataset_id, "src/lib.rs");
    let widget = edges
        .iter()
        .find(|edge| {
            edge.kind == EdgeKind::Imports
                && edge.src_id == lib
                && edge.payload.as_deref() == Some("crate::models::Widget")
        })
        .expect("unique import");
    assert_eq!(widget.dst_id, id_of("Widget", "src/lib.rs"));

    let third = file_node_id(&added.dataset_id, "src/third.rs");
    let ambiguous = edges
        .iter()
        .find(|edge| {
            edge.kind == EdgeKind::Imports
                && edge.src_id == third
                && edge.payload.as_deref() == Some("crate::b::SharedName")
        })
        .expect("ambiguous import");
    assert!(ambiguous.dst_id.starts_with("name:"));
    let missing = edges
        .iter()
        .find(|edge| {
            edge.kind == EdgeKind::Imports
                && edge.src_id == third
                && edge.payload.as_deref() == Some("crate::missing::Missing")
        })
        .expect("missing import");
    assert!(missing.dst_id.starts_with("name:"));

    let points = handle.db.list_datapoints(&added.dataset_id).unwrap();
    let chunks: Vec<_> = points
        .iter()
        .filter(|point| point.kind == "chunk")
        .collect();
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(|point| point.content.is_empty()));
    assert!(points
        .iter()
        .any(|point| point.provenance.path.starts_with("src/gone.rs")));

    fs::remove_file(root.join("src/gone.rs")).unwrap();
    index_pipeline(&mut handle, false, &TaskRegistry::new()).unwrap();
    let after = handle.db.list_datapoints(&added.dataset_id).unwrap();
    assert!(after
        .iter()
        .all(|point| !point.provenance.path.starts_with("src/gone.rs")));
}

#[test]
fn pasted_text_respects_size_limit_and_redacts_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_repo(root);
    fs::create_dir_all(root.join(".02agent")).unwrap();
    fs::write(root.join(".02agent/config.toml"), "max_file_size_kb = 1\n").unwrap();
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    let too_big = add_pipeline(
        &mut handle,
        &AddRequest {
            sources: vec![SourceKind::PastedText],
            dataset_name: None,
            text: Some("x".repeat(2048)),
            full: false,
        },
        &TaskRegistry::new(),
    );
    assert!(too_big.is_err());

    let token = format!("ghp_{}", "b".repeat(36));
    let added = add_pipeline(
        &mut handle,
        &AddRequest {
            sources: vec![SourceKind::PastedText],
            dataset_name: None,
            text: Some(format!("token {token}")),
            full: false,
        },
        &TaskRegistry::new(),
    )
    .unwrap();
    let points = handle.db.list_datapoints(&added.dataset_id).unwrap();
    let note = points
        .iter()
        .find(|point| point.kind == "pasted_text")
        .unwrap();
    assert!(note.content.contains("[REDACTED]"));
    assert!(!note.content.contains(&token));
}

fn head_commit(root: &std::path::Path) -> String {
    Repository::open(root)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap()
        .to_string()
}

fn commit_message(root: &std::path::Path, message: &str, paths: &[&str], remove: &[&str]) {
    let repo = Repository::open(root).unwrap();
    let mut index = repo.index().unwrap();
    for path in remove {
        index.remove_path(std::path::Path::new(path)).unwrap();
    }
    for path in paths {
        index.add_path(std::path::Path::new(path)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
        .unwrap();
}

#[test]
fn cognify_drops_deleted_and_moved_symbol_nodes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_repo(root);
    commit_file(
        root,
        "retained.rs",
        "fn obsolete_definition() {}\nfn retained_definition() {}\n",
    );
    commit_file(root, "deleted_file.rs", "fn deleted_file_symbol() {}\n");

    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    let added = index_pipeline(&mut handle, true, &TaskRegistry::new()).unwrap();
    cognify_pipeline(&mut handle, true, None, &TaskRegistry::new()).unwrap();
    let before = handle.db.list_nodes(&added.dataset_id).unwrap();
    assert!(before
        .iter()
        .any(|node| node.kind == "symbol" && node.label == "obsolete_definition"));
    assert!(before
        .iter()
        .any(|node| node.kind == "symbol" && node.label == "deleted_file_symbol"));
    assert_eq!(
        before
            .iter()
            .filter(|node| node.kind == "symbol" && node.label == "retained_definition")
            .count(),
        1
    );

    fs::remove_file(root.join("deleted_file.rs")).unwrap();
    fs::write(root.join("retained.rs"), "fn retained_definition() {}\n").unwrap();
    commit_message(
        root,
        "move retained and drop deleted",
        &["retained.rs"],
        &["deleted_file.rs"],
    );
    index_pipeline(&mut handle, true, &TaskRegistry::new()).unwrap();
    cognify_pipeline(&mut handle, true, None, &TaskRegistry::new()).unwrap();

    let symbols = handle.db.list_symbols(handle.info.id.as_str()).unwrap();
    assert!(
        symbols
            .iter()
            .all(|symbol| symbol.name != "obsolete_definition"
                && symbol.name != "deleted_file_symbol")
    );
    assert_eq!(
        symbols
            .iter()
            .filter(|symbol| symbol.name == "retained_definition")
            .count(),
        1
    );
    let nodes = handle.db.list_nodes(&added.dataset_id).unwrap();
    assert!(!nodes
        .iter()
        .any(|node| node.kind == "symbol" && node.label == "obsolete_definition"));
    assert!(!nodes
        .iter()
        .any(|node| node.kind == "symbol" && node.label == "deleted_file_symbol"));
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.kind == "symbol" && node.label == "retained_definition")
            .count(),
        1
    );
    let points = handle.db.list_datapoints(&added.dataset_id).unwrap();
    assert!(points
        .iter()
        .all(|point| !point.provenance.path.contains("deleted_file.rs")));
    let exported = export_graph(&handle, None, GraphExportFormat::Json).unwrap();
    assert!(!exported.contains("obsolete_definition"));
    assert!(!exported.contains("deleted_file_symbol"));
    assert!(exported.contains("retained_definition"));
}

#[test]
fn read_only_open_does_not_advance_index_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_repo(root);
    let first = head_commit(root);
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    index_pipeline(&mut handle, true, &TaskRegistry::new()).unwrap();
    let indexed = handle.db.get_stats(&handle.info.id).unwrap();
    assert_eq!(indexed.last_indexed_commit.as_deref(), Some(first.as_str()));
    let indexed_at = indexed.last_indexed_at.expect("indexed_at");

    commit_file(root, "src/next.rs", "pub fn next_definition() {}\n");
    let second = head_commit(root);
    assert_ne!(second, first);
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let handle = RepoHandle::open_with_db(root, db).unwrap();
    let exported = export_graph(&handle, None, GraphExportFormat::Json).unwrap();
    assert!(exported.contains("target") || exported.contains("caller") || !exported.is_empty());
    let after_open = handle.db.get_stats(&handle.info.id).unwrap();
    assert_eq!(
        after_open.last_indexed_commit.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(after_open.last_indexed_at, Some(indexed_at));

    std::thread::sleep(std::time::Duration::from_millis(5));
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    index_pipeline(&mut handle, true, &TaskRegistry::new()).unwrap();
    let after_index = handle.db.get_stats(&handle.info.id).unwrap();
    assert_eq!(
        after_index.last_indexed_commit.as_deref(),
        Some(second.as_str())
    );
    assert_ne!(after_index.last_indexed_at, Some(indexed_at));
}

#[test]
fn git_history_redacts_synthetic_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_repo(root);
    let token = format!("ghp_{}", "x".repeat(36));
    commit_message(root, &format!("rotate {token} now"), &["src/lib.rs"], &[]);

    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    let added = add_pipeline(
        &mut handle,
        &AddRequest {
            sources: vec![SourceKind::GitHistory],
            dataset_name: None,
            text: None,
            full: false,
        },
        &TaskRegistry::new(),
    )
    .unwrap();
    cognify_pipeline(&mut handle, true, None, &TaskRegistry::new()).unwrap();

    let points = handle.db.list_datapoints(&added.dataset_id).unwrap();
    let commits: Vec<_> = points
        .iter()
        .filter(|point| point.kind == "commit")
        .collect();
    assert!(!commits.is_empty());
    assert!(commits.iter().all(|point| !point.content.contains(&token)));
    assert!(commits
        .iter()
        .any(|point| point.content.contains("[REDACTED]")));
    let exported = export_graph(&handle, None, GraphExportFormat::Json).unwrap();
    assert!(!exported.contains(&token));
    assert!(exported.contains("[REDACTED]"));
}

#[test]
fn scoped_callers_keep_their_identity_through_catalog_and_graph_projection() {
    for (file, code) in [
        ("methods.rs", "fn only_a() {} fn only_b() {} struct A; struct B; impl A { fn run(&self) { only_a(); } } impl B { fn run(&self) { only_b(); } }"),
        ("methods.js", "function only_a() {} function only_b() {} class A { run() { only_a(); } } class B { run() { only_b(); } }"),
        ("methods.ts", "function only_a() {} function only_b() {} class A { run() { only_a(); } } class B { run() { only_b(); } }"),
        ("methods.cpp", "void only_a() {} void only_b() {} struct A { void run() { only_a(); } void run(int n) { only_b(); } };"),
        ("nested.py", "def only_a(): pass\ndef only_b(): pass\ndef a():\n    def run():\n        only_a()\ndef b():\n    def run():\n        only_b()\n"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_repo(root);
        commit_file(root, file, code);
        let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
        let mut handle = RepoHandle::open_with_db(root, db).unwrap();
        let report = index_pipeline(&mut handle, true, &TaskRegistry::new()).unwrap();
        let check = |handle: &RepoHandle| {
            let refs = handle.db.list_references(handle.info.id.as_str()).unwrap();
            let calls: Vec<_> = refs.iter().filter(|reference| reference.source_file == file
                && matches!(reference.target_name.as_str(), "only_a" | "only_b")).collect();
            assert_eq!(calls.len(), 2, "{file}: {calls:?}");
            assert!(calls.iter().all(|reference| reference.source_symbol_name.as_deref() == Some("run")));
            assert_ne!(calls[0].source_symbol_id, calls[1].source_symbol_id, "{file}");
            let symbols = handle.db.list_symbols(handle.info.id.as_str()).unwrap();
            let edges = handle.db.list_edges(&report.dataset_id).unwrap();
            for reference in calls {
                let source = symbol_node_id(&report.dataset_id, reference.source_symbol_id.as_deref().unwrap());
                let target = symbols.iter().find(|symbol| symbol.file_path == file
                    && symbol.name == reference.target_name).unwrap();
                let destination = symbol_node_id(&report.dataset_id, &target.id);
                let outgoing: Vec<_> = edges.iter().filter(|edge| edge.kind == EdgeKind::Calls
                    && edge.src_id == source).collect();
                assert_eq!(outgoing.len(), 1, "{file}: {outgoing:?}");
                assert_eq!(outgoing[0].dst_id, destination, "{file}");
            }
            let dependencies = handle.db.find_symbol_deps(&handle.info.id, "run").unwrap();
            assert!(dependencies.callees.iter().all(|reference| reference.source_symbol_id.is_some()));
        };
        check(&handle);
        // Simulate migrated v1/v2 rows with no source identity and an unchanged file hash.
        let symbols = handle.db.list_symbols(handle.info.id.as_str()).unwrap();
        let mut references = handle.db.list_references(handle.info.id.as_str()).unwrap();
        for reference in &mut references { reference.source_symbol_id = None; }
        handle.db.save_symbols_and_references(&handle.info.id, &symbols, &references).unwrap();
        assert!(handle.db.files_missing_source_identity(&handle.info.id).unwrap().iter().any(|path| path == file));
        cognify_pipeline(&mut handle, false, None, &TaskRegistry::new()).unwrap();
        check(&handle);
        assert!(handle.db.files_missing_source_identity(&handle.info.id).unwrap().is_empty());
    }
}

#[test]
fn excluded_symbol_nodes_do_not_leave_dangling_caller_edges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_repo(root);
    fs::create_dir_all(root.join(".02agent")).unwrap();
    fs::write(
        root.join(".02agent/ontology.toml"),
        "node_kinds = [\"file\", \"name\", \"chunk\"]\nedge_kinds = [\"calls\", \"owns\"]\n",
    )
    .unwrap();
    let db = IndexDatabase::open(root.join("index.sqlite")).unwrap();
    let mut handle = RepoHandle::open_with_db(root, db).unwrap();
    let report = index_pipeline(&mut handle, true, &TaskRegistry::new()).unwrap();
    let nodes = handle.db.list_nodes(&report.dataset_id).unwrap();
    assert!(nodes.iter().all(|node| node.kind != "symbol"));
    let edges = handle.db.list_edges(&report.dataset_id).unwrap();
    assert!(edges.iter().any(|edge| edge.kind == EdgeKind::Calls));
    assert!(edges
        .iter()
        .all(|edge| nodes.iter().any(|node| node.id == edge.src_id)
            && nodes.iter().any(|node| node.id == edge.dst_id)));
}
