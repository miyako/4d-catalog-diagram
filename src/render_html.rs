//! Single-file interactive HTML renderer.
//!
//! Everything — CSS, JS, geometry — is inlined. There is no CDN reference, no
//! web font fetch, and no framework: pan/zoom/search/highlight are implemented
//! directly against the SVG DOM in vanilla JS (a few KB), per BUILD-SPEC §4.3.
//!
//! With JavaScript disabled the document still shows the fully rendered,
//! legible (non-interactive) diagram, because the SVG is generated server-side
//! rather than built in the browser.

use std::fmt::Write as _;

use crate::render_svg::{self, esc_attr, esc_text, Theme};
use crate::scene::Scene;

pub const JS: &str = include_str!("../assets/app.js");
pub const CSS: &str = include_str!("../assets/app.css");

/// A pre-built layout/system-field variant the in-page toggles can switch to
/// without re-running the CLI.
pub struct Variant {
    pub layout: &'static str,
    pub hide_system_fields: bool,
    pub scene: Scene,
}

pub struct HtmlOptions {
    pub theme: Theme,
    /// The variant shown on load, and the one the no-JS fallback displays.
    pub default_layout: &'static str,
    pub default_hide_system_fields: bool,
    pub generated_at: Option<String>,
    pub command: String,
}

