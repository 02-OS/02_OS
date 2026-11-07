use agent_core::{error::AgentError, Result};
use std::path::{Path, PathBuf};

pub struct GitDiscovery;

impl GitDiscovery {
    /// Discover the root directory of a Git repository starting from `start_path` and moving upwards.
    pub fn find_repository_root<P: AsRef<Path>>(start_path: P) -> Result<PathBuf> {
        let mut current = match start_path.as_ref().canonicalize() {
            Ok(c) => c,
            Err(_) => start_path.as_ref().to_path_buf(),
        };

        loop {
            let git_dir = current.join(".git");
            if git_dir.exists() {
                return Ok(current);
            }
            if !current.pop() {
                break;
            }
        }

        Err(AgentError::RepositoryNotFound(format!(
            "No Git repository found at or above '{}'",
            start_path.as_ref().display()
        )))
    }
}
