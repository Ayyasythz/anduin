//! Replaces Claude Code's 9x3 mascot with a custom one.
//!
//! A mascot is 18x6 "pixels": every terminal cell shows 2x2 of them through a
//! quadrant block character, with at most two colors per cell.

use crate::config::{MascotSpec, expand_tilde};
use crate::theme::is_claude_orange;
use anyhow::{Context, Result, bail};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use std::collections::HashMap;
use std::time::Instant;

pub const COLS: usize = 9;
pub const ROWS: usize = 3;
const PX_W: usize = COLS * 2;
const PX_H: usize = ROWS * 2;

/// Quadrant glyph by mask: bit 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right.
const QUAD: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

type Rgb = (u8, u8, u8);
/// Row-major pixels; `None` is transparent.
type Pixels = [[Option<Rgb>; PX_W]; PX_H];

#[derive(Clone, Copy, Default)]
struct Cell {
    ch: char,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
}

type Cells = [[Cell; COLS]; ROWS];

/// Frames and how long each stays on screen, in milliseconds.
type Frames = Vec<(Cells, u64)>;

pub struct Mascot {
    frames: Frames,
    /// Played once when clicked.
    click: Frames,
    start: Instant,
    clicked_at: Option<Instant>,
}

/// Built-in one-color designs; `#` is drawn in the accent color.
const PRESETS: &[(&str, [&str; PX_H])] = &[
    ("cat", [
        "...##........##...",
        "...####....####...",
        "...############...",
        "...##..####..##...",
        "...############...",
        ".....########.....",
    ]),
    ("robot", [
        "........##........",
        "....##########....",
        "....##########....",
        "....##..##..##....",
        "..##############..",
        "....##......##....",
    ]),
    ("ghost", [
        "......######......",
        "....##########....",
        "...###..##..###...",
        "...############...",
        "...############...",
        "...##..####..##...",
    ]),
    ("frog", [
        "...###......###...",
        "..##.##....##.##..",
        "..##############..",
        "..###.######.###..",
        "...############...",
        "..###........###..",
    ]),
];

impl Mascot {
    /// Builds the configured mascot; `None` keeps Claude's own.
    pub fn load(spec: &MascotSpec, accent: Color) -> Result<Option<Self>> {
        let accent = match accent {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => (215, 119, 87),
        };
        let still = |cells| Some(Self::new(vec![(cells, 1)], Vec::new()));
        let palette = |colors: &HashMap<String, String>| -> Result<HashMap<char, Rgb>> {
            let mut palette = HashMap::new();
            for (key, value) in colors {
                let k = key.chars().next().context("empty color key")?;
                let rgb = if value == "accent" { accent } else { parse_hex(value)? };
                palette.insert(k, rgb);
            }
            Ok(palette)
        };
        match spec {
            MascotSpec::Name(name) => match name.trim() {
                "" | "claude" => Ok(None),
                "none" => Ok(still(cells_from_pixels(&[[None; PX_W]; PX_H]))),
                name => {
                    if let Some((_, rows)) = PRESETS.iter().find(|(n, _)| *n == name) {
                        let colors = HashMap::from([('#', accent)]);
                        return Ok(still(Self::from_art(rows, &colors)?));
                    }
                    Ok(still(Self::from_image(&expand_tilde(name))?))
                }
            },
            MascotSpec::Art { pixels, colors } => Ok(still(Self::from_art(pixels, &palette(colors)?)?)),
            MascotSpec::Animated { frames, colors, click } => {
                let palette = palette(colors)?;
                let build = |list: &[crate::config::MascotFrame], what: &str| {
                    list.iter()
                        .enumerate()
                        .map(|(i, f)| {
                            let cells = Self::from_art(&f.pixels, &palette)
                                .with_context(|| format!("{what} frame {}", i + 1))?;
                            Ok((cells, f.ms.max(1)))
                        })
                        .collect::<Result<Frames>>()
                };
                let frames = build(frames, "mascot")?;
                if frames.is_empty() {
                    bail!("mascot has no frames");
                }
                Ok(Some(Self::new(frames, build(click, "click")?)))
            }
        }
    }

    fn new(frames: Frames, click: Frames) -> Self {
        Self { frames, click, start: Instant::now(), clicked_at: None }
    }

