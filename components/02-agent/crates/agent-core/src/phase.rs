use crate::{paths::StoragePaths, AgentError, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Phases a Shell session is allowed to show.
pub fn phase_label(raw: &str) -> &'static str {
    match raw.trim() {
        "indexing" => "indexing",
        "complete" => "complete",
        "stale" => "stale",
        _ => "idle",
    }
}

/// Write one allowlisted phase line. The Shell extension reads this file.
pub fn write_index_phase(path: &Path, phase: &str) -> Result<()> {
    let phase = phase_label(phase);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            StoragePaths::ensure_private_dir(parent)?;
        }
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, format!("{phase}\n"))?;
    let mut permissions = fs::metadata(&temporary)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&temporary, permissions)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

/// Publish the phase for the user session at `~/.local/share/02-agent/index-phase`.
pub fn publish_index_phase(phase: &str) -> Result<()> {
    let directory = StoragePaths::data_dir()?;
    StoragePaths::ensure_private_dir(&directory)?;
    write_index_phase(&directory.join("index-phase"), phase)
}

/// Refuse a phase that is not one of the four Shell labels.
pub fn require_phase(phase: &str) -> Result<&'static str> {
    match phase.trim() {
        "idle" => Ok("idle"),
        "indexing" => Ok("indexing"),
        "complete" => Ok("complete"),
        "stale" => Ok("stale"),
        _ => Err(AgentError::General(format!("unknown index phase: {phase}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn phase_file_is_private_and_unknown_text_is_idle() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("index-phase");
        write_index_phase(&path, "complete").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(fs::read_to_string(&path).unwrap(), "complete\n");
        write_index_phase(&path, "not-a-phase").unwrap();
        assert_eq!(phase_label(&fs::read_to_string(&path).unwrap()), "idle");
        assert!(require_phase("applied").is_err());
    }
}
