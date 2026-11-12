use crate::app::{App, Click, Confirm, Focus, Mode, SIDEBAR_WIDTH, Sel, Slot};
use crate::config::shorten;
use crate::mascot::Mascot;
use crate::theme::Theme;
use crate::tools::Kind;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use std::time::{SystemTime, UNIX_EPOCH};

const PANEL: Color = Color::Rgb(22, 24, 30);
const ITEM_SELECTED: Color = Color::Rgb(38, 43, 56);
const ITEM_SELECTED_DIM: Color = Color::Rgb(32, 35, 45);
const ITEM_HOVER: Color = Color::Rgb(28, 31, 39);
const BUTTON: Color = Color::Rgb(34, 38, 48);
const TEXT: Color = Color::Rgb(222, 224, 230);
const SUBTLE: Color = Color::Rgb(178, 182, 194);
const MUTED: Color = Color::Rgb(118, 123, 138);
const FAINT: Color = Color::Rgb(68, 72, 86);
const GREEN: Color = Color::Rgb(110, 200, 130);
const YELLOW: Color = Color::Rgb(240, 190, 80);
const RED: Color = Color::Rgb(230, 100, 110);

pub fn draw(f: &mut Frame, app: &mut App) {
    let [main, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
    let [side, pane] =
        Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(0)]).areas(main);
    draw_sidebar(f, app, side);
    draw_pane(f, app, pane);
    draw_status(f, app, status);
    match app.mode {
        Mode::Confirm(confirm) => draw_confirm(f, app, confirm),
        Mode::NewSession { folder, cursor } => draw_launcher(f, app, folder, cursor),
        _ => {}
    }
}

/// Centered list of what a new session can run; tools that aren't installed
/// are shown dimmed and can't be picked.
fn draw_launcher(f: &mut Frame, app: &mut App, folder: usize, cursor: usize) {
    let accent = app.theme.accent;
    let area = f.area();
    let name = app.cfg.projects.get(folder).map(|p| p.name.clone()).unwrap_or_default();
    let width = 58.min(area.width);
    let height = (app.tools.len() as u16 + 5).min(area.height);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, rect);
    let title = Line::from(vec![
        Span::styled(" New session ", Style::new().fg(TEXT).bold()),
        Span::styled(format!("· {} ", truncate_end(&name, width.saturating_sub(20) as usize)), Style::new().fg(MUTED)),
    ]);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .title(title)
        .style(Style::new().bg(PANEL));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    app.hits.push((rect, Click::Dialog));

    let list = Rect::new(inner.x, inner.y + 1, inner.width, inner.height.saturating_sub(3));
    let w = list.width as usize;
    for (i, tool) in app.tools.iter().enumerate().take(list.height as usize) {
        let row = Rect::new(list.x, list.y + i as u16, list.width, 1);
        let selected = i == cursor;
        let bg = if selected {
            ITEM_SELECTED
        } else if app.hover == Some(Click::Tool(i)) && tool.available {
            ITEM_HOVER
        } else {
            PANEL
        };
        let (glyph, glyph_color) = match tool.kind {
            Kind::Claude => ("✻", accent),
            Kind::Shell => ("❯", GREEN),
            Kind::Other => ("◇", SUBTLE),
        };
        let (glyph_color, name_style, right, right_color) = if tool.available {
            let name_style = if selected { Style::new().fg(TEXT).bold() } else { Style::new().fg(SUBTLE) };
            (glyph_color, name_style, tool.program(), MUTED)
        } else {
            (FAINT, Style::new().fg(FAINT), "not installed".to_string(), FAINT)
        };
        let num = if i < 9 { format!("{} ", i + 1) } else { "  ".into() };
        let bar = if selected { Span::styled("▌", Style::new().fg(accent)) } else { Span::raw(" ") };
        // bar(1) + space(1) + num(2) + space(1) + glyph(2) | right + space(2)
        let label = truncate_end(&tool.name, w.saturating_sub(7 + right.chars().count() + 3));
        let pad = w.saturating_sub(7 + label.chars().count() + right.chars().count() + 2);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                bar,
                Span::styled(format!(" {num} "), Style::new().fg(if tool.available { MUTED } else { FAINT })),
                Span::styled(format!("{glyph} "), Style::new().fg(glyph_color)),
                Span::styled(label, name_style),
                Span::raw(" ".repeat(pad)),
                Span::styled(right, Style::new().fg(right_color)),
            ]))
            .style(Style::new().bg(bg)),
            row,
        );
        app.hits.push((row, Click::Tool(i)));
    }

    let hint_row = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    let key = |k: &str| Span::styled(k.to_string(), Style::new().fg(accent).bold());
    let txt = |t: &str| Span::styled(t.to_string(), Style::new().fg(MUTED));
    f.render_widget(
        Paragraph::new(Line::from(vec![
            key("↑↓"), txt(" choose  "), key("Enter"), txt(" start  "),
            key("1-9"), txt(" quick  "), key("Esc"), txt(" cancel"),
        ]))
        .alignment(Alignment::Center),
        hint_row,
    );
}

