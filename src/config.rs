use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
}

/// Replacement for Claude's mascot: a preset name ("cat", "robot", "ghost",
/// "frog", "none"), an image path, or pixel art.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum MascotSpec {
    Name(String),
    /// 6 rows of 18 chars; each char is a key into `colors` ("#rrggbb" or
    /// "accent"), `.` is transparent.
    Art {
        pixels: Vec<String>,
        colors: HashMap<String, String>,
    },
    /// Like `Art`, but several frames shown in a loop, plus optional frames
    /// played once when the mascot is clicked.
    Animated {
        frames: Vec<MascotFrame>,
        colors: HashMap<String, String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        click: Vec<MascotFrame>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MascotFrame {
    pub pixels: Vec<String>,
    /// How long the frame stays on screen.
    #[serde(default = "default_frame_ms")]
    pub ms: u64,
}

fn default_frame_ms() -> u64 {
    500
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(default)]
    pub projects: Vec<Project>,
    /// Command run in each project, e.g. "claude --continue".
    #[serde(default = "default_command")]
    pub command: String,
    /// Prefix key, written as "ctrl-<char>", e.g. "ctrl-]" or "ctrl-a".
    #[serde(default = "default_prefix")]
    pub prefix: String,
    /// Color Claude's orange is turned into: "blue", "purple", "teal", a hue
    /// in degrees, or "none" for the original look.
    #[serde(default = "default_tint")]
    pub tint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mascot: Option<MascotSpec>,
    /// Folder opened (and added if needed) when anduin starts without arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Extra programs offered when starting a session, besides the built-in list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub command: String,
}

fn default_command() -> String {
    "claude".into()
}

fn default_prefix() -> String {
    "ctrl-]".into()
}

fn default_tint() -> String {
    "blue".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            command: default_command(),
            prefix: default_prefix(),
            tint: default_tint(),
            mascot: None,
            default: None,
            tools: Vec::new(),
        }
    }
}

/// `$ANDUIN_CONFIG`, or `<config dir>/anduin/config.json`.
pub fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ANDUIN_CONFIG") {
        return Some(PathBuf::from(path));
    }
    dirs::config_dir().map(|d| d.join("anduin").join("config.json"))
}

impl Config {
    pub fn load() -> Self {
        config_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path().context("no config directory")?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Adds a directory and returns its index. Existing entries are reused.
    pub fn add(&mut self, path: &Path) -> Result<usize> {
        let path = path
            .canonicalize()
            .with_context(|| format!("{} not found", path.display()))?;
        if !path.is_dir() {
            anyhow::bail!("{} is not a directory", path.display());
        }
        if let Some(i) = self.projects.iter().position(|p| p.path == path) {
            return Ok(i);
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.projects.push(Project { name, path });
        Ok(self.projects.len() - 1)
    }
}

/// Expands a leading `~` to the home directory.
pub fn expand_tilde(input: &str) -> PathBuf {
    if input == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(input)
}

/// Replaces the home directory prefix with `~` for display.
pub fn shorten(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

/// Tab-completes a directory path typed by the user.
pub fn complete_dir(input: &str) -> String {
    let (head, tail) = match input.rfind('/') {
        Some(i) => input.split_at(i + 1),
        None => ("", input),
    };
    let dir = if head.is_empty() {
        PathBuf::from(".")
    } else {
        expand_tilde(head)
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return input.to_string();
    };
    let mut matches: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(tail) && (tail.starts_with('.') || !n.starts_with('.')))
        .collect();
    matches.sort();
    let Some(first) = matches.first() else {
        return input.to_string();
    };
    let mut common = first.clone();
    for m in &matches[1..] {
        let len = common
            .chars()
            .zip(m.chars())
            .take_while(|(a, b)| a == b)
            .count();
        common = common.chars().take(len).collect();
    }
    let mut out = format!("{head}{common}");
    if matches.len() == 1 {
        out.push('/');
    }
    out
}
