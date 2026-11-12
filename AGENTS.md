# Agent guide

Anduin is a Rust TUI that runs Claude Code sessions side by side, plus a
SwiftTerm-based macOS app (`macos/app/`) that hosts it in a native window.
README.md covers features, keys and config; this file covers what you need to
change the code safely.

## Map

- `src/app.rs`: state, input handling (modes: normal, prefix, scroll, add-dir, confirm, new-session chooser), sessions per folder.
- `src/ui.rs`: drawing. Records clickable areas in `app.hits` while drawing; later entries win.
- `src/session.rs`: one PTY + vt100 parser per session; answers terminal queries (DSR, DA, OSC 10/11).
- `src/mascot.rs`: finds Claude's mascot on screen and paints the configured one over it.
- `src/theme.rs`: tint, rotating Claude's orange hues only.
- `src/keys.rs`: key and mouse encoding sent to the child.
- `src/tools.rs`: what a session can run (Claude, a shell, other AI CLIs) and whether each is on `PATH`. Tint and mascot apply to Claude sessions only.
- `src/config.rs`, `src/picker.rs`: config file, Finder folder picker.

## Build and verify

- `cargo test` builds the test profile only. Run `cargo build --release` before
  exercising `target/release/anduin` or building the app.
- `macos/build-app.sh [dest]` installs to `/Applications` by default. Pass a
  scratch folder while testing so the user's installed app stays untouched.
- Point test runs at a throwaway config with `ANDUIN_CONFIG=/path/config.json`
  (folders you open get saved into it). Keep `HOME` as is: Claude Code reads its
  login from there.
- Verify UI changes by running the binary in a PTY and rebuilding the screen
  with an emulator such as Python's `pyte`. The harness must answer the DA query
  (`ESC [ c`) and keep draining output until the process exits, or anduin blocks.
- Set `"command"` in the test config to a small script that prints Claude-like
  output for fast, deterministic runs. Do a final check against real `claude`.
- When launching Anduin.app from inside a Claude Code session, start it with a
  clean environment (`env -i HOME=$HOME USER=$USER PATH=/usr/bin:/bin open -n ...`):
  inherited `CLAUDE_*` variables change how the inner Claude starts.

## Gotchas

- **Clickable UI stays visible.** Terminal.app sends no hover events, so every
  button is drawn at all times; hover only adds a highlight.
- **Mascot detection follows Claude Code's banner.** The title "Claude Code" sits
  at column `c` with the mascot at `c - 11` (`TITLE_OFFSET`), and Claude animates
  its mascot (jumps, crouches, blank rows). After a Claude Code update, recheck
  `mascot::find` against real output; the unit tests use captured layouts.
- **Tint touches warm hues only** (8–38°, saturation ≥ 0.25) so error red and
  warning yellow keep their meaning.
- **Prefix key matching** goes through `keys::ctrl_byte`, which accepts the
  legacy digit form (Ctrl+] as Ctrl+5) and the kitty keyboard form that SwiftTerm sends.
- **Bell in the Swift app**: `LocalProcessTerminalView` answers the view-level
  `bell` through a protocol default, so the Dock bounce overrides
  `bell(source: Terminal)` instead.
- The app starts `anduin-cli` through the user's login + interactive shell so
  `PATH` (where `claude` lives) matches their terminal.
