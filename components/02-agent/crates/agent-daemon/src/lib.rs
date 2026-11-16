pub mod session;
pub mod socket;
pub mod status;
pub mod watcher;

pub use session::resolve_mcp_repo_path;
pub use socket::DaemonSocket;
pub use status::{
    apply_gnome_decision, approval_from_confirmation, decide_rpc, publish_daemon_status,
    publish_status_snapshot, status_rpc, write_status_file, StatusAgent, StatusApproval,
    StatusEvidence, StatusReport, StatusTokenCount,
};
pub use watcher::RepoWatcher;