    /// Whether the picture changes over time right now.
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1 || self.click_frame().is_some()
    }

    /// Starts the click animation, if there is one.
    pub fn poke(&mut self) {
        if !self.click.is_empty() {
            self.clicked_at = Some(Instant::now());
        }
    }

    fn click_frame(&self) -> Option<&Cells> {
        let elapsed = self.clicked_at?.elapsed().as_millis() as u64;
        let mut t = elapsed;
        for (cells, ms) in &self.click {
            if t < *ms {
                return Some(cells);
            }
            t -= ms;
        }
        None
    }

    /// The looping frame to show `elapsed_ms` after start.
    fn frame_at(&self, elapsed_ms: u64) -> &Cells {
        let total: u64 = self.frames.iter().map(|(_, ms)| ms).sum();
        let mut t = elapsed_ms % total;
        for (cells, ms) in &self.frames {
            if t < *ms {
                return cells;
            }
            t -= ms;
        }
        &self.frames[0].0
    }

    /// Pixel art as 6 rows of 18 chars; `.` and space are transparent.
    fn from_art<S: AsRef<str>>(rows: &[S], palette: &HashMap<char, Rgb>) -> Result<Cells> {
        if rows.len() != PX_H {
            bail!("mascot needs {PX_H} rows of pixels, got {}", rows.len());
        }
        let mut px: Pixels = [[None; PX_W]; PX_H];
        for (y, row) in rows.iter().enumerate() {
            let chars: Vec<char> = row.as_ref().chars().collect();
            if chars.len() != PX_W {
                bail!("mascot row {} has {} pixels, expected {PX_W}", y + 1, chars.len());
            }
            for (x, c) in chars.into_iter().enumerate() {
                px[y][x] = match c {
                    '.' | ' ' => None,
                    c => Some(*palette.get(&c).with_context(|| format!("no color for '{c}'"))?),
                };
            }
        }
        Ok(cells_from_pixels(&px))
    }

    /// Converts an image: white background is removed, the subject is
    /// cropped and scaled to fit, then each cell picks its two best colors.
    fn from_image(path: &std::path::Path) -> Result<Cells> {
        let img = image::open(path)
            .with_context(|| format!("cannot open mascot image {}", path.display()))?
            .to_rgba8();
        let img = remove_background(img);
        let (x0, y0, w, h) = bounding_box(&img).context("mascot image is empty")?;
        let img = image::imageops::crop_imm(&img, x0, y0, w, h).to_image();

        // A pixel is twice as tall as wide on screen, so stretch horizontally.
        let k = (PX_W as f32 / (2.0 * w as f32)).min(PX_H as f32 / h as f32);
        let tw = ((2.0 * w as f32 * k).round() as u32).clamp(1, PX_W as u32);
        let th = ((h as f32 * k).round() as u32).clamp(1, PX_H as u32);
        let small = image::imageops::resize(&img, tw, th, image::imageops::FilterType::Lanczos3);

        let (ox, oy) = ((PX_W as u32 - tw) / 2, (PX_H as u32 - th) / 2);
        let mut px: Pixels = [[None; PX_W]; PX_H];
        for (x, y, p) in small.enumerate_pixels() {
            if p[3] >= 128 {
                px[(y + oy) as usize][(x + ox) as usize] = Some((p[0], p[1], p[2]));
            }
        }
        Ok(cells_from_pixels(&px))
    }

    /// Finds Claude's mascot and returns its top-left (row, col) on the
    /// visible screen. The mascot animates and its glyphs differ between
    /// terminals, so it is located by color and position, not by shape.
    pub fn find(screen: &vt100::Screen) -> Option<(u16, u16)> {
        find_by_title(screen).or_else(|| find_by_blob(screen))
    }

    /// Draws over the cells at screen position (row, col) inside `area`.
    pub fn draw(&self, buf: &mut Buffer, area: Rect, row: u16, col: u16) {
        let frame = self
            .click_frame()
            .unwrap_or_else(|| self.frame_at(self.start.elapsed().as_millis() as u64));
        for (dy, cells) in frame.iter().enumerate() {
            for (dx, cell) in cells.iter().enumerate() {
                let (x, y) = (area.x + col + dx as u16, area.y + row + dy as u16);
                if x >= area.right() || y >= area.bottom() {
                    continue;
                }
                let rgb = |c: Option<Rgb>| c.map_or(Color::Reset, |(r, g, b)| Color::Rgb(r, g, b));
                let out = &mut buf[(x, y)];
                out.reset();
                out.set_char(cell.ch);
                out.set_style(Style::new().fg(rgb(cell.fg)).bg(rgb(cell.bg)));
            }
        }
    }
}