/// Centered yes/no dialog with clickable buttons.
fn draw_confirm(f: &mut Frame, app: &mut App, confirm: Confirm) {
    let (title, message, yes): (String, String, &str) = match confirm {
        Confirm::RemoveFolder(fi) => {
            let name = app.cfg.projects.get(fi).map(|p| p.name.clone()).unwrap_or_default();
            (
                format!("Remove “{name}”?"),
                "Its sessions are closed. Files on disk are not touched.".into(),
                "Remove",
            )
        }
        Confirm::CloseSession(fi, s) => {
            let slot = app.slot(Sel { folder: fi, session: Some(s) });
            let label = slot.map(session_label).unwrap_or_default();
            let noun = slot.map(|s| s.tool.noun().to_string()).unwrap_or_default();
            (format!("Close {label}?"), format!("{noun} in this session will be stopped."), "Close")
        }
        Confirm::Quit => (
            "Quit anduin?".to_string(),
            "All running sessions will be stopped.".into(),
            "Quit",
        ),
    };

    let width = (message.chars().count().max(title.chars().count()) as u16 + 6).min(f.area().width);
    let area = f.area();
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(6) / 2,
        width,
        6.min(area.height),
    );
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(RED))
        .style(Style::new().bg(PANEL));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let [title_row, message_row, _, buttons] = Layout::vertical([Constraint::Length(1); 4]).areas(inner);
    f.render_widget(
        Paragraph::new(Span::styled(title, Style::new().fg(TEXT).bold())).alignment(Alignment::Center),
        title_row,
    );
    f.render_widget(
        Paragraph::new(Span::styled(message, Style::new().fg(MUTED))).alignment(Alignment::Center),
        message_row,
    );

    let yes_label = format!(" {yes} (y) ");
    let no_label = " Cancel (n) ".to_string();
    let total = (yes_label.len() + no_label.len() + 2) as u16;
    let x = buttons.x + buttons.width.saturating_sub(total) / 2;
    let yes_rect = Rect::new(x, buttons.y, yes_label.len() as u16, 1);
    let no_rect = Rect::new(yes_rect.right() + 2, buttons.y, no_label.len() as u16, 1);
    f.render_widget(
        Paragraph::new(yes_label).style(Style::new().fg(PANEL).bg(RED).bold()),
        yes_rect,
    );
    f.render_widget(Paragraph::new(no_label).style(Style::new().fg(TEXT).bg(BUTTON)), no_rect);
    app.hits.push((yes_rect, Click::ConfirmYes));
    app.hits.push((no_rect, Click::ConfirmNo));
}

/// One row of the sidebar list.
#[derive(Clone, Copy)]
enum Row {
    Header(usize),
    Session(usize, usize),
    Empty(usize),
    Spacer,
}

fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    let accent = app.theme.accent;
    app.hits.clear();
    f.buffer_mut().set_style(area, Style::new().bg(PANEL));
    let [header, rule, list_area, button, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    // Header: name on the left, running count on the right.
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ◆ ", Style::new().fg(accent).bold()),
            Span::styled("anduin", Style::new().fg(TEXT).bold()),
        ])),
        header,
    );
    let running = app.running_count();
    if running > 0 {
        // Styled on the span: a Paragraph style would recolor the whole row.
        let count = format!("{running} running ");
        f.render_widget(
            Paragraph::new(Span::styled(count, Style::new().fg(MUTED))).alignment(Alignment::Right),
            header,
        );
    }
    let rule = inset(rule, 1);
    f.render_widget(Paragraph::new("─".repeat(rule.width as usize)).fg(FAINT), rule);

    // "+ Add folder" button.
    let button = inset(button, 1);
    let hovered = app.hover == Some(Click::AddFolder);
    let (bg, fg, plus) = if hovered { (accent, PANEL, PANEL) } else { (BUTTON, TEXT, accent) };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("+ ", Style::new().fg(plus).bold()),
            Span::styled("Add folder", Style::new().fg(fg)),
        ]))
        .alignment(Alignment::Center)
        .style(Style::new().bg(bg)),
        button,
    );
    app.hits.push((button, Click::AddFolder));

    if app.folders.is_empty() {
        let hint = Paragraph::new(vec![
            Line::from("No folders yet").fg(SUBTLE),
            Line::from(""),
            Line::from("Click “+ Add folder”").fg(MUTED),
            Line::from("or press n").fg(MUTED),
        ])
        .alignment(Alignment::Center);
        f.render_widget(hint, centered(list_area, 4));
        return;
    }

    let mut rows = Vec::new();
    for (fi, folder) in app.folders.iter().enumerate() {
        rows.push(Row::Header(fi));
        if folder.sessions.is_empty() {
            rows.push(Row::Empty(fi));
        }
        rows.extend((0..folder.sessions.len()).map(|s| Row::Session(fi, s)));
        rows.push(Row::Spacer);
    }

    // Scroll so the selection (and its folder header, if possible) is visible.
    let height = list_area.height as usize;
    let selected_row = app.sel.and_then(|sel| {
        rows.iter().position(|row| match (*row, sel.session) {
            (Row::Session(fi, s), Some(sel_s)) => fi == sel.folder && s == sel_s,
            (Row::Empty(fi), None) => fi == sel.folder,
            _ => false,
        })
    });
    if let Some(r) = selected_row {
        app.side_scroll = app.side_scroll.min(r.saturating_sub(1));
        if r >= app.side_scroll + height {
            app.side_scroll = r + 1 - height;
        }
    }
    app.side_scroll = app.side_scroll.min(rows.len().saturating_sub(height));

    for (i, row) in rows.iter().enumerate().skip(app.side_scroll).take(height) {
        let y = list_area.y + (i - app.side_scroll) as u16;
        draw_row(f, app, *row, Rect::new(list_area.x, y, list_area.width, 1));
    }
}

