use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const SCROLLBACK: usize = 10_000;

/// Env vars from the host terminal that would make the child believe it talks
/// to a terminal with features we don't emulate.
const HOST_ENV: &[&str] = &[
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TMUX",
    "TMUX_PANE",
    "ITERM_SESSION_ID",
    "KITTY_WINDOW_ID",
    "WEZTERM_PANE",
    "GHOSTTY_RESOURCES_DIR",
];

/// Side channel of the emulator: bells, titles, and replies to terminal queries.
#[derive(Default)]
pub struct Hooks {
    pub attention: bool,
    pub title: Option<String>,
    replies: Vec<u8>,
}

impl vt100::Callbacks for Hooks {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.attention = true;
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        let title = String::from_utf8_lossy(title).trim().to_string();
        self.title = (!title.is_empty()).then_some(title);
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied();
        match (i1, c, first) {
            // Cursor position report.
            (None, 'n', Some(6)) => {
                let (row, col) = screen.cursor_position();
                let _ = write!(self.replies, "\x1b[{};{}R", row + 1, col + 1);
            }
            // Device status.
            (None, 'n', Some(5)) => self.replies.extend_from_slice(b"\x1b[0n"),
            // Primary / secondary device attributes.
            (None, 'c', _) => self.replies.extend_from_slice(b"\x1b[?62;22c"),
            (Some(b'>'), 'c', _) => self.replies.extend_from_slice(b"\x1b[>0;0;0c"),
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            // Foreground / background color queries: answer with a dark theme.
            [b"10", b"?", ..] => self
                .replies
                .extend_from_slice(b"\x1b]10;rgb:d8d8/d8d8/d8d8\x1b\\"),
            [b"11", b"?", ..] => self
                .replies
                .extend_from_slice(b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\"),
            // Desktop notifications (iTerm2 / rxvt style).
            [b"9", ..] | [b"777", ..] => self.attention = true,
            _ => {}
        }
    }
}

pub struct Session {
    parser: Arc<Mutex<vt100::Parser<Hooks>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    exited: Arc<AtomicBool>,
    last_output: Arc<Mutex<Instant>>,
    pub exit_code: Option<u32>,
}

impl Session {
    pub fn spawn(
        cwd: &Path,
        command: &str,
        rows: u16,
        cols: u16,
        dirty: Arc<AtomicBool>,
    ) -> Result<Self> {
        let argv: Vec<&str> = command.split_whitespace().collect();
        let (prog, args) = argv.split_first().context("empty command")?;

        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(prog);
        cmd.args(args);
        cmd.cwd(cwd);
        for key in HOST_ENV {
            cmd.env_remove(key);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "anduin");

        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("failed to start `{command}`"))?;
        drop(pair.slave);

        let parser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            nz(rows),
            nz(cols),
            SCROLLBACK,
            Hooks::default(),
        )));
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let exited = Arc::new(AtomicBool::new(false));
        let last_output = Arc::new(Mutex::new(Instant::now()));

        let mut reader = pair.master.try_clone_reader()?;
        {
            let parser = parser.clone();
            let writer = writer.clone();
            let exited = exited.clone();
            let last_output = last_output.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let replies = {
                                let mut p = parser.lock().unwrap();
                                p.process(&buf[..n]);
                                std::mem::take(&mut p.callbacks_mut().replies)
                            };
                            if !replies.is_empty() {
                                let mut w = writer.lock().unwrap();
                                let _ = w.write_all(&replies);
                                let _ = w.flush();
                            }
                            *last_output.lock().unwrap() = Instant::now();
                            dirty.store(true, Ordering::Relaxed);
                        }
                    }
                }
                exited.store(true, Ordering::Relaxed);
                dirty.store(true, Ordering::Relaxed);
            });
        }

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            exited,
            last_output,
            exit_code: None,
        })
    }

    pub fn write(&self, bytes: &[u8]) {
        let mut w = self.writer.lock().unwrap();
        let _ = w.write_all(bytes);
        let _ = w.flush();
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.lock().unwrap().set_size(nz(rows), nz(cols));
    }

    pub fn parser(&self) -> std::sync::MutexGuard<'_, vt100::Parser<Hooks>> {
        self.parser.lock().unwrap()
    }

    /// Returns true once the child is gone; records its exit code.
    pub fn poll_exit(&mut self) -> bool {
        if self.exit_code.is_none() {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.exit_code = Some(status.exit_code());
            }
        }
        self.exit_code.is_some() || self.exited.load(Ordering::Relaxed)
    }

    /// True while the child produced output recently (e.g. Claude's spinner).
    pub fn busy(&self) -> bool {
        self.last_output.lock().unwrap().elapsed().as_millis() < 1200
    }

    /// Scrolls the view by `delta` rows (positive = back in history).
    pub fn scroll(&self, delta: isize) {
        let mut p = self.parser();
        let screen = p.screen_mut();
        let cur = screen.scrollback() as isize;
        screen.set_scrollback((cur + delta).max(0) as usize);
    }

    pub fn scroll_to(&self, rows: usize) {
        self.parser().screen_mut().set_scrollback(rows);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n.max(1)).unwrap()
}
