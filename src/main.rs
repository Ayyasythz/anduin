mod app;
mod config;
mod keys;
mod mascot;
mod picker;
mod session;
mod theme;
mod tools;
mod ui;

use anyhow::Result;
use app::App;
use config::Config;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyboardEnhancementFlags, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::{execute, terminal};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const HELP: &str = "\
anduin — run Claude Code side by side in several directories

usage: anduin [DIR...]

  DIR    directories to add to the list and open. Without DIR, the
         'default' folder from the config is opened, if set.

config: ";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        let path = config::config_path().map(|p| p.display().to_string()).unwrap_or_default();
        println!("{HELP}{path}");
        return Ok(());
    }

    let mut cfg = Config::load();
    let mut selected = 0;
    let dirs: Vec<PathBuf> = if let (true, Some(default)) = (args.is_empty(), &cfg.default) {
        vec![config::expand_tilde(default)]
    } else if args.is_empty() && cfg.projects.is_empty() {
        // Launched from the Dock the cwd is ~ or /, which is not a project.
        let cwd = std::env::current_dir()?;
        if Some(&cwd) == dirs::home_dir().as_ref() || cwd.parent().is_none() {
            Vec::new()
        } else {
            vec![cwd]
        }
    } else {
        args.iter().map(|a| config::expand_tilde(a)).collect()
    };
    let mut added = Vec::new();
    let mut skipped = Vec::new();
    for dir in &dirs {
        match cfg.add(dir) {
            Ok(i) => added.push(i),
            Err(_) => skipped.push(dir.display().to_string()),
        }
    }
    if let Some(&first) = added.first() {
        selected = first;
        cfg.save()?;
    } else if !args.is_empty() {
        // Explicit arguments that don't exist are an error; a missing
        // default folder only gets a message in the status bar.
        anyhow::bail!("directory not found: {}", skipped.join(", "));
    }
    let dirs = added;

    let mut term = ratatui::init();
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    let mut out = std::io::stdout();
    execute!(out, EnableBracketedPaste, EnableMouseCapture)?;
    if enhanced {
        // Lets us tell Shift+Enter apart from Enter.
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }

    let size = term.size()?;
    let mut app = App::new(cfg, selected, size.width, size.height);
    if !dirs.is_empty() {
        app.start_claude();
    }
    if !skipped.is_empty() {
        app.message = Some(format!("skipped (not found): {}", skipped.join(", ")));
    }

    let result = run(&mut term, &mut app);

    if enhanced {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(out, DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
    result
}

fn run(term: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    let frame_time = Duration::from_millis(16);
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let mut needs_draw = true;

    while !app.quit {
        app.tick();
        // Redraw on output, input, or periodically so status icons stay fresh.
        let output = app.dirty.swap(false, Ordering::Relaxed);
        let idle_redraw = Duration::from_millis(if app.animating { 100 } else { 500 });
        if needs_draw || output || last_draw.elapsed() > idle_redraw {
            term.draw(|f| ui::draw(f, app))?;
            last_draw = Instant::now();
            needs_draw = false;
        }
        if event::poll(frame_time)? {
            loop {
                let ev = event::read()?;
                let moved = matches!(&ev, Event::Mouse(m) if m.kind == MouseEventKind::Moved);
                let hover = app.hover;
                app.handle(ev);
                // Plain mouse movement only matters when the hover target changes.
                if !moved || app.hover != hover {
                    needs_draw = true;
                }
                if app.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }
    Ok(())
}
