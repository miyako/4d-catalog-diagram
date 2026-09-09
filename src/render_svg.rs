//! Standalone SVG renderer.
//!
//! The same body is reused by the HTML renderer, which wraps it in a viewport
//! and layers interactivity on top. Every string that came from the catalog is
//! escaped here with XML rules before it reaches the output.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::fonts;
use crate::scene::{Badge, Card, Edge, Row, Scene, HEADER_H, ROW_H};
use crate::scene::{LABEL_SIZE, META_SIZE, NAME_SIZE, PAD_X, TITLE_SIZE, TYPE_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    Auto,
}

impl Theme {
    pub fn parse(value: &str) -> Option<Theme> {
        match value {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            "auto" => Some(Theme::Auto),
            _ => None,
        }
    }
}

/// Escape a string for an XML/SVG/HTML *text node*.
pub fn esc_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // Control characters are not legal in XML 1.0 and some real exports
            // carry stray ones; drop them rather than emit invalid markup.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Escape a string for an XML/SVG/HTML *attribute value* (always double-quoted).
pub fn esc_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

fn n(value: f64) -> String {
    let rounded = crate::layout::round2(value);
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}

/// Palette resolved per theme. The HTML renderer emits both palettes as CSS
/// custom properties instead.
pub struct Palette {
    pub bg: &'static str,
    pub card: &'static str,
    pub card_border: &'static str,
    pub row_alt: &'static str,
    pub text: &'static str,
    pub text_muted: &'static str,
    pub badge_bg: &'static str,
    pub badge_text: &'static str,
    pub edge_label_bg: &'static str,
    pub dark: bool,
}

pub const LIGHT: Palette = Palette {
    bg: "#f6f7fb",
    card: "#ffffff",
    card_border: "#dbe1ea",
    row_alt: "#f8fafc",
    text: "#0f172a",
    text_muted: "#64748b",
    badge_bg: "#eef2f7",
    badge_text: "#475569",
    edge_label_bg: "#f6f7fb",
    dark: false,
};

pub const DARK: Palette = Palette {
    bg: "#0b1020",
    card: "#151b2e",
    card_border: "#26304a",
    row_alt: "#1a2135",
    text: "#e6ecf7",
    text_muted: "#94a3b8",
    badge_bg: "#232c45",
    badge_text: "#a9b6cc",
    edge_label_bg: "#0b1020",
    dark: true,
};

pub fn palette(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Dark => &DARK,
        // `auto` has no meaning for a static image; light is the export default.
        _ => &LIGHT,
    }
}

/// A complete, standalone SVG document.
pub fn render(scene: &Scene, theme: Theme) -> String {
    let palette = palette(theme);
    let mut out = String::with_capacity(16 * 1024);

    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img" aria-label="{label}">"#,
        w = n(scene.width),
        h = n(scene.height),
        label = esc_attr(&format!("Entity relationship diagram for {}", scene.title)),
    );
    let _ = write!(out, "<title>{}</title>", esc_text(&scene.title));
    let _ = write!(out, "<style>{}</style>", static_css(palette));
    let _ = write!(out, "{}", defs(&edge_colors([scene])));
    let _ = write!(
        out,
        r#"<rect class="bg" x="0" y="0" width="{}" height="{}"/>"#,
        n(scene.width),
        n(scene.height)
    );
    let _ = write!(out, "{}", body(scene));
    out.push_str("</svg>");
    out
}

/// Shared `<defs>`: the drop shadow plus one marker pair per distinct relation
/// colour, so each edge's cardinality glyphs carry its own colour without
/// relying on `context-stroke` (not universally supported).
pub fn defs(colors: &BTreeSet<String>) -> String {
    let mut out = String::from("<defs>");
    out.push_str(
        r#"<filter id="cardShadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="2" stdDeviation="3" flood-opacity="0.16"/></filter>"#,
    );
    for color in colors {
        let key = color_key(color);
        let color = esc_attr(color);
        // Crow's foot at the "many" end: prongs sit on the card, apex points out.
        let _ = write!(
            out,
            r#"<marker id="mN-{key}" markerWidth="12" markerHeight="12" refX="0" refY="6" orient="auto" markerUnits="userSpaceOnUse"><path d="M10 6L0 1M10 6L0 6M10 6L0 11" fill="none" stroke="{color}" stroke-width="1.4"/></marker>"#,
        );
        // A single bar at the "one" end.
        let _ = write!(
            out,
            r#"<marker id="m1-{key}" markerWidth="10" markerHeight="12" refX="1" refY="6" orient="auto" markerUnits="userSpaceOnUse"><path d="M1 1.5L1 10.5" fill="none" stroke="{color}" stroke-width="1.6"/></marker>"#,
        );
    }
    out.push_str("</defs>");
    out
}

