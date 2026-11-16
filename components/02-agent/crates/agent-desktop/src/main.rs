use agent_core::Result;
use agent_desktop::{
    authorize, consent_path, handle_message, kill_path, write_consent, write_kill, SessionTree,
};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::fs;
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
    /// Write the kill switch file to immediately stop all desktop automation.
    Kill,
    /// Remove consent (and leave any kill switch untouched) so desktop automation stays off.
    Revoke,
    /// Inspect desktop consent and kill-switch status.
    Status,
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
        Command::Kill => {
            let path = kill_path()?;
            write_kill(&path)?;
            println!("Wrote kill switch {}", path.display());
            Ok(())
        }
        Command::Revoke => {
            let path = consent_path()?;
            match fs::remove_file(&path) {
                Ok(()) => println!("Removed {}", path.display()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    println!("No consent file at {}", path.display());
                }
                Err(err) => return Err(err.into()),
            }
            Ok(())
        }
        Command::Status => {
            let consent = consent_path()?;
            let kill = kill_path()?;
            let authorized = authorize(&consent, &kill);
            println!(
                "{}",
                json!({
                    "consent_path": consent.display().to_string(),
                    "kill_path": kill.display().to_string(),
                    "consented": consent.symlink_metadata().is_ok(),
                    "killed": kill.symlink_metadata().is_ok(),
                    "active": authorized.is_ok(),
                    "reason": authorized.err().map(|e| e.to_string()),
                })
            );
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
            match serde_json::from_str::<Value>(trimmed) {
                Ok(message) => {
                    if let Some(response) = handle_message(&consent, &kill, &mut tree, &message) {
                        writeln!(stdout, "{response}")?;
                        stdout.flush()?;
                    }
                }
                Err(_) => {
                    let response = json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": { "code": -32700, "message": "Parse error" }
                    });
                    writeln!(stdout, "{response}")?;
                    stdout.flush()?;
                }
            }
        }
        line.clear();
    }
    Ok(())
}
