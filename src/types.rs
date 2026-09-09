//! 4D field type codes.
//!
//! Kept as data (not scattered across the renderers) so the table is easy to
//! extend as the format evolves. Unknown codes never fail — they fall back to a
//! generic `Type {n}` label and a neutral colour.

/// `(code, label, accent colour, glyph family)`
///
/// The glyph family drives the small vector icon drawn in the field list; it is
/// intentionally coarse so unknown types can borrow the "unknown" shape.
pub const TYPES: &[(i64, &str, &str, Glyph)] = &[
    (1, "Boolean", "#8b5cf6", Glyph::Boolean),
    (2, "Byte", "#0ea5e9", Glyph::Number),
    (3, "Integer", "#0ea5e9", Glyph::Number),
    (4, "Longint", "#0284c7", Glyph::Number),
    (5, "Long 64", "#0284c7", Glyph::Number),
    (6, "Real", "#06b6d4", Glyph::Number),
    (7, "Float", "#06b6d4", Glyph::Number),
    (8, "Date", "#f59e0b", Glyph::Date),
    (9, "Time", "#f97316", Glyph::Time),
    (10, "Alpha", "#16a34a", Glyph::Text),
    (11, "Blob", "#64748b", Glyph::Blob),
    (12, "Image", "#db2777", Glyph::Image),
    (14, "Alpha", "#16a34a", Glyph::Text),
    (17, "Text", "#15803d", Glyph::Text),
    (18, "Blob", "#64748b", Glyph::Blob),
    (21, "Blob", "#64748b", Glyph::Blob),
];

pub const UNKNOWN_COLOR: &str = "#94a3b8";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Boolean,
    Number,
    Date,
    Time,
    Text,
    Blob,
    Image,
    Unknown,
}

pub fn type_label(code: i64) -> String {
    TYPES
        .iter()
        .find(|(c, ..)| *c == code)
        .map(|(_, label, ..)| (*label).to_string())
        .unwrap_or_else(|| format!("Type {code}"))
}

pub fn type_color(code: i64) -> &'static str {
    TYPES
        .iter()
        .find(|(c, ..)| *c == code)
        .map(|(_, _, color, _)| *color)
        .unwrap_or(UNKNOWN_COLOR)
}

pub fn type_glyph(code: i64) -> Glyph {
    TYPES
        .iter()
        .find(|(c, ..)| *c == code)
        .map(|(_, _, _, g)| *g)
        .unwrap_or(Glyph::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_codes_resolve() {
        assert_eq!(type_label(4), "Longint");
        assert_eq!(type_label(17), "Text");
    }

    #[test]
    fn unknown_codes_degrade_instead_of_failing() {
        assert_eq!(type_label(999), "Type 999");
        assert_eq!(type_color(999), UNKNOWN_COLOR);
        assert_eq!(type_glyph(999), Glyph::Unknown);
    }
}