fn draw_row(f: &mut Frame, app: &mut App, row: Row, rect: Rect) {
    let accent = app.theme.accent;
    let width = rect.width as usize;
    // A small clickable button at the right edge, `slot` 0 being rightmost.
    let button_rect = |slot: u16| Rect::new(rect.right() - 3 * (slot + 1), rect.y, 3, 1);
    let button = |label: &str, hovered: bool, color: Color| {
        let style = if hovered {
            Style::new().fg(PANEL).bg(color).bold()
        } else {
            Style::new().fg(color)
        };
        Paragraph::new(format!(" {label} ")).style(style)
    };

    match row {
        Row::Spacer => {}
        Row::Header(fi) => {
            let project = &app.cfg.projects[fi];
            let active = app.sel.is_some_and(|s| s.folder == fi);
            let hover = app.hover;
            let row_hovered = matches!(
                hover,
                Some(Click::Folder(h) | Click::NewSession(h) | Click::RemoveFolder(h)) if h == fi
            );
            let bg = if row_hovered { ITEM_HOVER } else { PANEL };
            let num = if fi < 9 { format!("{} ", fi + 1) } else { "  ".into() };
            let name = truncate_end(&project.name, width.saturating_sub(10));
            let name_style = if active {
                Style::new().fg(TEXT).bold()
            } else {
                Style::new().fg(SUBTLE).bold()
            };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!(" {num}"), Style::new().fg(FAINT)),
                    Span::styled(name, name_style),
                ]))
                .style(Style::new().bg(bg)),
                rect,
            );
            app.hits.push((rect, Click::Folder(fi)));

            // Always drawn: Terminal.app doesn't report hover, so buttons
            // that appear on hover would never show up there.
            let lit = row_hovered || active;
            let plus = button_rect(1);
            let plus_color = if lit { accent } else { MUTED };
            f.render_widget(button("+", hover == Some(Click::NewSession(fi)), plus_color), plus);
            app.hits.push((plus, Click::NewSession(fi)));
            let close = button_rect(0);
            let close_color = if lit { RED } else { FAINT };
            f.render_widget(button("×", hover == Some(Click::RemoveFolder(fi)), close_color), close);
            app.hits.push((close, Click::RemoveFolder(fi)));
        }
        Row::Empty(fi) => {
            let sel = Sel { folder: fi, session: None };
            let style = entry_style(app, sel, false);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    selection_bar(app, sel),
                    Span::styled("   ▸ ", Style::new().fg(FAINT)),
                    Span::styled("start a session", Style::new().fg(MUTED)),
                ]))
                .style(style),
                rect,
            );
            app.hits.push((rect, Click::Select(sel)));
        }
        Row::Session(fi, s) => {
            let sel = Sel { folder: fi, session: Some(s) };
            let close_hovered = app.hover == Some(Click::CloseSession(fi, s));
            let style = entry_style(app, sel, close_hovered);
            let slot = &app.folders[fi].sessions[s];
            let (icon, status_label, color) = status(slot);
            let label = session_label(slot);
            let close_color = if style.bg != Some(PANEL) { RED } else { FAINT };
            // bar(1) + indent(3) + icon(2) | status + space + close(3)
            let right = status_label.len() + 1 + 3;
            let label = truncate_end(&label, width.saturating_sub(6 + right + 1));
            let pad = width.saturating_sub(6 + label.chars().count() + right);
            let label_style = if app.sel == Some(sel) {
                Style::new().fg(TEXT)
            } else {
                Style::new().fg(SUBTLE)
            };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    selection_bar(app, sel),
                    Span::raw("   "),
                    Span::styled(format!("{icon} "), Style::new().fg(color)),
                    Span::styled(label, label_style),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(status_label, Style::new().fg(color)),
                ]))
                .style(style),
                rect,
            );
            app.hits.push((rect, Click::Select(sel)));
            let close = button_rect(0);
            f.render_widget(button("×", close_hovered, close_color), close);
            app.hits.push((close, Click::CloseSession(fi, s)));
        }
    }
}

/// Background for a selectable entry.
fn entry_style(app: &App, sel: Sel, child_hovered: bool) -> Style {
    let bg = if app.sel == Some(sel) && app.focus == Focus::Sidebar {
        ITEM_SELECTED
    } else if app.sel == Some(sel) {
        ITEM_SELECTED_DIM
    } else if app.hover == Some(Click::Select(sel)) || child_hovered {
        ITEM_HOVER
    } else {
        PANEL
    };
    Style::new().bg(bg)
}

fn selection_bar(app: &App, sel: Sel) -> Span<'static> {
    if app.sel == Some(sel) {
        Span::styled("▌", Style::new().fg(app.theme.accent))
    } else {
        Span::raw(" ")
    }
}

/// "#2 Fix login bug" using the title the program sets, else "#2 new session"
/// for Claude or "#2 Codex" for other tools.
pub fn session_label(slot: &Slot) -> String {
    let title = slot.session.parser().callbacks().title.clone().unwrap_or_default();
    // Claude prefixes its title with a status glyph such as "✳ ".
    let title = title.trim_start_matches(|c: char| !c.is_alphanumeric()).trim();
    if title.is_empty() || title == "Claude Code" {
        match slot.tool.kind {
            Kind::Claude => format!("#{} new session", slot.id),
            _ => format!("#{} {}", slot.id, slot.tool.name),
        }
    } else {
        format!("#{} {title}", slot.id)
    }
}

fn status(slot: &Slot) -> (String, &'static str, Color) {
    let s = &slot.session;
    if slot.attention {
        ("◆".into(), "needs you", YELLOW)
    } else if s.exit_code.is_some() {
        ("✕".into(), "exited", RED)
    } else if s.busy() {
        (spinner(), "working", GREEN)
    } else {
        ("○".into(), "idle", MUTED)
    }
}

fn spinner() -> String {
    const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    FRAMES[(ms / 100 % 10) as usize].to_string()
}

