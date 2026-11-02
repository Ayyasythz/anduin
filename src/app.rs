use crate::config::{self, Config};
use crate::keys;
use crate::mascot::Mascot;
use crate::picker;
use crate::session::Session;
use crate::theme::Theme;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub const SIDEBAR_WIDTH: u16 = 34;

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Sidebar,
    Term,
}

pub enum Mode {
    Normal,
    /// Prefix key was pressed; the next key is an anduin command.
    Prefix,
    /// Browsing scrollback of the selected session.
    Scroll,
    AddDir { input: String, error: Option<String> },
    /// A yes/no dialog; answered with y/n or by clicking its buttons.
    Confirm(Confirm),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confirm {
    RemoveFolder(usize),
    CloseSession(usize, usize),
    Quit,
}

/// A selectable sidebar entry: a session, or a folder without sessions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sel {
    pub folder: usize,
    pub session: Option<usize>,
}

/// Something clickable in the sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Click {
    Select(Sel),
    Folder(usize),
    NewSession(usize),
    CloseSession(usize, usize),
    RemoveFolder(usize),
    AddFolder,
    ConfirmYes,
    ConfirmNo,
}

pub struct Slot {
    pub session: Session,
    /// Session rang the bell / sent a notification while not visible.
    pub attention: bool,
    /// Number shown in the sidebar, unique within its folder.
    pub id: u32,
}

#[derive(Default)]
pub struct Folder {
    pub sessions: Vec<Slot>,
    next_id: u32,
}

pub struct App {
    pub cfg: Config,
    pub folders: Vec<Folder>,
    pub sel: Option<Sel>,
    pub focus: Focus,
    pub mode: Mode,
    pub prefix: u8,
    pub message: Option<String>,
    pub quit: bool,
    pub dirty: Arc<AtomicBool>,
    pub theme: Theme,
    pub mascot: Option<Mascot>,
    /// Clickable regions from the last draw; later entries win.
    pub hits: Vec<(Rect, Click)>,
    pub hover: Option<Click>,
    pub pane_area: Rect,
    /// Set while drawing: an animated mascot is on screen.
    pub animating: bool,
    /// Where our mascot was drawn, for clicks.
    pub mascot_rect: Option<Rect>,
    /// First visible sidebar row.
    pub side_scroll: usize,
    /// Inner size of the terminal pane (rows, cols).
    pane: (u16, u16),
}

impl App {
    pub fn new(cfg: Config, selected: usize, width: u16, height: u16) -> Self {
        let prefix = keys::parse_prefix(&cfg.prefix).unwrap_or(0x1d);
        let theme = Theme::new(&cfg.tint);
        let (mascot, message) = match cfg.mascot.as_ref().map(|m| Mascot::load(m, theme.accent)) {
            Some(Ok(m)) => (m, None),
            Some(Err(e)) => (None, Some(format!("mascot: {e:#}"))),
            None => (None, None),
        };
        let folders = cfg.projects.iter().map(|_| Folder::default()).collect();
        let sel = (!cfg.projects.is_empty()).then(|| Sel {
            folder: selected.min(cfg.projects.len() - 1),
            session: None,
        });
        Self {
            cfg,
            folders,
            sel,
            focus: Focus::Sidebar,
            mode: Mode::Normal,
            prefix,
            message,
            quit: false,
            dirty: Arc::new(AtomicBool::new(true)),
            theme,
            mascot,
            hits: Vec::new(),
            hover: None,
            pane_area: Rect::default(),
            animating: false,
            mascot_rect: None,
            side_scroll: 0,
            pane: pane_size(width, height),
        }
    }

    /// All selectable entries in sidebar order.
    pub fn entries(&self) -> Vec<Sel> {
        let mut out = Vec::new();
        for (f, folder) in self.folders.iter().enumerate() {
            if folder.sessions.is_empty() {
                out.push(Sel { folder: f, session: None });
            }
            for s in 0..folder.sessions.len() {
                out.push(Sel { folder: f, session: Some(s) });
            }
        }
        out
    }