const TITLE: &str = "Claude Code";
/// Columns from the mascot's left edge to the title next to it.
const TITLE_OFFSET: u16 = 11;

fn is_mascot_cell(screen: &vt100::Screen, r: u16, c: u16) -> bool {
    let Some(cell) = screen.cell(r, c) else { return false };
    let orange = |color| matches!(color, vt100::Color::Rgb(r, g, b) if is_claude_orange(r, g, b));
    let block = cell
        .contents()
        .chars()
        .next()
        .is_some_and(|ch| ('\u{2580}'..='\u{259F}').contains(&ch));
    (block && orange(cell.fgcolor())) || orange(cell.bgcolor())
}

fn row_has_mascot(screen: &vt100::Screen, r: u16, cols: std::ops::Range<u16>) -> bool {
    cols.into_iter().any(|c| is_mascot_cell(screen, r, c))
}

/// The welcome header: mascot, two spaces, then "Claude Code vX" on its top row.
fn find_by_title(screen: &vt100::Screen) -> Option<(u16, u16)> {
    let (rows, cols) = screen.size();
    let (rows, cols) = (rows.get(), cols.get());
    let title: Vec<char> = TITLE.chars().collect();
    for r in 0..rows.saturating_sub(2) {
        for c in TITLE_OFFSET..cols.saturating_sub(title.len() as u16) {
            let matches = title.iter().enumerate().all(|(i, ch)| {
                screen.cell(r, c + i as u16).is_some_and(|cell| cell.contents().starts_with(*ch))
            });
            if !matches {
                continue;
            }
            // The mascot always sits at the same spot left of the title, but
            // its animations (e.g. jumping when clicked) change its shape and
            // can leave rows empty, so any orange block in the area will do.
            let start = c - TITLE_OFFSET;
            let window = start.saturating_sub(1)..c - 1;
            if (r..r + 3).any(|row| row_has_mascot(screen, row, window.clone())) {
                return Some((r, start));
            }
        }
    }
    None
}

/// Fallback: an isolated 3-row blob of orange blocks, 5 to 9 columns wide.
fn find_by_blob(screen: &vt100::Screen) -> Option<(u16, u16)> {
    let (rows, cols) = screen.size();
    let (rows, cols) = (rows.get(), cols.get());
    for r in 0..rows.saturating_sub(2) {
        for c in 0..cols {
            if !is_mascot_cell(screen, r, c) {
                continue;
            }
            let lo = c.saturating_sub(COLS as u16 - 1);
            let hi = (c + COLS as u16).min(cols);
            let Some((min, max)) = blob_bounds(screen, r, lo..hi) else { continue };
            let isolated = r + 3 >= rows || !row_has_mascot(screen, r + 3, min..max + 1);
            if (r..r + 3).all(|row| row_has_mascot(screen, row, min..max + 1)) && isolated {
                if let Some(start) = blob_start(screen, r, lo..hi, cols) {
                    return Some((r, start));
                }
            }
        }
    }
    None
}

fn cells_from_pixels(px: &Pixels) -> Cells {
    let mut cells = [[Cell::default(); COLS]; ROWS];
    for (cy, row) in cells.iter_mut().enumerate() {
        for (cx, cell) in row.iter_mut().enumerate() {
            let quad = [
                px[cy * 2][cx * 2],
                px[cy * 2][cx * 2 + 1],
                px[cy * 2 + 1][cx * 2],
                px[cy * 2 + 1][cx * 2 + 1],
            ];
            *cell = encode_cell(quad);
        }
    }
    cells
}

/// Leftmost and rightmost mascot columns in rows r..r+3 within `cols`.
fn blob_bounds(screen: &vt100::Screen, r: u16, cols: std::ops::Range<u16>) -> Option<(u16, u16)> {
    let hits: Vec<u16> = (r..r + 3)
        .flat_map(|row| cols.clone().filter(move |&c| is_mascot_cell(screen, row, c)))
        .collect();
    Some((*hits.iter().min()?, *hits.iter().max()?))
}

