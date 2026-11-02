use ratatui::style::Color;

/// Hues (degrees) of Claude Code's orange brand color and its shimmer.
/// Errors (~350°) and warnings (~45°) fall outside and keep their color.
const WARM_HUES: std::ops::RangeInclusive<f32> = 8.0..=38.0;
const CLAUDE_HUE: f32 = 15.0;

#[derive(Clone, Copy)]
pub struct Theme {
    /// Hue that Claude's orange is rotated to; `None` keeps original colors.
    tint: Option<f32>,
    pub accent: Color,
}

impl Theme {
    /// Parses a tint name ("blue", "purple", ...), a hue in degrees, or "none".
    pub fn new(tint: &str) -> Self {
        let hue = match tint.trim().to_ascii_lowercase().as_str() {
            "" | "none" | "off" | "orange" => None,
            "blue" => Some(215.0),
            "sky" => Some(200.0),
            "indigo" => Some(235.0),
            "purple" => Some(270.0),
            "pink" => Some(330.0),
            "teal" => Some(175.0),
            "green" => Some(140.0),
            other => other.parse::<f32>().ok().map(|h| h.rem_euclid(360.0)),
        };
        let (r, g, b) = hsl_to_rgb(hue.unwrap_or(CLAUDE_HUE), 0.70, 0.64);
        Self {
            tint: hue,
            accent: Color::Rgb(r, g, b),
        }
    }

    pub fn map(&self, r: u8, g: u8, b: u8) -> (u8, u8, u8) {
        let Some(target) = self.tint else {
            return (r, g, b);
        };
        if !is_claude_orange(r, g, b) {
            return (r, g, b);
        }
        let (h, s, l) = rgb_to_hsl(r, g, b);
        hsl_to_rgb(target + (h - CLAUDE_HUE), s, l)
    }

    /// Converts an emulator color to a ratatui color, applying the tint.
    pub fn color(&self, c: vt100::Color) -> Color {
        match c {
            vt100::Color::Default => Color::Reset,
            vt100::Color::Idx(i) => Color::Indexed(i),
            vt100::Color::Rgb(r, g, b) => {
                let (r, g, b) = self.map(r, g, b);
                Color::Rgb(r, g, b)
            }
        }
    }
}

/// True for Claude's brand orange and nearby warm shades.
pub fn is_claude_orange(r: u8, g: u8, b: u8) -> bool {
    let (h, s, _) = rgb_to_hsl(r, g, b);
    s >= 0.25 && WARM_HUES.contains(&h)
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_claude_orange_only() {
        let t = Theme::new("blue");
        let (r, g, b) = t.map(215, 119, 87);
        let (h, _, _) = rgb_to_hsl(r, g, b);
        assert!((h - 215.0).abs() < 2.0, "hue {h}");
        assert!(b > r && b > g);
        // Error red, warning yellow, grays and greens are untouched.
        for c in [(255, 107, 128), (255, 193, 7), (153, 153, 153), (78, 186, 101)] {
            assert_eq!(t.map(c.0, c.1, c.2), c);
        }
        assert_eq!(Theme::new("none").map(215, 119, 87), (215, 119, 87));
    }
}