fn draw_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Term;
    let accent = app.theme.accent;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { accent } else { FAINT }));
    app.pane_area = block.inner(area);
    (app.animating, app.mascot_rect) = draw_pane_inner(f, app, block, area);
}

/// Draws the pane; returns whether an animated mascot is visible, and where
/// our mascot was drawn.
fn draw_pane_inner(f: &mut Frame, app: &App, mut block: Block, area: Rect) -> (bool, Option<Rect>) {
    let focused = app.focus == Focus::Term;
    let accent = app.theme.accent;

    let Some(sel) = app.sel else {
        let inner = block.inner(area);
        f.render_widget(block, area);
        let msg = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("◆ ", Style::new().fg(accent).bold()),
                Span::styled("anduin", Style::new().fg(TEXT).bold()),
            ]),
            Line::from(""),
            Line::from("Add a folder to run Claude Code in it.").fg(MUTED),
        ])
        .alignment(Alignment::Center);
        f.render_widget(msg, centered(inner, 3));
        return (false, None);
    };
    let project = &app.cfg.projects[sel.folder];
    let mut title = vec![
        Span::styled(format!(" {} ", project.name), Style::new().fg(TEXT).bold()),
        Span::styled(format!("{} ", shorten(&project.path)), Style::new().fg(MUTED)),
    ];
    if let Some(slot) = app.current_slot() {
        title.push(Span::styled(format!("· #{} ", slot.id), Style::new().fg(accent)));
        if slot.tool.kind != Kind::Claude {
            title.push(Span::styled(format!("{} ", slot.tool.name), Style::new().fg(MUTED)));
        }
    }
    block = block.title(Line::from(title));

    let inner = block.inner(area);
    let Some(session) = app.current() else {
        f.render_widget(block, area);
        if matches!(app.mode, Mode::NewSession { .. }) {
            return (false, None);
        }
        let msg = Paragraph::new(vec![
            Line::from("No session in this folder yet.").fg(SUBTLE),
            Line::from(""),
            Line::from("Press Enter to choose what to start.").fg(MUTED),
        ])
        .alignment(Alignment::Center);
        f.render_widget(msg, centered(inner, 3));
        return (false, None);
    };

    let mut parser = session.parser();
    if let Some(title) = &parser.callbacks().title {
        block = block.title_top(Line::from(format!(" {title} ")).right_aligned().fg(MUTED));
    }
    let screen = parser.screen_mut();
    let offset = screen.scrollback();
    if offset > 0 {
        // Determine the scrollback length by clamping to the maximum.
        screen.set_scrollback(usize::MAX);
        let total = screen.scrollback();
        screen.set_scrollback(offset);
        block = block.title_bottom(
            Line::from(format!(" scroll {offset}/{total} ")).right_aligned().fg(YELLOW),
        );
    }
    if let Some(code) = session.exit_code {
        block = block.title_bottom(
            Line::from(format!(" exited ({code}) · Enter to restart ")).centered().fg(RED),
        );
    }
    f.render_widget(block, area);
    let screen = parser.screen();
    // Tint and mascot are about Claude's look; other programs render as-is.
    let is_claude = app.current_slot().is_some_and(|s| s.tool.kind == Kind::Claude);
    let plain = Theme::new("none");
    render_screen(screen, inner, f.buffer_mut(), if is_claude { &app.theme } else { &plain });
    let mut animating = false;
    let mut mascot_rect = None;
    // Not while scrolled back: the mascot is only replaced on the live view.
    if let Some(mascot) = app.mascot.as_ref().filter(|_| offset == 0 && is_claude) {
        if let Some((row, col)) = Mascot::find(screen) {
            mascot.draw(f.buffer_mut(), inner, row, col);
            animating = mascot.is_animated();
            mascot_rect = Some(
                Rect::new(inner.x + col, inner.y + row, crate::mascot::COLS as u16, crate::mascot::ROWS as u16)
                    .intersection(inner),
            );
        }
    }

    let show_cursor = focused
        && matches!(app.mode, Mode::Normal)
        && session.exit_code.is_none()
        && offset == 0
        && !screen.hide_cursor();
    if show_cursor {
        let (row, col) = screen.cursor_position();
        if row < inner.height && col < inner.width {
            f.set_cursor_position((inner.x + col, inner.y + row));
        }
    }
    (animating, mascot_rect)
}