pub fn render(variants: &[Variant], options: &HtmlOptions) -> String {
    let default = variants
        .iter()
        .find(|v| {
            v.layout == options.default_layout
                && v.hide_system_fields == options.default_hide_system_fields
        })
        .or_else(|| variants.first())
        .expect("at least one variant");

    let colors = render_svg::edge_colors(variants.iter().map(|v| &v.scene));
    let title = &default.scene.title;

    let theme_attr = match options.theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
        Theme::Auto => "auto",
    };

    let mut out = String::with_capacity(64 * 1024);
    out.push_str("<!DOCTYPE html>\n");
    let _ = write!(
        out,
        r#"<html lang="en" data-theme="{theme}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title}</title>"#,
        theme = theme_attr,
        title = esc_text(title),
    );
    let _ = write!(out, "<style>{}\n{}</style>", CSS, render_svg::shared_css());
    out.push_str("</head><body>");

    out.push_str(&sidebar(default, variants, options));

    out.push_str(r#"<main id="stage">"#);
    let _ = write!(
        out,
        r#"<svg id="canvas" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="{label}">"#,
        w = default.scene.width,
        h = default.scene.height,
        label = esc_attr(&format!("Entity relationship diagram for {title}")),
    );
    out.push_str(&render_svg::defs(&colors));
    out.push_str(r#"<rect id="canvas-bg" x="0" y="0" width="100%" height="100%"/>"#);
    out.push_str(r#"<g id="viewport">"#);
    for variant in variants {
        let active = std::ptr::eq(variant, default);
        let _ = write!(
            out,
            r#"<g class="scene{extra}" data-layout="{layout}" data-hide="{hide}" data-width="{w}" data-height="{h}">"#,
            extra = if active { " scene-active" } else { "" },
            layout = esc_attr(variant.layout),
            hide = u8::from(variant.hide_system_fields),
            w = variant.scene.width,
            h = variant.scene.height,
        );
        out.push_str(&render_svg::body(&variant.scene));
        out.push_str("</g>");
    }
    out.push_str("</g></svg>");

    out.push_str(&toolbar());
    // Only useful once a diagram is too big to take in at a glance; the script
    // populates and unhides it when the active scene is dense enough.
    out.push_str(
        r#"<svg id="minimap" width="180" height="120" viewBox="0 0 180 120" aria-hidden="true" hidden><g id="minimap-cards"></g><rect id="minimap-view" class="mm-view" x="0" y="0" width="0" height="0"/></svg>"#,
    );
    out.push_str(r#"<div id="popup" hidden></div>"#);
    out.push_str("</main>");

    let _ = write!(
        out,
        r#"<script id="diagram-data" type="application/json">{}</script>"#,
        // Serialized into a <script> block, so `<` and `&` must be neutralised
        // to make a `</script>` breakout impossible.
        json_for_script(&serde_json::json!({
            "title": title,
            "command": options.command,
            "generated_at": options.generated_at,
            "default": {
                "layout": options.default_layout,
                "hide_system_fields": options.default_hide_system_fields,
            },
            "theme": theme_attr,
            "css": {
                "light": render_svg::static_css(&render_svg::LIGHT),
                "dark": render_svg::static_css(&render_svg::DARK),
                "shared": render_svg::shared_css(),
            },
            "variants": variants.iter().map(|v| serde_json::json!({
                "layout": v.layout,
                "hide_system_fields": v.hide_system_fields,
                "width": v.scene.width,
                "height": v.scene.height,
                "tables": v.scene.cards.iter().map(|c| serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "ghost": c.ghost,
                    "field_count": c.field_count,
                    "relation_count": c.relation_count,
                    "primary_key": c.primary_key,
                    "x": c.x, "y": c.y, "w": c.w, "h": c.h,
                })).collect::<Vec<_>>(),
                "relations": v.scene.edges.iter().map(|e| serde_json::json!({
                    "id": e.id,
                    "from_table": e.from_table,
                    "from_field": e.from_field,
                    "to_table": e.to_table,
                    "to_field": e.to_field,
                    "name_Nto1": e.name_nto1,
                    "name_1toN": e.name_1ton,
                    "auto_load_Nto1": e.auto_load_nto1,
                    "auto_load_1toN": e.auto_load_1ton,
                    "integrity": e.integrity,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }))
    );
    let _ = write!(out, "<script>{JS}</script>");
    out.push_str("</body></html>");
    out
}

/// `serde_json` already escapes quotes and backslashes; the remaining risk in an
/// inline `<script>` is a literal `</script>` (or `<!--`) inside a string, so
/// escape `<`, `>` and `&` as unicode escapes, which are valid inside JSON
/// strings and parse back to the original characters.
fn json_for_script(value: &serde_json::Value) -> String {
    serde_json::to_string(value)
        .expect("diagram data is serializable")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

fn sidebar(default: &Variant, variants: &[Variant], options: &HtmlOptions) -> String {
    let scene = &default.scene;
    let table_count = scene.cards.iter().filter(|c| !c.ghost).count();
    let ghost_count = scene.cards.len() - table_count;

    let mut out = String::new();
    out.push_str(r#"<aside id="sidebar">"#);
    let _ = write!(
        out,
        r#"<header><h1>{title}</h1><p class="counts">{tables} table{ts} · {relations} relation{rs}{ghosts}</p></header>"#,
        title = esc_text(&scene.title),
        tables = table_count,
        ts = if table_count == 1 { "" } else { "s" },
        relations = scene.edges.len(),
        rs = if scene.edges.len() == 1 { "" } else { "s" },
        ghosts = if ghost_count > 0 {
            format!(" · {ghost_count} external")
        } else {
            String::new()
        },
    );

    out.push_str(
        r#"<div class="search"><input id="search" type="search" placeholder="Filter tables…" autocomplete="off" spellcheck="false"></div>"#,
    );

    out.push_str(r#"<ul id="table-list">"#);
    let mut sorted: Vec<_> = scene.cards.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for card in sorted {
        let _ = write!(
            out,
            r#"<li><button type="button" data-target="{name}" class="table-item{ghost}"><span class="swatch" style="background:{accent}"></span><span class="label">{label}</span><span class="count">{count}</span></button></li>"#,
            name = esc_attr(&card.name),
            ghost = if card.ghost { " is-ghost" } else { "" },
            accent = esc_attr(&card.accent),
            label = esc_text(&card.name),
            count = if card.ghost {
                "ext".to_string()
            } else {
                card.field_count.to_string()
            },
        );
    }
    out.push_str("</ul>");

    let has_hidden_variant = variants.iter().any(|v| v.hide_system_fields)
        && variants.iter().any(|v| !v.hide_system_fields);
    let has_both_layouts = variants.iter().any(|v| v.layout == "auto")
        && variants.iter().any(|v| v.layout == "as-designed");

    out.push_str(r#"<div class="options">"#);
    let _ = write!(
        out,
        r#"<label class="toggle{disabled}"><input type="checkbox" id="toggle-system"{attr}> Hide 4D-internal fields</label>"#,
        disabled = if has_hidden_variant {
            ""
        } else {
            " is-disabled"
        },
        attr = if has_hidden_variant {
            if options.default_hide_system_fields {
                " checked"
            } else {
                ""
            }
        } else {
            " disabled"
        },
    );
    let _ = write!(
        out,
        r#"<label class="toggle{disabled}"><input type="checkbox" id="toggle-layout"{attr}> Automatic layout</label>"#,
        disabled = if has_both_layouts { "" } else { " is-disabled" },
        attr = if has_both_layouts {
            if options.default_layout == "auto" {
                " checked"
            } else {
                ""
            }
        } else {
            " disabled"
        },
    );
    out.push_str(
        r#"<label class="toggle"><input type="checkbox" id="toggle-theme"> Dark theme</label>"#,
    );
    out.push_str("</div>");

    out.push_str(r#"<footer><div class="exports"><button type="button" id="export-svg">Export SVG</button><button type="button" id="export-png">Export PNG</button></div>"#);
    if let Some(stamp) = &options.generated_at {
        let _ = write!(out, r#"<p class="stamp">{}</p>"#, esc_text(stamp));
    }
    out.push_str("</footer></aside>");
    out
}

fn toolbar() -> String {
    r#"<div id="toolbar" role="group" aria-label="Zoom controls"><button type="button" id="zoom-out" title="Zoom out">−</button><button type="button" id="zoom-fit" title="Fit to screen">Fit</button><button type="button" id="zoom-in" title="Zoom in">+</button></div>"#
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_payload_cannot_break_out() {
        let value = serde_json::json!({ "name": "</script><img src=x onerror=alert(1)>" });
        let encoded = json_for_script(&value);
        assert!(!encoded.contains('<'));
        assert!(!encoded.contains('>'));
    }
}