/// Marker ids are derived from the colour so identical colours share markers
/// and several scenes can coexist in one document without id collisions.
pub fn color_key(color: &str) -> String {
    color
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Every distinct relation colour used by `scenes`.
pub fn edge_colors<'a>(scenes: impl IntoIterator<Item = &'a Scene>) -> BTreeSet<String> {
    let mut colors = BTreeSet::new();
    for scene in scenes {
        for edge in &scene.edges {
            colors.insert(edge.color.clone());
        }
    }
    colors
}

/// The diagram itself (edges + cards), without the document wrapper. Shared
/// verbatim with the HTML renderer.
pub fn body(scene: &Scene) -> String {
    let mut out = String::with_capacity(16 * 1024);

    let _ = write!(
        out,
        r#"<text class="diagram-title" x="{}" y="{}">{}</text>"#,
        n(20.0),
        n(28.0),
        esc_text(&scene.title)
    );

    out.push_str(r#"<g class="edges">"#);
    for edge in &scene.edges {
        out.push_str(&render_edge(edge));
    }
    out.push_str("</g>");

    out.push_str(r#"<g class="cards">"#);
    for card in &scene.cards {
        out.push_str(&render_card(card));
    }
    out.push_str("</g>");

    out
}

fn render_edge(edge: &Edge) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<g class="edge" data-edge="{id}" data-from="{from}" data-to="{to}">"#,
        id = esc_attr(&edge.id),
        from = esc_attr(&edge.from_table),
        to = esc_attr(&edge.to_table),
    );
    let _ = write!(
        out,
        r#"<title>{}</title>"#,
        esc_text(&format!(
            "{}.{} → {}.{}",
            edge.from_table, edge.from_field, edge.to_table, edge.to_field
        ))
    );
    // A transparent, fat copy of the path widens the click/hover target.
    let _ = write!(
        out,
        r#"<path class="edge-hit" d="{}"/>"#,
        esc_attr(&edge.path)
    );
    let _ = write!(
        out,
        r#"<path class="edge-line" d="{d}" stroke="{color}" marker-start="url(#mN-{key})" marker-end="url(#m1-{key})"/>"#,
        d = esc_attr(&edge.path),
        color = esc_attr(&edge.color),
        key = color_key(&edge.color),
    );

    let label = |text: &Option<String>, point: [f64; 2], anchor_left: bool| -> String {
        let Some(text) = text else {
            return String::new();
        };
        let anchor = if anchor_left { "end" } else { "start" };
        format!(
            r#"<text class="edge-label" x="{x}" y="{y}" text-anchor="{anchor}">{t}</text>"#,
            x = n(point[0]),
            y = n(point[1]),
            t = esc_text(&fonts::ellipsize(text, LABEL_SIZE, 150.0, false)),
        )
    };
    out.push_str(&label(
        &edge.name_nto1,
        edge.from_label,
        edge.from_anchor_left,
    ));
    out.push_str(&label(&edge.name_1ton, edge.to_label, edge.to_anchor_left));
    out.push_str("</g>");
    out
}

