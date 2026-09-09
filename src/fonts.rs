//! Embedded fonts and text measurement.
//!
//! Fonts are compiled into the binary (`include_bytes!`) so the tool never
//! touches system fontconfig, a font cache, or the network. The same metrics
//! drive card sizing in the layout stage and glyph rasterization in the PNG
//! path, which keeps computed card widths honest.

use std::sync::OnceLock;

pub const SANS: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
pub const MONO: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// Font stacks declared in generated SVG/HTML. Output stays tiny by relying on
/// the viewer's own fonts; the embedded faces above are only used for our own
/// measurement and for headless rasterization.
pub const SANS_STACK: &str =
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Helvetica Neue', Arial, 'DejaVu Sans', sans-serif";
pub const MONO_STACK: &str =
    "ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace";

struct Metrics {
    advances: Vec<u16>,
    units_per_em: f64,
    fallback: f64,
}

impl Metrics {
    fn build(data: &'static [u8]) -> Metrics {
        let face = ttf_parser::Face::parse(data, 0).expect("embedded font is valid");
        let units_per_em = face.units_per_em() as f64;
        // Cache Latin-1, which covers the overwhelming majority of catalog
        // identifiers; anything outside it falls back to an average advance.
        let mut advances = vec![0u16; 256];
        for code in 0u32..256 {
            let ch = char::from_u32(code).unwrap();
            advances[code as usize] = face
                .glyph_index(ch)
                .and_then(|gid| face.glyph_hor_advance(gid))
                .unwrap_or(0);
        }
        let fallback = face
            .glyph_index('M')
            .and_then(|gid| face.glyph_hor_advance(gid))
            .unwrap_or(units_per_em as u16) as f64;
        Metrics {
            advances,
            units_per_em,
            fallback,
        }
    }

    fn width(&self, text: &str, size: f64) -> f64 {
        let mut units = 0.0;
        for ch in text.chars() {
            let advance = if (ch as u32) < 256 {
                self.advances[ch as usize] as f64
            } else {
                self.fallback
            };
            units += advance;
        }
        units / self.units_per_em * size
    }
}

fn sans() -> &'static Metrics {
    static M: OnceLock<Metrics> = OnceLock::new();
    M.get_or_init(|| Metrics::build(SANS))
}

fn mono() -> &'static Metrics {
    static M: OnceLock<Metrics> = OnceLock::new();
    M.get_or_init(|| Metrics::build(MONO))
}

/// Advance width of `text` in pixels at `size` px, in the proportional face.
pub fn measure_sans(text: &str, size: f64) -> f64 {
    sans().width(text, size)
}

/// Advance width of `text` in pixels at `size` px, in the monospaced face.
pub fn measure_mono(text: &str, size: f64) -> f64 {
    mono().width(text, size)
}

/// Bold text is rendered with synthetic or real bold depending on the viewer;
/// budget a small extra allowance so headers never clip.
pub fn measure_sans_bold(text: &str, size: f64) -> f64 {
    sans().width(text, size) * 1.06
}

/// Truncate `text` so it fits `max_width` px, appending an ellipsis when cut.
pub fn ellipsize(text: &str, size: f64, max_width: f64, monospaced: bool) -> String {
    let measure = |s: &str| {
        if monospaced {
            measure_mono(s, size)
        } else {
            measure_sans(s, size)
        }
    };
    if measure(text) <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let budget = max_width - measure(ellipsis);
    if budget <= 0.0 {
        return ellipsis.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let mut candidate = out.clone();
        candidate.push(ch);
        if measure(&candidate) > budget {
            break;
        }
        out = candidate;
    }
    out.push_str(ellipsis);
    out
}

/// A `fontdb` database preloaded with the embedded faces, for rasterization.
pub fn font_database() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    db.load_font_data(SANS.to_vec());
    db.load_font_data(MONO.to_vec());
    db.set_sans_serif_family("DejaVu Sans");
    db.set_monospace_family("DejaVu Sans Mono");
    db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_is_monotonic_and_positive() {
        assert!(measure_sans("A", 12.0) > 0.0);
        assert!(measure_sans("AA", 12.0) > measure_sans("A", 12.0));
        assert!(measure_sans("A", 24.0) > measure_sans("A", 12.0));
    }

    #[test]
    fn mono_is_uniform_width() {
        let a = measure_mono("i", 12.0);
        let b = measure_mono("W", 12.0);
        assert!((a - b).abs() < 0.001);
    }

    #[test]
    fn ellipsize_respects_budget() {
        let out = ellipsize("VERY_LONG_FIELD_NAME_INDEED", 12.0, 40.0, false);
        assert!(out.ends_with('…'));
        assert!(measure_sans(&out, 12.0) <= 40.0);
    }

    #[test]
    fn ellipsize_leaves_short_text_alone() {
        assert_eq!(ellipsize("ID", 12.0, 200.0, false), "ID");
    }
}
