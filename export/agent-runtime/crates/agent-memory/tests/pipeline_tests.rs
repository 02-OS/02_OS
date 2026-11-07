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
