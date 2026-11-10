pub mod session;
pub mod socket;
pub mod status;
pub mod watcher;

pub use session::resolve_mcp_repo_path;
pub use socket::DaemonSocket;
pub use status::{decide_rpc, publish_daemon_status, status_rpc};
pub use watcher::RepoWatcher;
