use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Shows the native macOS folder picker. Returns `None` when cancelled.
/// Blocks until the dialog closes.
pub fn choose_folder(start: Option<&Path>) -> Result<Option<PathBuf>> {
    let mut choose = String::from("set f to choose folder with prompt \"Add a folder to Anduin\"");
    if let Some(dir) = start {
        let dir = dir.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
        choose.push_str(&format!(" default location (POSIX file \"{dir}\")"));
    }
    let out = Command::new("osascript")
        .args(["-e", "activate", "-e", &choose, "-e", "POSIX path of f"])
        .output()?;
    refocus_host();

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        // -128 is AppleScript's "User canceled".
        if err.contains("-128") {
            return Ok(None);
        }
        bail!("folder picker failed: {}", err.trim());
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(Some(PathBuf::from(path)))
}

/// The dialog steals focus; hand it back to the terminal running anduin.
fn refocus_host() {
    let app = match std::env::var("TERM_PROGRAM").as_deref() {
        // Anduin.app's own window.
        Ok("Anduin") => "Anduin",
        Ok("Apple_Terminal") => "Terminal",
        Ok("iTerm.app") => "iTerm",
        Ok("ghostty") => "Ghostty",
        Ok("WezTerm") => "WezTerm",
        _ => return,
    };
    let _ = Command::new("open").args(["-a", app]).status();
}