fn render_card(card: &Card) -> String {
    let mut out = String::new();
    let class = if card.ghost { "card ghost" } else { "card" };
    let _ = write!(
        out,
        r#"<g class="{class}" data-card="{id}" data-table="{name}" transform="translate({x} {y})">"#,
        id = esc_attr(&card.id),
        name = esc_attr(&card.name),
        x = n(card.x),
        y = n(card.y),
    );

    if card.ghost {
        let _ = write!(
            out,
            r#"<title>{}</title>"#,
            esc_text(&format!("{} — outside the current selection", card.name))
        );
        let _ = write!(
            out,
            r#"<rect class="card-bg ghost-bg" x="0" y="0" width="{w}" height="{h}" rx="9"/>"#,
            w = n(card.w),
            h = n(card.h),
        );
        let _ = write!(
            out,
            r#"<text class="ghost-name" x="{x}" y="{y}">{t}</text>"#,
            x = n(PAD_X),
            y = n(card.h / 2.0 + 4.0),
            t = esc_text(&card.display_name),
        );
        out.push_str("</g>");
        return out;
    }

    let _ = write!(out, r#"<title>{}</title>"#, esc_text(&card_tooltip(card)));
    let _ = write!(
        out,
        r#"<rect class="card-bg" x="0" y="0" width="{w}" height="{h}" rx="9" filter="url(#cardShadow)"/>"#,
        w = n(card.w),
        h = n(card.h),
    );
    // The header is a rounded rect clipped to the top by a second square-cornered
    // rect, which avoids needing a clipPath per card.
    let _ = write!(
        out,
        r#"<path class="card-header" d="M0 9a9 9 0 0 1 9-9h{inner}a9 9 0 0 1 9 9v{rest}H0Z" fill="{accent}"/>"#,
        inner = n(card.w - 18.0),
        rest = n(HEADER_H - 9.0),
        accent = esc_attr(&card.accent),
    );
    // The header colour comes from the catalog, so the readable foreground is
    // decided per card and written inline rather than via CSS — this keeps it
    // correct in minimal SVG renderers that do not support compound selectors.
    let header_fg = if card.dark_text { "#0f172a" } else { "#ffffff" };
    let _ = write!(
        out,
        r#"<text class="card-title" x="{x}" y="{y}" fill="{fg}">{t}</text>"#,
        x = n(PAD_X),
        y = n(22.0),
        fg = header_fg,
        t = esc_text(&card.display_name),
    );
    let meta = format!(
        "{} field{}",
        card.field_count,
        if card.field_count == 1 { "" } else { "s" }
    );
    let _ = write!(
        out,
        r#"<text class="card-meta" x="{x}" y="{y}" text-anchor="end" fill="{fg}">{t}</text>"#,
        x = n(card.w - PAD_X),
        y = n(22.0),
        fg = header_fg,
        t = esc_text(&meta),
    );

    // When the card is collapsed, the rows past the cut are still emitted (so
    // the static export and the DOM stay complete) but live in a group that CSS
    // hides. Expanding draws over the neighbours rather than reflowing, which
    // keeps the card box — and therefore every edge anchor — exactly as laid
    // out.
    let cut = card.collapsed_rows.unwrap_or(card.rows.len());
    if card.collapsed_rows.is_some() {
        let _ = write!(
            out,
            r#"<rect class="card-expanded-bg" x="0" y="0" width="{w}" height="{h}" rx="9"/>"#,
            w = n(card.w),
            h = n(card.expanded_h),
        );
    }
    for (index, row) in card.rows.iter().take(cut).enumerate() {
        out.push_str(&render_row(card, row, index));
    }
    if card.collapsed_rows.is_some() {
        out.push_str(r#"<g class="card-extra-rows">"#);
        for (index, row) in card.rows.iter().enumerate().skip(cut) {
            out.push_str(&render_row(card, row, index));
        }
        out.push_str("</g>");
        out.push_str(&render_more_toggle(card, cut));
    }

    let _ = write!(
        out,
        r#"<rect class="card-outline" x="0.5" y="0.5" width="{w}" height="{h}" rx="9"/>"#,
        w = n(card.w - 1.0),
        h = n(card.h - 1.0),
    );
    if card.collapsed_rows.is_some() {
        let _ = write!(
            out,
            r#"<rect class="card-outline card-expanded-outline" x="0.5" y="0.5" width="{w}" height="{h}" rx="9"/>"#,
            w = n(card.w - 1.0),
            h = n(card.expanded_h - 1.0),
        );
    }

    out.push_str("</g>");
    out
}

/// The "Show N more" / "Show less" affordance on a collapsed card. Both labels
/// are pre-rendered and swapped with CSS so expanding needs no text measurement
/// in the browser.
fn render_more_toggle(card: &Card, cut: usize) -> String {
    let hidden = card.rows.len().saturating_sub(cut);
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<g class="row-more" data-more="{hidden}" transform="translate(0 {y})"><title>{title}</title>"#,
        y = n(HEADER_H + cut as f64 * ROW_H),
        title = esc_text(&format!("{hidden} more field(s)")),
    );
    let _ = write!(
        out,
        r#"<rect class="row-more-bg" x="1" y="0" width="{w}" height="{h}"/>"#,
        w = n(card.w - 2.0),
        h = n(ROW_H),
    );
    let _ = write!(
        out,
        r#"<text class="row-more-label row-more-open" x="{x}" y="{y}" text-anchor="middle">{t}</text>"#,
        x = n(card.w / 2.0),
        y = n(ROW_H / 2.0 + 4.0),
        t = esc_text(&format!("▾ Show {hidden} more")),
    );
    let _ = write!(
        out,
        r#"<text class="row-more-label row-more-close" x="{x}" y="{y}" text-anchor="middle">▴ Show less</text>"#,
        x = n(card.w / 2.0),
        y = n(ROW_H / 2.0 + 4.0),
    );
    out.push_str("</g>");
    out
}

fn card_tooltip(card: &Card) -> String {
    let mut parts = vec![card.name.clone()];
    if let Some(pk) = &card.primary_key {
        parts.push(format!("primary key: {pk}"));
    }
    parts.push(format!("{} fields", card.field_count));
    if card.hidden_field_count > 0 {
        parts.push(format!("{} hidden", card.hidden_field_count));
    }
    parts.push(format!("{} relations", card.relation_count));
    parts.join(" · ")
}

fn render_row(card: &Card, row: &Row, index: usize) -> String {
    let mut out = String::new();
    let mut class = String::from("row");
    if row.hidden {
        class.push_str(" row-hidden");
    }
    if index % 2 == 1 {
        class.push_str(" row-alt");
    }
    let _ = write!(
        out,
        r#"<g class="{class}" data-field="{field}" transform="translate(0 {y})">"#,
        field = esc_attr(&row.name),
        y = n(row.y),
    );

    if let Some(tip) = &row.tip {
        let _ = write!(out, "<title>{}</title>", esc_text(tip));
    }

    let _ = write!(
        out,
        r#"<rect class="row-bg" x="1" y="0" width="{w}" height="{h}"/>"#,
        w = n(card.w - 2.0),
        h = n(ROW_H),
    );

    out.push_str(&icon(row.glyph, PAD_X, ROW_H / 2.0 - 5.0, &row.type_color));

    let _ = write!(
        out,
        r#"<text class="field-name" x="{x}" y="{y}">{t}</text>"#,
        x = n(PAD_X + 12.0 + 6.0),
        y = n(ROW_H / 2.0 + 4.0),
        t = esc_text(&row.display_name),
    );

    let badge_span = if row.badges.is_empty() {
        0.0
    } else {
        row.badges.len() as f64 * 16.0
    };
    let _ = write!(
        out,
        r#"<text class="field-type" x="{x}" y="{y}" text-anchor="end" fill="{color}">{t}</text>"#,
        x = n(card.w - PAD_X - badge_span),
        y = n(ROW_H / 2.0 + 3.5),
        color = esc_attr(&row.type_color),
        t = esc_text(&row.type_label),
    );

    for (i, badge) in row.badges.iter().enumerate() {
        let x = card.w - PAD_X - badge_span + i as f64 * 16.0;
        out.push_str(&render_badge(*badge, x, ROW_H / 2.0 - 6.5));
    }

    out.push_str("</g>");
    out
}

fn render_badge(badge: Badge, x: f64, y: f64) -> String {
    format!(
        r#"<g class="badge badge-{kind}"><title>{title}</title><rect x="{x}" y="{y}" width="13" height="13" rx="3.5"/><text x="{tx}" y="{ty}" text-anchor="middle">{letter}</text></g>"#,
        kind = badge.letter().to_lowercase(),
        title = esc_text(badge.title()),
        x = n(x),
        y = n(y),
        tx = n(x + 6.5),
        ty = n(y + 9.7),
        letter = badge.letter(),
    )
}

/// Small inline vector icons, drawn in a 10×10 box at `(x, y)`. Redrawn as
/// geometry rather than shipping the legacy bitmap field icons, so the output
/// has zero external asset dependencies.
fn icon(glyph: &str, x: f64, y: f64, color: &str) -> String {
    let color = esc_attr(color);
    let strokes: &str = match glyph {
        "boolean" => "M2 5.4L4.1 7.6L8.2 2.9",
        "number" => "M3.4 1.6L2.4 8.4M7.2 1.6L6.2 8.4M1.4 3.6H8.2M1.1 6.4H7.9",
        "date" => "M1.4 2.6H8.6M1.4 2.6V8.4H8.6V2.6M3.2 1.2V3.6M6.8 1.2V3.6",
        "time" => "M5 1.4A3.6 3.6 0 1 1 4.99 1.4M5 2.9V5.2L6.7 6.4",
        "text" => "M1.4 2.6H8.6M1.4 5H8.6M1.4 7.4H6.2",
        "blob" => "M1.6 2.4H8.4M1.6 5H8.4M1.6 7.6H8.4M3 1.2V8.8",
        "image" => "M1.2 1.6H8.8V8.4H1.2ZM1.2 6.6L3.6 4.2L5.6 6.2L7 5L8.8 6.8",
        _ => "M5 1.4A3.6 3.6 0 1 1 4.99 1.4M5 7.6V7.61M3.6 4.1A1.4 1.4 0 1 1 5 5.6V6.2",
    };
    format!(
        r#"<g class="icon" transform="translate({x} {y})"><path d="{strokes}" fill="none" stroke="{color}" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></g>"#,
        x = n(x),
        y = n(y),
    )
}

/// CSS for a standalone SVG (single, resolved theme).
pub fn static_css(palette: &Palette) -> String {
    format!(
        "{shared}\
        .bg{{fill:{bg}}}\
        .card-bg{{fill:{card}}}\
        .card-outline{{fill:none;stroke:{border}}}\
        .row-alt .row-bg{{fill:{row_alt}}}\
        .diagram-title{{fill:{text}}}\
        .field-name{{fill:{text}}}\
        .ghost-name{{fill:{muted}}}\
        .badge rect{{fill:{badge_bg}}}\
        .badge text{{fill:{badge_text}}}\
        .edge-label{{fill:{muted}}}\
        .ghost-bg{{fill:{card};stroke:{border};stroke-dasharray:5 4}}\
        .card-expanded-bg{{fill:none;display:none}}\
        .card-expanded-outline{{fill:none;stroke:{border};display:none}}\
        .row-more-bg{{fill:{row_alt}}}\
        .row-more-label{{fill:{muted}}}\
        .card-extra-rows{{display:none}}\
        .row-more-close{{display:none}}",
        shared = shared_css(),
        bg = palette.bg,
        card = palette.card,
        border = palette.card_border,
        row_alt = palette.row_alt,
        text = palette.text,
        muted = palette.text_muted,
        badge_bg = palette.badge_bg,
        badge_text = palette.badge_text,
    )
}

/// Geometry/typography rules shared by both output families.
pub fn shared_css() -> String {
    format!(
        "text{{font-family:{sans};dominant-baseline:auto}}\
        .diagram-title{{font-size:{title}px;font-weight:700;letter-spacing:.2px}}\
        .card-title{{font-size:{title}px;font-weight:700}}\
        .card-meta{{font-size:{meta}px}}\
        .row-more-label{{font-size:10.5px}}\
        .field-name{{font-size:{name}px}}\
        .ghost-name{{font-size:{title}px;font-weight:600}}\
        .field-type{{font-family:{mono};font-size:{type_size}px}}\
        .badge text{{font-family:{mono};font-size:8.5px;font-weight:700}}\
        .edge-label{{font-size:{label}px}}\
        .row-bg{{fill:transparent}}\
        .row-hidden{{opacity:.55}}\
        .edge-line{{fill:none;stroke-width:1.6}}\
        .edge-hit{{fill:none;stroke:transparent;stroke-width:14}}",
        sans = fonts::SANS_STACK,
        mono = fonts::MONO_STACK,
        title = TITLE_SIZE,
        meta = META_SIZE,
        name = NAME_SIZE,
        type_size = TYPE_SIZE,
        label = LABEL_SIZE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_escaping_neutralises_markup() {
        let out = esc_text("<script>alert(1)</script>");
        assert!(!out.contains('<'));
        assert!(!out.contains('>'));
    }

    #[test]
    fn attribute_escaping_neutralises_quote_breakouts() {
        let out = esc_attr(r#""><img src=x onerror=alert(1)>"#);
        assert!(!out.contains('"'));
        assert!(!out.contains('<'));
        assert!(!out.contains('>'));
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(esc_text("a\u{0}b"), "ab");
    }

    #[test]
    fn numbers_are_compact_and_stable() {
        assert_eq!(n(10.0), "10");
        assert_eq!(n(10.125), "10.13");
        assert_eq!(n(-0.001), "0");
    }
}
