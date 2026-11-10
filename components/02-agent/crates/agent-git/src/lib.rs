pub mod discovery;
pub mod repo;
pub mod workspace;

pub use discovery::GitDiscovery;
pub use repo::{DiffHunk, FileDiffHunks, GitCommitInfo, GitRepo};
pub use workspace::{discover_workspace, WorkspaceMember};
