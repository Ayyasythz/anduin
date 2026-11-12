use crate::config::ToolSpec;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Claude,
    Shell,
    Other,
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub kind: Kind,
    pub name: String,
    pub command: String,
    pub available: bool,
}

impl Tool {
    /// Who gets stopped when the session is closed.
    pub fn noun(&self) -> &str {
        match self.kind {
            Kind::Claude => "Claude",
            Kind::Shell => "The shell",
            Kind::Other => &self.name,
        }
    }

    /// Short form of the command for display, e.g. "zsh" for "/bin/zsh -l".
    pub fn program(&self) -> String {
        let prog = self.command.split_whitespace().next().unwrap_or_default();
        Path::new(prog).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

const KNOWN: &[(&str, &str)] = &[
    ("Codex", "codex"),
    ("Gemini CLI", "gemini"),
    ("OpenCode", "opencode"),
    ("Cursor Agent", "cursor-agent"),
    ("Copilot CLI", "copilot"),
    ("Amp", "amp"),
    ("Aider", "aider"),
    ("Goose", "goose"),
    ("Qwen Code", "qwen"),
    ("Crush", "crush"),
];

/// Claude, a shell, the extra tools from the config, then the known AI CLIs,
/// each marked with whether it is installed.
pub fn detect(claude_command: &str, extra: &[ToolSpec]) -> Vec<Tool> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let tool = |kind, name: &str, command: String| Tool {
        kind,
        name: name.to_string(),
        available: installed(&command, &path),
        command,
    };
    let mut tools = vec![
        tool(Kind::Claude, "Claude Code", claude_command.to_string()),
        tool(Kind::Shell, "Terminal", format!("{} -l", shell())),
    ];
    for spec in extra {
        tools.push(tool(Kind::Other, &spec.name, spec.command.clone()));
    }
    for (name, bin) in KNOWN {
        if !tools.iter().any(|t| t.program() == *bin) {
            tools.push(tool(Kind::Other, name, bin.to_string()));
        }
    }
    tools
}

fn shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| is_executable(Path::new(s)))
        .unwrap_or_else(|| "/bin/zsh".into())
}

fn installed(command: &str, path: &std::ffi::OsStr) -> bool {
    let Some(prog) = command.split_whitespace().next() else { return false };
    if prog.contains('/') {
        return is_executable(Path::new(prog));
    }
    std::env::split_paths(path).any(|dir| is_executable(&dir.join(prog)))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn finds_executables_on_path() {
        let dir = std::env::temp_dir().join(format!("anduin-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("fake-ai");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plain = dir.join("not-exec");
        std::fs::write(&plain, "").unwrap();

        let path = std::env::join_paths([&dir]).unwrap();
        assert!(installed("fake-ai --flag", &path));
        assert!(!installed("not-exec", &path));
        assert!(!installed("missing-ai", &path));
        assert!(installed(exe.to_str().unwrap(), &path));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lists_claude_and_shell_first_without_duplicates() {
        let extra = [ToolSpec { name: "My Codex".into(), command: "codex --full-auto".into() }];
        let tools = detect("claude --continue", &extra);
        assert_eq!(tools[0].kind, Kind::Claude);
        assert_eq!(tools[0].command, "claude --continue");
        assert_eq!(tools[1].kind, Kind::Shell);
        assert!(tools[1].available);
        assert_eq!(tools[2].name, "My Codex");
        assert_eq!(tools.iter().filter(|t| t.program() == "codex").count(), 1);
    }
}