fn render_screen(screen: &vt100::Screen, area: Rect, buf: &mut Buffer, theme: &Theme) {
    for row in 0..area.height {
        for col in 0..area.width {
            let Some(cell) = screen.cell(row, col) else { continue };
            if cell.is_wide_continuation() {
                continue;
            }
            let mut style = Style::new()
                .fg(theme.color(cell.fgcolor()))
                .bg(theme.color(cell.bgcolor()));
            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.dim() {
                style = style.add_modifier(Modifier::DIM);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let out = &mut buf[(area.x + col, area.y + row)];
            out.set_symbol(if cell.has_contents() { cell.contents() } else { " " });
            out.set_style(style);
        }
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let accent = app.theme.accent;
    let key = |k: &str| Span::styled(k.to_string(), Style::new().fg(accent).bold());
    let txt = |t: &str| Span::styled(t.to_string(), Style::new().fg(MUTED));
    let p = app.prefix_label();

    let line = match &app.mode {
        Mode::AddDir { input, error } => {
            let mut spans = vec![key(" add dir: "), Span::styled(input.clone(), Style::new().fg(TEXT))];
            let cursor_x = area.x + 11 + input.chars().count() as u16;
            f.set_cursor_position((cursor_x.min(area.right().saturating_sub(1)), area.y));
            match error {
                Some(e) => spans.push(Span::styled(format!("   {e}"), Style::new().fg(RED))),
                None => spans.push(txt("   Tab complete · Enter add · Esc cancel")),
            }
            Line::from(spans)
        }
        Mode::NewSession { .. } => Line::from(vec![
            key(" j/k"), txt(" choose  "),
            key("Enter"), txt(" start  "),
            key("1-9"), txt(" start that one  "),
            key("Esc"), txt(" cancel  "),
            txt("dimmed tools aren't installed"),
        ]),
        Mode::Confirm(_) => Line::from(vec![
            key(" y"), txt(" / "), key("Enter"), txt(" confirm   "),
            key("n"), txt(" / "), key("Esc"), txt(" cancel"),
        ]),
        Mode::Prefix => Line::from(vec![
            key(" PREFIX "),
            key(" j/k"), txt(" switch "),
            key(" 1-9"), txt(" jump "),
            key(" s"), txt(" sidebar "),
            key(" c"), txt(" new session "),
            key(" n"), txt(" add folder "),
            key(" ["), txt(" scroll "),
            key(" r"), txt(" restart "),
            key(" x"), txt(" close "),
            key(" q"), txt(" quit "),
            key(&format!(" {p}")), txt(" send literal"),
        ]),
        Mode::Scroll => Line::from(vec![
            key(" SCROLL "),
            key(" j/k"), txt(" line "),
            key(" u/d"), txt(" half page "),
            key(" g/G"), txt(" top/bottom "),
            key(" q"), txt(" exit"),
        ]),
        Mode::Normal => match (&app.message, app.focus) {
            (Some(m), _) => Line::from(Span::styled(format!(" {m}"), Style::new().fg(YELLOW))),
            (None, Focus::Sidebar) => Line::from(vec![
                key(" j/k"), txt(" move "),
                key(" Enter"), txt(" open "),
                key(" c"), txt(" new session "),
                key(" x"), txt(" close session "),
                key(" n"), txt(" add folder "),
                key(" d"), txt(" remove folder "),
                key(" r"), txt(" restart "),
                key(" q"), txt(" quit "),
                key(&format!(" {p}")), txt(" prefix"),
            ]),
            (None, Focus::Term) => Line::from(vec![
                key(&format!(" {p}")), txt(" prefix  "),
                key(&format!("{p} j/k")), txt(" switch dir  "),
                key(&format!("{p} s")), txt(" sidebar  "),
                key(&format!("{p} [")), txt(" scroll"),
            ]),
        },
    };
    f.render_widget(Paragraph::new(line).style(Style::new().bg(PANEL)), area);
}

fn centered(area: Rect, height: u16) -> Rect {
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(area.x, y, area.width, height.min(area.height))
}

fn inset(area: Rect, x: u16) -> Rect {
    Rect::new(area.x + x, area.y, area.width.saturating_sub(2 * x), area.height)
}

fn truncate_end(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{keep}…")
}
