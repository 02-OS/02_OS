pub mod approval;
pub mod datapoint;
pub mod db;
#[cfg(feature = "embeddings")]
pub mod embed;
pub mod evidence;
pub mod gc;
pub mod graph;
pub mod scanner;
pub mod schema;
pub mod search;

pub use approval::{admit_approval, ApprovalRequest, StoredApproval};
pub use datapoint::{InsertedPoint, NewDataPoint, PutPoint};
pub use db::{IndexDatabase, IndexStats, SymbolDepsResult};
pub use evidence::{admit_evidence, digest_package, EvidenceIngest};
pub use gc::{gc_repos, GcReport};
pub use graph::{
    chunk_owner_path, commit_node_id, datapoint_file_path, file_node_id, name_node_id,
    session_node_id, symbol_node_id, GraphExportFormat, GraphNode,
};
pub use scanner::{RepoScanner, ScannedFile};
pub use search::{FtsSearcher, SearchResult};

#[cfg(test)]
mod migrate_tests;
