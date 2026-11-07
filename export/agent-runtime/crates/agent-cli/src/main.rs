use clap::{Parser, Subcommand};
use std::process::ExitCode;

mod commands;

#[derive(Parser, Debug)]
#[command(
    name = "02",
    author = "02_OS Systems Engineering Team",
    version,
    about = "02: Git-aware repository intelligence service for coding agents",
    long_about = "02 Agent Runtime provides deterministic repository intelligence (Git + Tree-sitter + Evidence Graph + Invalidation + Context Compiler + MCP) natively in 02_OS."
)]
struct Cli {
    #[arg(long, global = true, help = "Output machine-readable JSON")]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Initialize 02 Agent index for the current Git repository")]
    Init,

    #[command(about = "Show repository indexing status, Git HEAD, and health")]
    Status,

    #[command(about = "Scan repository files and update the local SQLite FTS5 index")]
    Index {
        #[arg(long, help = "Force a complete re-index of the entire repository")]
        full: bool,
    },

    #[command(
        about = "Ingest sources into a dataset (working tree, docs, history, session, text)"
    )]
    Add {
        #[arg(
            long,
            default_value = "worktree,docs",
            help = "Comma-separated sources: worktree, docs, history, session, text"
        )]
        source: String,

        #[arg(long, help = "Text for session or pasted-text sources")]
        text: Option<String>,

        #[arg(long, help = "Dataset name (default: default)")]
        dataset: Option<String>,

        #[arg(long, help = "Re-read every file instead of the incremental catalog")]
        full: bool,
    },

    #[command(about = "Build a deterministic symbol, chunk, and git graph for a dataset")]
    Cognify {
        #[arg(long, help = "Dataset name (default: default)")]
        dataset: Option<String>,

        #[arg(long, help = "Re-extract every source file")]
        full: bool,
    },

    #[command(about = "Inspect the repository knowledge graph")]
    Graph {
        #[command(subcommand)]
        action: GraphCommands,
    },

    #[command(about = "Search repository code and documentation using BM25 full-text search")]
    Search {
        #[arg(help = "Search query string")]
        query: String,

        #[arg(short, long, default_value_t = 10, help = "Maximum results to return")]
        limit: usize,
    },

    #[command(
        about = "Run diagnostic checks on system privileges, Git, storage, and 02_OS runtime"
    )]
    Doctor,

    #[command(
        about = "Audit repository for exposed secrets, permissions, and ignore configuration"
    )]
    Security,

    #[command(about = "Find definition and location of a code symbol (Phase 2)")]
    Symbol {
        #[arg(help = "Symbol name")]
        name: String,
    },

    #[command(about = "List symbol dependencies and caller/callee relationships (Phase 2)")]
    Deps {
        #[arg(help = "Symbol name")]
        symbol: String,
    },

    #[command(about = "Find test suites covering a given symbol or file (Phase 2)")]
    Tests {
        #[arg(help = "Symbol name or path")]
        symbol: String,
    },

    #[command(about = "Compile a focused evidence package for a given coding task (Phase 5)")]
    Context {
        #[arg(help = "Task description")]
        task: String,

        #[arg(short, long, default_value_t = 8000, help = "Token budget limit")]
        budget: usize,
    },

    #[command(about = "Interact with repository evidence memories (Phase 3)")]
    Memory {
        #[command(subcommand)]
        sub: MemoryCommands,
    },

    #[command(about = "Start standard I/O Model Context Protocol (MCP) server for agents")]
    Mcp,

    #[command(
        about = "Generate or configure MCP connection for coding agents (codex, claude, opencode, cursor, gemini, zed)"
    )]
    Connect {
        #[arg(help = "Target coding agent (codex, claude, opencode, cursor, gemini, zed)")]
        agent: String,

        #[arg(
            long,
            help = "Automatically write configuration to default agent config path"
        )]
        write: bool,
    },

    #[command(about = "Start background 02-agentd daemon service (Phase 7)")]
    Serve,

    #[command(about = "Garbage collect orphaned or stale repository indexes")]
    Gc,
}

#[derive(Subcommand, Debug)]
enum GraphCommands {
    #[command(about = "Export the knowledge graph as JSON, DOT, or Mermaid")]
    Export {
        #[arg(long, default_value = "json", help = "json, dot, or mermaid")]
        format: String,

        #[arg(long, help = "Dataset name (default: default)")]
        dataset: Option<String>,

        #[arg(short, long, help = "Write the document to this path")]
        output: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum MemoryCommands {
    #[command(about = "List all verified memories")]
    List,
    #[command(about = "Inspect a specific memory by ID")]
    Inspect { id: String },
    #[command(about = "Verify freshness of a memory against current Git HEAD")]
    Verify { id: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => commands::init::execute(cli.json),
        Commands::Status => commands::status::execute(cli.json),
        Commands::Index { full } => commands::index::execute(full, cli.json),
        Commands::Add {
            source,
            text,
            dataset,
            full,
        } => commands::add::execute(&source, text, dataset, full, cli.json),
        Commands::Cognify { dataset, full } => commands::cognify::execute(dataset, full, cli.json),
        Commands::Graph { action } => match action {
            GraphCommands::Export {
                format,
                dataset,
                output,
            } => commands::graph::execute_export(&format, dataset, output, cli.json),
        },
        Commands::Search { query, limit } => commands::search::execute(&query, limit, cli.json),
        Commands::Doctor => commands::doctor::execute(cli.json),
        Commands::Security => commands::security::execute(cli.json),
        Commands::Symbol { name } => commands::symbol::execute(&name, cli.json),
        Commands::Deps { symbol } => commands::deps::execute(&symbol, cli.json),
        Commands::Tests { symbol } => commands::tests::execute(&symbol, cli.json),
        Commands::Context { task, budget } => commands::context::execute(&task, budget, cli.json),
        Commands::Memory { sub } => match sub {
            MemoryCommands::List => commands::memory::execute_list(cli.json),
            MemoryCommands::Inspect { id } => commands::memory::execute_inspect(&id, cli.json),
            MemoryCommands::Verify { id } => commands::memory::execute_verify(&id, cli.json),
        },
        Commands::Mcp => commands::mcp::execute(),
        Commands::Connect { agent, write } => commands::connect::execute(&agent, write),
        Commands::Serve => commands::serve::execute(),
        Commands::Gc => commands::gc::execute(cli.json),
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}
