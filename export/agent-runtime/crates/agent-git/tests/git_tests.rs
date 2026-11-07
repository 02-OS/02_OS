use agent_git::{GitDiscovery, GitRepo};
use git2::Repository;
use tempfile::tempdir;

#[test]
fn test_git_discovery_and_repo_info() {
    let dir = tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    // Configure test signature
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();

    // Create a file and commit
    let test_file = dir.path().join("main.rs");
    std::fs::write(&test_file, "fn main() { println!(\"02_OS\"); }").unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("main.rs")).unwrap();
    index.write().unwrap();

    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let commit_id = repo
        .commit(Some("HEAD"), &sig, &sig, "Initial test commit", &tree, &[])
        .unwrap();

    // Discover repository
    let discovered_root =
        GitDiscovery::find_repository_root(dir.path().join("subdir_that_does_not_exist_yet"))
            .unwrap_or_else(|_| dir.path().to_path_buf());
    assert_eq!(
        discovered_root.canonicalize().unwrap(),
        dir.path().canonicalize().unwrap()
    );

    // Open GitRepo wrapper
    let git_repo = GitRepo::open(dir.path()).unwrap();
    let info = git_repo.info().unwrap();
    assert_eq!(info.head_commit, Some(commit_id.to_string()));
    assert!(info.is_clean);

    let tracked = git_repo.list_tracked_files().unwrap();
    assert_eq!(tracked, vec!["main.rs"]);

    let commits = git_repo.get_recent_commits(5).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].summary, "Initial test commit");
}

#[test]
fn test_status_includes_deleted_and_staged_deletion() {
    let dir = tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();

    let file = dir.path().join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("main.rs")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    std::fs::remove_file(&file).unwrap();
    let git_repo = GitRepo::open(dir.path()).unwrap();
    let (clean, modified, _) = git_repo.status_summary().unwrap();
    assert!(!clean, "an unstaged deletion must not look clean");
    assert!(modified >= 1);
    let diffs = git_repo.get_diff_files().unwrap();
    assert!(
        diffs.iter().any(|p| p == "main.rs"),
        "working-tree deletion missing from diff files: {diffs:?}"
    );

    std::fs::write(&file, "fn main() {}\n").unwrap();
    let (clean, _, _) = git_repo.status_summary().unwrap();
    assert!(clean);

    let mut index = repo.index().unwrap();
    index.remove_path(std::path::Path::new("main.rs")).unwrap();
    index.write().unwrap();
    std::fs::remove_file(&file).unwrap();

    let (clean, modified, _) = git_repo.status_summary().unwrap();
    assert!(!clean, "a staged deletion must not look clean");
    assert!(modified >= 1);
    let diffs = git_repo.get_diff_files().unwrap();
    assert!(diffs.iter().any(|p| p == "main.rs"));
}

#[test]
fn test_status_includes_rename_and_conflict() {
    let dir = tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();

    let file = dir.path().join("main.rs");
    std::fs::write(&file, "fn main() { println!(\"base\"); }\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("main.rs")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let base = repo
        .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    let renamed = dir.path().join("lib.rs");
    std::fs::rename(&file, &renamed).unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(std::path::Path::new("main.rs")).unwrap();
    index.add_path(std::path::Path::new("lib.rs")).unwrap();
    index.write().unwrap();

    let git_repo = GitRepo::open(dir.path()).unwrap();
    let (clean, modified, _) = git_repo.status_summary().unwrap();
    assert!(!clean, "a rename must not look clean");
    assert!(modified >= 1);
    let diffs = git_repo.get_diff_files().unwrap();
    assert!(
        diffs.iter().any(|p| p == "lib.rs" || p == "main.rs"),
        "rename missing from diff files: {diffs:?}"
    );

    // Return to a clean tree, then build a content conflict.
    std::fs::remove_file(&renamed).unwrap();
    std::fs::write(&file, "fn main() { println!(\"base\"); }\n").unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(std::path::Path::new("lib.rs")).ok();
    index.add_path(std::path::Path::new("main.rs")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let parent = repo.find_commit(base).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "restore", &tree, &[&parent])
        .unwrap();

    let head = repo.head().unwrap();
    let branch_name = head.shorthand().unwrap().to_string();
    let base_commit = repo
        .find_commit(repo.head().unwrap().target().unwrap())
        .unwrap();
    repo.branch("other", &base_commit, false).unwrap();

    std::fs::write(&file, "fn main() { println!(\"ours\"); }\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("main.rs")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let ours_parent = repo
        .find_commit(repo.head().unwrap().target().unwrap())
        .unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "ours", &tree, &[&ours_parent])
        .unwrap();

    repo.set_head("refs/heads/other").unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    std::fs::write(&file, "fn main() { println!(\"theirs\"); }\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("main.rs")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let their_parent = repo
        .find_commit(repo.head().unwrap().target().unwrap())
        .unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "theirs", &tree, &[&their_parent])
        .unwrap();

    repo.set_head(&format!("refs/heads/{branch_name}")).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();

    let other = repo.find_branch("other", git2::BranchType::Local).unwrap();
    let annotated = repo
        .find_annotated_commit(other.get().target().unwrap())
        .unwrap();
    repo.merge(&[&annotated], None, None).unwrap();

    let git_repo = GitRepo::open(dir.path()).unwrap();
    let (clean, modified, _) = git_repo.status_summary().unwrap();
    assert!(!clean, "a merge conflict must not look clean");
    assert!(modified >= 1);
    let diffs = git_repo.get_diff_files().unwrap();
    assert!(
        diffs.iter().any(|p| p == "main.rs"),
        "conflict missing from diff files: {diffs:?}"
    );
}

#[test]
fn untracked_directory_child_edits_change_the_fingerprint() {
    let dir = tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let sig = git2::Signature::now("Tester", "test@02os.org").unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("main.rs")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    let child = dir.path().join("notes/draft.txt");
    std::fs::write(&child, "one").unwrap();
    let git_repo = GitRepo::open(dir.path()).unwrap();
    let first = git_repo.status_fingerprint().unwrap();
    std::fs::write(&child, "two — longer").unwrap();
    let second = git_repo.status_fingerprint().unwrap();
    assert_ne!(first, second);
    assert!(
        first.contains("notes/draft.txt"),
        "fingerprint did not recurse into the untracked directory: {first}"
    );
}