/// Left column for our mascot, centered on Claude's so its animation frames
/// (which shift by a column) don't make ours jitter. `None` if the blob
/// isn't mascot-sized.
fn blob_start(screen: &vt100::Screen, r: u16, cols: std::ops::Range<u16>, width: u16) -> Option<u16> {
    let (min, max) = blob_bounds(screen, r, cols)?;
    if !(5..=COLS as u16).contains(&(max - min + 1)) {
        return None;
    }
    Some(((min + max) / 2).saturating_sub(4).min(width.saturating_sub(COLS as u16)))
}

/// Picks a glyph and up to two colors for a 2x2 block of pixels.
fn encode_cell(quad: [Option<Rgb>; 4]) -> Cell {
    let opaque: Vec<usize> = (0..4).filter(|&i| quad[i].is_some()).collect();
    if opaque.is_empty() {
        return Cell { ch: ' ', ..Cell::default() };
    }
    let rgb = |i: usize| quad[i].unwrap();
    if opaque.len() < 4 {
        // Transparent pixels show the terminal background, so only one color.
        let mask = opaque.iter().fold(0, |m, &i| m | 1 << i);
        return Cell { ch: QUAD[mask], fg: Some(average(opaque.iter().map(|&i| rgb(i)))), bg: None };
    }
    let all = average((0..4).map(rgb));
    let mut best = (error((0..4).map(rgb), all), 15usize, all, all);
    for mask in 1..15usize {
        let (a, b): (Vec<usize>, Vec<usize>) = (0..4).partition(|i| mask >> i & 1 == 1);
        let (ca, cb) = (average(a.iter().map(|&i| rgb(i))), average(b.iter().map(|&i| rgb(i))));
        let e = error(a.iter().map(|&i| rgb(i)), ca) + error(b.iter().map(|&i| rgb(i)), cb);
        if e < best.0 {
            best = (e, mask, ca, cb);
        }
    }
    let (_, mask, fg, bg) = best;
    Cell { ch: QUAD[mask], fg: Some(fg), bg: (mask != 15).then_some(bg) }
}

