use crate::protocol::ToolCallResult;
use agent_core::SourceKind;
use agent_index::GraphExportFormat;
use agent_memory::{
    add_pipeline, cognify_pipeline, export_graph, AddRequest, RepoHandle, TaskRegistry,
};
use serde_json::{json, Value};
use std::path::Path;

pub fn call(repo_root: &Path, name: &str, arguments: &Value) -> ToolCallResult {
    let mut handle = match RepoHandle::open(repo_root) {
        Ok(handle) => handle,
        Err(error) => return ToolCallResult::error(format!("Repository error: {error}")),
    };
    let dataset = arguments
        .get("dataset")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let full = arguments
        .get("full")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    match name {
        "add" => {
            let source = arguments
                .get("source")
                .and_then(|value| value.as_str())
                .unwrap_or("worktree,docs");
            let sources = match SourceKind::parse_list(source) {
                Ok(sources) => sources,
                Err(error) => return ToolCallResult::error(error.to_string()),
            };
            let text = arguments
                .get("text")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            match add_pipeline(
                &mut handle,
                &AddRequest {
                    sources,
                    dataset_name: dataset,
                    text,
                    full,
                },
                &TaskRegistry::new(),
            ) {
                Ok(report) => ToolCallResult::text(
                    json!({
                        "dataset_id": report.dataset_id,
                        "files_updated": report.files_updated,
                        "datapoints_inserted": report.datapoints_inserted,
                        "datapoints_unchanged": report.datapoints_unchanged,
                    })
                    .to_string(),
                ),
                Err(error) => ToolCallResult::error(error.to_string()),
            }
        }
        "cognify" => {
            match cognify_pipeline(&mut handle, full, dataset.as_deref(), &TaskRegistry::new()) {
                Ok(report) => ToolCallResult::text(
                    json!({
                        "dataset_id": report.dataset_id,
                        "nodes": report.nodes,
                        "edges": report.edges,
                        "chunks": report.chunks,
                        "symbols": report.symbols,
                    })
                    .to_string(),
                ),
                Err(error) => ToolCallResult::error(error.to_string()),
            }
        }
        "graph_export" => {
            let format = arguments
                .get("format")
                .and_then(|value| value.as_str())
                .unwrap_or("json");
            let parsed = match GraphExportFormat::parse(format) {
                Ok(format) => format,
                Err(error) => return ToolCallResult::error(error.to_string()),
            };
            match export_graph(&handle, dataset.as_deref(), parsed) {
                Ok(document) => ToolCallResult::text(document),
                Err(error) => ToolCallResult::error(error.to_string()),
            }
        }
        _ => ToolCallResult::error(format!("Unknown graph tool '{name}'")),
    }
}