    pub fn slot(&self, sel: Sel) -> Option<&Slot> {
        self.folders.get(sel.folder)?.sessions.get(sel.session?)
    }

    pub fn current_slot(&self) -> Option<&Slot> {
        self.sel.and_then(|s| self.slot(s))
    }

    pub fn current(&self) -> Option<&Session> {
        self.current_slot().map(|s| &s.session)
    }

    pub fn prefix_label(&self) -> String {
        self.cfg.prefix.replace("ctrl-", "C-")
    }

    fn sessions(&self) -> impl Iterator<Item = &Slot> {
        self.folders.iter().flat_map(|f| &f.sessions)
    }

    pub fn running_count(&self) -> usize {
        self.sessions().filter(|s| s.session.exit_code.is_none()).count()
    }

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) => self.on_paste(&text),
            Event::Mouse(mouse) => self.on_mouse(mouse),
            Event::Resize(w, h) => self.resize(w, h),
            _ => {}
        }
    }

    /// Called every loop iteration: reaps exited children, collects bells.
    pub fn tick(&mut self) {
        let visible = if self.focus == Focus::Term { self.sel } else { None };
        let mut ring = false;
        for (f, folder) in self.folders.iter_mut().enumerate() {
            for (s, slot) in folder.sessions.iter_mut().enumerate() {
                slot.session.poll_exit();
                let rang = std::mem::take(&mut slot.session.parser().callbacks_mut().attention);
                if visible == Some(Sel { folder: f, session: Some(s) }) {
                    slot.attention = false;
                } else if rang {
                    slot.attention = true;
                    ring = true;
                }
            }
        }
        if ring {
            // Forward to the host terminal so it can notify the user.
            let _ = std::io::stdout().write_all(b"\x07");
            let _ = std::io::stdout().flush();
        }
    }

    fn resize(&mut self, w: u16, h: u16) {
        self.pane = pane_size(w, h);
        let (rows, cols) = self.pane;
        for slot in self.sessions() {
            slot.session.resize(rows, cols);
        }
    }

    fn on_paste(&mut self, text: &str) {
        match &mut self.mode {
            Mode::AddDir { input, .. } => input.push_str(text.trim()),
            Mode::Normal if self.focus == Focus::Term => {
                if let Some(s) = self.current().filter(|s| s.exit_code.is_none()) {
                    s.scroll_to(0);
                    if s.parser().screen().bracketed_paste() {
                        s.write(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                    } else {
                        s.write(text.as_bytes());
                    }
                }
            }
            _ => {}
        }
    }

    fn hit(&self, pos: Position) -> Option<Click> {
        self.hits.iter().rev().find(|(r, _)| r.contains(pos)).map(|(_, c)| *c)
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        self.hover = self.hit(pos);
        if let Mode::Confirm(confirm) = self.mode {
            // Any click outside the "yes" button cancels the dialog.
            if let MouseEventKind::Down(_) = m.kind {
                self.mode = Mode::Normal;
                if self.hover == Some(Click::ConfirmYes) {
                    self.confirm(confirm);
                }
            }
            return;
        }
        if !matches!(self.mode, Mode::Normal | Mode::Scroll) {
            return;
        }
        let in_pane = self.pane_area.contains(pos);

        // Clicking our mascot plays its animation instead of Claude's.
        if let (MouseEventKind::Down(MouseButton::Left), Some(rect)) = (m.kind, self.mascot_rect) {
            if rect.contains(pos) {
                if let Some(mascot) = self.mascot.as_mut() {
                    mascot.poke();
                    return;
                }
            }
        }

        // Programs that turned on mouse reporting get the events themselves.
        if in_pane && self.focus == Focus::Term {
            if let Some(s) = self.current().filter(|s| s.exit_code.is_none()) {
                let screen = s.parser();
                let screen = screen.screen();
                let (mode, enc) = (screen.mouse_protocol_mode(), screen.mouse_protocol_encoding());
                let col = m.column - self.pane_area.x;
                let row = m.row - self.pane_area.y;
                if let Some(bytes) = keys::encode_mouse(&m, col, row, mode, enc) {
                    s.write(&bytes);
                    return;
                }
            }
        }

        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.message = None;
                if let Some(click) = self.hover {
                    self.mode = Mode::Normal;
                    match click {
                        Click::Select(sel) => self.open(sel),
                        Click::Folder(f) => self.open(self.first_entry(f)),
                        Click::NewSession(f) => self.new_session(f),
                        Click::CloseSession(f, s) => {
                            self.mode = Mode::Confirm(Confirm::CloseSession(f, s))
                        }
                        Click::RemoveFolder(f) => self.mode = Mode::Confirm(Confirm::RemoveFolder(f)),
                        Click::AddFolder => self.pick_folder(),
                        Click::ConfirmYes | Click::ConfirmNo => {}
                    }
                } else if in_pane && self.sel.is_some() {
                    self.focus = Focus::Term;
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if in_pane => {
                if let Some(s) = self.current() {
                    s.scroll(if m.kind == MouseEventKind::ScrollUp { 3 } else { -3 });
                }
            }
            MouseEventKind::ScrollUp if !in_pane => self.move_selection(-1),
            MouseEventKind::ScrollDown if !in_pane => self.move_selection(1),
            _ => {}
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        self.message = None;
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        match mode {
            Mode::Normal => self.on_normal_key(key),
            Mode::Prefix => self.on_prefix_key(key),
            Mode::Scroll => self.on_scroll_key(key),
            Mode::AddDir { input, error } => self.on_add_dir_key(key, input, error),
            Mode::Confirm(confirm) => {
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Enter) {
                    self.confirm(confirm);
                }
            }
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
        if keys::is_ctrl_byte(&key, self.prefix) {
            self.mode = Mode::Prefix;
            return;
        }
        match self.focus {
            Focus::Term => self.on_term_key(key),
            Focus::Sidebar => self.on_sidebar_key(key),
        }
    }

    fn on_term_key(&mut self, key: KeyEvent) {
        let Some(sel) = self.sel else {
            self.focus = Focus::Sidebar;
            return;
        };
        let Some(s) = self.current().filter(|s| s.exit_code.is_none()) else {
            match key.code {
                KeyCode::Enter => self.restart(sel),
                KeyCode::Esc => self.focus = Focus::Sidebar,
                _ => {}
            }
            return;
        };
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            let half = (self.pane.0 / 2).max(1) as isize;
            match key.code {
                KeyCode::PageUp => return s.scroll(half),
                KeyCode::PageDown => return s.scroll(-half),
                _ => {}
            }
        }
        let app_cursor = s.parser().screen().application_cursor();
        if let Some(bytes) = keys::encode(&key, app_cursor) {
            s.scroll_to(0);
            s.write(&bytes);
        }
    }

    /// Commands shared by the sidebar and the prefix key. Returns false if
    /// the key isn't one of them.
    fn on_command_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c @ '1'..='9') => self.jump(c as usize - '1' as usize),
            KeyCode::Char('n') => self.pick_folder(),
            KeyCode::Char('N') => self.begin_add_dir(),
            KeyCode::Char('c') | KeyCode::Char('+') => {
                if let Some(sel) = self.sel {
                    self.new_session(sel.folder);
                }
            }
            KeyCode::Char('x') => {
                if let Some(Sel { folder, session: Some(s) }) = self.sel {
                    self.mode = Mode::Confirm(Confirm::CloseSession(folder, s));
                }
            }
            KeyCode::Char('d') => {
                if let Some(sel) = self.sel {
                    self.mode = Mode::Confirm(Confirm::RemoveFolder(sel.folder));
                }
            }
            KeyCode::Char('r') => {
                if let Some(sel) = self.sel {
                    self.restart(sel);
                }
            }
            KeyCode::Char('q') => self.request_quit(),
            _ => return false,
        }
        true
    }

    fn on_sidebar_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => {
                if let Some(sel) = self.sel {
                    self.open(sel);
                }
            }
            _ => {
                self.on_command_key(key);
            }
        }
    }

    fn on_prefix_key(&mut self, key: KeyEvent) {
        if keys::is_ctrl_byte(&key, self.prefix) {
            // Prefix twice sends the prefix byte itself.
            if let Some(s) = self.current() {
                s.write(&[self.prefix]);
            }
            return;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.switch(1),
            KeyCode::Char('k') | KeyCode::Up => self.switch(-1),
            KeyCode::Char('s') | KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Sidebar if self.sel.is_some() => Focus::Term,
                    _ => Focus::Sidebar,
                };
            }
            KeyCode::Char('[') if self.current().is_some() => self.mode = Mode::Scroll,
            _ => {
                self.on_command_key(key);
            }
        }
    }

    fn on_scroll_key(&mut self, key: KeyEvent) {
        let Some(s) = self.current() else { return };
        let half = (self.pane.0 / 2).max(1) as isize;
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => s.scroll(1),
            KeyCode::Char('j') | KeyCode::Down => s.scroll(-1),
            KeyCode::Char('u') | KeyCode::PageUp => s.scroll(half),
            KeyCode::Char('d') | KeyCode::PageDown => s.scroll(-half),
            KeyCode::Char('g') | KeyCode::Home => s.scroll_to(usize::MAX),
            KeyCode::Char('G') | KeyCode::End => s.scroll_to(0),
            KeyCode::Char('q') | KeyCode::Esc => {
                s.scroll_to(0);
                return;
            }
            _ => {}
        }
        self.mode = Mode::Scroll;
    }

    fn on_add_dir_key(&mut self, key: KeyEvent, mut input: String, error: Option<String>) {
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Enter => match self.add_dir(&input) {
                Ok(()) => return,
                Err(e) => {
                    self.mode = Mode::AddDir { input, error: Some(e.to_string()) };
                    return;
                }
            },
            KeyCode::Tab => input = config::complete_dir(&input),
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => input.clear(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => input.push(c),
            _ => {
                self.mode = Mode::AddDir { input, error };
                return;
            }
        }
        self.mode = Mode::AddDir { input, error: None };
    }

    /// Adds a folder chosen in the native Finder dialog and opens it.
    fn pick_folder(&mut self) {
        let start = self
            .sel
            .and_then(|s| self.cfg.projects[s.folder].path.parent().map(|p| p.to_path_buf()))
            .or_else(dirs::home_dir);
        match picker::choose_folder(start.as_deref()) {
            Ok(Some(path)) => match self.add_dir(&path.to_string_lossy()) {
                Ok(()) => {
                    if let Some(sel) = self.sel {
                        self.open(sel);
                    }
                }
                Err(e) => self.message = Some(format!("{e:#}")),
            },
            Ok(None) => {}
            Err(e) => self.message = Some(format!("{e:#}")),
        }
    }

    fn begin_add_dir(&mut self) {
        let input = std::env::current_dir()
            .map(|d| format!("{}/", config::shorten(&d)))
            .unwrap_or_default();
        self.mode = Mode::AddDir { input, error: None };
    }

    fn add_dir(&mut self, input: &str) -> anyhow::Result<()> {
        let path = config::expand_tilde(input.trim());
        let before = self.cfg.projects.len();
        let f = self.cfg.add(&path)?;
        if self.cfg.projects.len() > before {
            self.folders.push(Folder::default());
            self.save();
        }
        self.sel = Some(self.first_entry(f));
        self.focus = Focus::Sidebar;
        Ok(())
    }

    fn confirm(&mut self, confirm: Confirm) {
        match confirm {
            Confirm::RemoveFolder(f) => self.remove_folder(f),
            Confirm::CloseSession(f, s) => self.close_session(f, s),
            Confirm::Quit => self.quit = true,
        }
    }

    fn remove_folder(&mut self, f: usize) {
        if f >= self.folders.len() {
            return;
        }
        // Dropping the folder kills its sessions.
        self.folders.remove(f);
        let p = self.cfg.projects.remove(f);
        self.message = Some(format!("removed {}", p.name));
        self.save();
        self.sel = if self.folders.is_empty() {
            self.focus = Focus::Sidebar;
            None
        } else {
            Some(self.first_entry(f.min(self.folders.len() - 1)))
        };
    }

    fn close_session(&mut self, f: usize, s: usize) {
        let Some(folder) = self.folders.get_mut(f) else { return };
        if s >= folder.sessions.len() {
            return;
        }
        let slot = folder.sessions.remove(s);
        let left = folder.sessions.len();
        self.message = Some(format!("closed session {} of {}", slot.id, self.cfg.projects[f].name));
        let next = (left > 0).then(|| s.min(left - 1));
        self.sel = Some(Sel { folder: f, session: next });
        if next.is_none() {
            self.focus = Focus::Sidebar;
        }
    }

    fn request_quit(&mut self) {
        if self.running_count() > 0 {
            self.mode = Mode::Confirm(Confirm::Quit);
        } else {
            self.quit = true;
        }
    }

    fn first_entry(&self, f: usize) -> Sel {
        let session = (!self.folders[f].sessions.is_empty()).then_some(0);
        Sel { folder: f, session }
    }

    fn move_selection(&mut self, delta: isize) {
        let entries = self.entries();
        if entries.is_empty() {
            return;
        }
        let cur = self.sel.and_then(|s| entries.iter().position(|e| *e == s)).unwrap_or(0);
        let next = (cur as isize + delta).rem_euclid(entries.len() as isize) as usize;
        self.sel = Some(entries[next]);
    }

    /// Moves to the next/previous entry and focuses its terminal.
    fn switch(&mut self, delta: isize) {
        self.move_selection(delta);
        if let Some(sel) = self.sel {
            self.open(sel);
        }
    }

    /// Opens the first session of folder `f` (1-9 shortcuts).
    fn jump(&mut self, f: usize) {
        if f < self.folders.len() {
            self.open(self.first_entry(f));
        }
    }

    /// Selects an entry, starting a session for an empty folder, and focuses it.
    fn open(&mut self, sel: Sel) {
        match sel.session {
            None => self.new_session(sel.folder),
            Some(s) => {
                self.sel = Some(sel);
                self.focus = Focus::Term;
                if let Some(slot) = self.folders[sel.folder].sessions.get_mut(s) {
                    slot.attention = false;
                }
            }
        }
    }

    /// Starts another session in folder `f` and focuses it.
    fn new_session(&mut self, f: usize) {
        let Some(session) = self.spawn(f) else { return };
        let folder = &mut self.folders[f];
        folder.next_id += 1;
        folder.sessions.push(Slot { session, attention: false, id: folder.next_id });
        self.sel = Some(Sel { folder: f, session: Some(folder.sessions.len() - 1) });
        self.focus = Focus::Term;
    }

    /// Restarts the selected session in place, or starts one.
    fn restart(&mut self, sel: Sel) {
        let Some(s) = sel.session else {
            return self.new_session(sel.folder);
        };
        if let Some(session) = self.spawn(sel.folder) {
            // Replacing drops the old session, which kills its child.
            self.folders[sel.folder].sessions[s].session = session;
            self.sel = Some(sel);
            self.focus = Focus::Term;
        }
    }

    fn spawn(&mut self, f: usize) -> Option<Session> {
        let (rows, cols) = self.pane;
        let path = &self.cfg.projects[f].path;
        match Session::spawn(path, &self.cfg.command, rows, cols, self.dirty.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                self.message = Some(format!("{e:#}"));
                None
            }
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.cfg.save() {
            self.message = Some(format!("failed to save config: {e:#}"));
        }
    }
}

fn pane_size(width: u16, height: u16) -> (u16, u16) {
    // Pane has a 1-cell border and a status line below it.
    let rows = height.saturating_sub(3).max(1);
    let cols = width.saturating_sub(SIDEBAR_WIDTH + 2).max(1);
    (rows, cols)
}