fn average(colors: impl Iterator<Item = Rgb>) -> Rgb {
    let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
    for c in colors {
        r += c.0 as u32;
        g += c.1 as u32;
        b += c.2 as u32;
        n += 1;
    }
    let n = n.max(1);
    ((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

fn error(colors: impl Iterator<Item = Rgb>, to: Rgb) -> u32 {
    colors
        .map(|c| {
            let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2) as u32;
            d(c.0, to.0) + d(c.1, to.1) + d(c.2, to.2)
        })
        .sum()
}

/// Makes near-white pixels connected to the image border transparent.
fn remove_background(mut img: image::RgbaImage) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    let mut seen = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for x in 0..w {
        stack.extend([(x, 0), (x, h - 1)]);
    }
    for y in 0..h {
        stack.extend([(0, y), (w - 1, y)]);
    }
    while let Some((x, y)) = stack.pop() {
        let i = (y * w + x) as usize;
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let p = img.get_pixel(x, y);
        if p[3] >= 128 && p[0].min(p[1]).min(p[2]) <= 225 {
            continue;
        }
        img.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
        if x > 0 {
            stack.push((x - 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    img
}

fn bounding_box(img: &image::RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] >= 128 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0 <= x1).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

fn parse_hex(s: &str) -> Result<Rgb> {
    let hex = s.trim_start_matches('#');
    if hex.len() != 6 {
        bail!("color '{s}' is not #rrggbb");
    }
    let v = u32::from_str_radix(hex, 16).with_context(|| format!("color '{s}' is not #rrggbb"))?;
    Ok(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude's mascot as captured from Claude Code 2.1, placed at row 3, col 4.
    const CLAUDE: &str = "\x1b[3;4H\x1b[38;2;215;119;87m▗\x1b[38;2;0;0;0;48;2;215;119;87m ▗   ▖ \
        \x1b[0m\x1b[38;2;215;119;87m▖\x1b[0m\
        \x1b[4;4H\x1b[48;2;215;119;87m         \x1b[0m\
        \x1b[5;4H \x1b[38;2;215;119;87m▘▘   ▝▝\x1b[0m";

    /// Another animation frame with fg-only glyphs, as Claude draws it
    /// inside anduin, next to its title.
    const CLAUDE_FG: &str = "\x1b[3;4H \x1b[38;2;215;119;87m▐▛███▛█\x1b[0m   Claude Code v2.1.314\
        \x1b[4;4H\x1b[38;2;215;119;87m▝▜██████▀\x1b[0m  Opus 5.5\
        \x1b[5;4H \x1b[38;2;215;119;87m▝▝   ▝▝\x1b[0m    ~/project";

    fn screen(input: &str) -> vt100::Parser {
        let mut p = vt100::Parser::new(10.try_into().unwrap(), 60.try_into().unwrap(), 0);
        p.process(input.as_bytes());
        p
    }

    #[test]
    fn finds_claude_mascot() {
        assert_eq!(Mascot::find(screen("").screen()), None);
        assert_eq!(Mascot::find(screen(CLAUDE).screen()), Some((2, 3)));
        assert_eq!(Mascot::find(screen(CLAUDE_FG).screen()), Some((2, 3)));
    }

    #[test]
    fn follows_claude_jump_animation() {
        // Crouch frame captured after clicking Claude's mascot: the top row
        // is empty and dust appears beside it.
        let crouch = "\x1b[3;4H           Claude Code v2.1.314\
            \x1b[4;4H \x1b[38;2;215;119;87m▐▛███▛█\x1b[0m   Opus 5.5\
            \x1b[5;4H·\x1b[38;2;215;119;87m▜██████\x1b[0m·  ~/project";
        assert_eq!(Mascot::find(screen(crouch).screen()), Some((2, 3)));
    }

    #[test]
    fn click_frames_play_once() {
        let spec: MascotSpec = serde_json::from_str(
            r##"{"colors": {"A": "#ff0000", "B": "#0000ff"},
                 "frames": [{"pixels": ["AA................","..................","..................",
                                        "..................","..................",".................."]}],
                 "click": [{"pixels": ["BB................","..................","..................",
                                        "..................","..................",".................."], "ms": 50}]}"##,
        )
        .unwrap();
        let mut m = Mascot::load(&spec, Color::Reset).unwrap().unwrap();
        assert!(!m.is_animated());
        m.poke();
        assert!(m.is_animated());
        assert_eq!(m.click_frame().unwrap()[0][0].fg, Some((0, 0, 255)));
        std::thread::sleep(std::time::Duration::from_millis(60));
        assert!(m.click_frame().is_none());
        assert!(!m.is_animated());
    }

    #[test]
    fn ignores_plain_mentions_and_bars() {
        let text = "\x1b[2;15HClaude Code is great";
        assert_eq!(Mascot::find(screen(text).screen()), None);
        // A one-row orange progress bar is not a mascot.
        let bar = "\x1b[2;1H\x1b[38;2;215;119;87m████████\x1b[0m";
        assert_eq!(Mascot::find(screen(bar).screen()), None);
    }

    #[test]
    fn encodes_two_color_cells() {
        let o = Some((236, 160, 82));
        let e = Some((18, 18, 22));
        let c = encode_cell([o, o, e, e]);
        assert_eq!(c.ch, '▀');
        assert_eq!((c.fg, c.bg), (o, e));
        let c = encode_cell([None, o, None, o]);
        assert_eq!((c.ch, c.fg, c.bg), ('▐', o, None));
    }

    #[test]
    fn animated_frames_loop_by_duration() {
        let spec: MascotSpec = serde_json::from_str(
            r##"{"colors": {"A": "#ff0000", "B": "#0000ff"}, "frames": [
                {"pixels": ["AAAAAAAAAAAAAAAAAA","AAAAAAAAAAAAAAAAAA","..................",
                            "..................","..................",".................."], "ms": 100},
                {"pixels": ["BBBBBBBBBBBBBBBBBB","BBBBBBBBBBBBBBBBBB","..................",
                            "..................","..................",".................."], "ms": 300}
            ]}"##,
        )
        .unwrap();
        let m = Mascot::load(&spec, Color::Reset).unwrap().unwrap();
        assert!(m.is_animated());
        let red = Some((255, 0, 0));
        let blue = Some((0, 0, 255));
        assert_eq!(m.frame_at(0)[0][0].fg, red);
        assert_eq!(m.frame_at(99)[0][0].fg, red);
        assert_eq!(m.frame_at(100)[0][0].fg, blue);
        assert_eq!(m.frame_at(399)[0][0].fg, blue);
        assert_eq!(m.frame_at(400)[0][0].fg, red);
    }

    #[test]
    fn presets_are_valid() {
        for (name, _) in PRESETS {
            let spec = MascotSpec::Name(name.to_string());
            assert!(Mascot::load(&spec, Color::Rgb(1, 2, 3)).unwrap().is_some(), "{name}");
        }
    }
}
