use agent_core::Result;
use agent_desktop::{consent_path, handle_message, kill_path, write_consent, SessionTree};
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};

#[derive(Parser, Debug)]
#[command(
    name = "02-desktop",
    about = "Local desktop automation. Off until a private consent file exists."
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Write the private consent file. This does not start a session.
    Consent,
    /// Serve MCP JSON-RPC on stdin. Every tool call checks consent and the kill file.
    Serve,
}

fn main() -> Result<()> {
    match Args::parse().command {
        Command::Consent => {
            let path = consent_path()?;
            write_consent(&path)?;
            println!("Wrote {}", path.display());
            Ok(())
        }
        Command::Serve => serve(),
    }
}

fn serve() -> Result<()> {
    let consent = consent_path()?;
    let kill = kill_path()?;
    let mut tree = SessionTree;
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout();
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            if let Ok(message) = serde_json::from_str::<Value>(trimmed) {
                if let Some(response) = handle_message(&consent, &kill, &mut tree, &message) {
                    writeln!(stdout, "{response}")?;
                    stdout.flush()?;
                }
            }
        }
        line.clear();
    }
    Ok(())
}
