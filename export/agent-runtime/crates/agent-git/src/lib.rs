pub mod discovery;
pub mod repo;

pub use discovery::GitDiscovery;
pub use repo::{DiffHunk, FileDiffHunks, GitCommitInfo, GitRepo};
