//! The geometric scene: card sizing, layout invocation, and edge routing.
//!
//! Both renderers consume a [`Scene`]; the HTML renderer additionally embeds
//! several pre-built scenes (one per layout × system-field combination) so its
//! toggles never need to re-run layout in the browser.

use serde::Serialize;

use crate::fonts;
use crate::layout::{self, LayoutMode, Size};
use crate::model::{Catalog, Color, Coordinates};
use crate::select::{Endpoint, Selection};
use crate::types;

pub const HEADER_H: f64 = 34.0;
pub const ROW_H: f64 = 20.0;
pub const PAD_X: f64 = 11.0;
pub const PAD_BOTTOM: f64 = 9.0;
pub const GHOST_H: f64 = 36.0;

/// A card with more than this many visible fields is rendered collapsed in the
/// interactive HTML output.
pub const COLLAPSE_MIN_FIELDS: usize = 10;
/// How many field rows a collapsed card keeps on screen. The row freed up by
/// collapsing carries the "Show N more" affordance, so a collapsed card is
/// `COLLAPSE_VISIBLE_ROWS + 1` rows tall.
pub const COLLAPSE_VISIBLE_ROWS: usize = 8;

pub const NAME_SIZE: f64 = 12.0;
pub const TYPE_SIZE: f64 = 10.0;
pub const TITLE_SIZE: f64 = 13.0;
pub const META_SIZE: f64 = 10.0;
pub const LABEL_SIZE: f64 = 9.5;

const ICON_W: f64 = 12.0;
const ICON_GAP: f64 = 6.0;
const NAME_TYPE_GAP: f64 = 14.0;
const BADGE_W: f64 = 13.0;
const BADGE_GAP: f64 = 3.0;
const MIN_CARD_W: f64 = 214.0;
const MAX_CARD_W: f64 = 380.0;
const MARGIN: f64 = 48.0;

const DEFAULT_TABLE_COLOR: Color = Color {
    r: 148,
    g: 163,
    b: 184,
    a: 255,
};
const DEFAULT_RELATION_COLOR: Color = Color {
    r: 100,
    g: 116,
    b: 139,
    a: 255,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Badge {
    /// Primary key.
    Key,
    Unique,
    Indexed,
    Mandatory,
    /// 4D-internal (`visible="false"`) field.
    Hidden,
}

impl Badge {
    pub fn letter(self) -> &'static str {
        match self {
            Badge::Key => "K",
            Badge::Unique => "U",
            Badge::Indexed => "I",
            Badge::Mandatory => "M",
            Badge::Hidden => "H",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Badge::Key => "primary key",
            Badge::Unique => "unique",
            Badge::Indexed => "indexed",
            Badge::Mandatory => "mandatory",
            Badge::Hidden => "hidden (4D-internal) field",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// Index into the owning table's full field list.
    pub field_index: usize,
    /// Vertical offset of the row's top edge, relative to the card's top.
    pub y: f64,
    pub name: String,
    /// Name after ellipsizing to the card width.
    pub display_name: String,
    pub type_label: String,
    pub type_color: String,
    pub glyph: &'static str,
    pub badges: Vec<Badge>,
    pub hidden: bool,
    pub tip: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub ghost: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub accent: String,
    pub accent_dark: String,
    /// `true` when the accent is light enough to need dark header text.
    pub dark_text: bool,
    pub rows: Vec<Row>,
    pub field_count: usize,
    pub hidden_field_count: usize,
    pub relation_count: usize,
    pub primary_key: Option<String>,
    /// Number of rows kept visible when the card is collapsed. `None` means the
    /// card shows every row, which is always the case for static SVG/PNG — a
    /// still image has no way to reveal hidden rows.
    pub collapsed_rows: Option<usize>,
    /// Height the card needs once expanded. Equals `h` when not collapsed.
    /// Expansion draws over neighbours instead of reflowing, so `h` — and
    /// therefore every edge anchor — stays valid.
    pub expanded_h: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub id: String,
    pub from_card: usize,
    pub to_card: usize,
    pub from_table: String,
    pub from_field: String,
    pub to_table: String,
    pub to_field: String,
    pub name_nto1: Option<String>,
    pub name_1ton: Option<String>,
    pub color: String,
    pub path: String,
    /// Label anchor near the "N" end.
    pub from_label: [f64; 2],
    /// Label anchor near the "1" end.
    pub to_label: [f64; 2],
    pub from_anchor_left: bool,
    pub to_anchor_left: bool,
    pub auto_load_nto1: bool,
    pub auto_load_1ton: bool,
    pub integrity: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    pub title: String,
    pub layout: &'static str,
    pub hide_system_fields: bool,
    pub width: f64,
    pub height: f64,
    pub cards: Vec<Card>,
    pub edges: Vec<Edge>,
    /// Set when `as-designed` was asked for but too few of the selected tables
    /// carried editor coordinates, so the whole selection was auto-laid out.
    pub layout_fallback: bool,
}

#[derive(Debug, Clone)]
pub struct SceneOptions {
    pub title: String,
    pub layout: LayoutMode,
    pub hide_system_fields: bool,
    /// Collapse dense cards behind a "Show N more" toggle. HTML only.
    pub collapse_dense_cards: bool,
}

/// Card geometry that does not depend on position, computed once and reused by
/// both layout modes.
struct CardShape {
    size: Size,
    rows: Vec<Row>,
    coordinates: Option<Coordinates>,
    collapsed_rows: Option<usize>,
    expanded_h: f64,
}

/// Share of selected tables that must carry usable editor coordinates before
/// `--layout as-designed` is honoured. Below this, mixing stored positions with
/// auto-placed ones looks worse than laying the whole selection out
/// automatically, so we do the latter.
pub const COORDINATE_COVERAGE_THRESHOLD: f64 = 0.8;

/// Fraction of `coords` that carry coordinates, counting real tables only —
/// ghost cards never have any and would drag the ratio down artificially.
fn coordinate_coverage(coords: &[Option<Coordinates>], table_count: usize) -> f64 {
    if table_count == 0 {
        return 1.0;
    }
    let with = coords
        .iter()
        .take(table_count)
        .filter(|c| c.is_some())
        .count();
    with as f64 / table_count as f64
}

pub fn build(catalog: &Catalog, selection: &Selection, options: &SceneOptions) -> Scene {
    let mut shapes: Vec<CardShape> = Vec::new();

    for &table_index in &selection.tables {
        shapes.push(shape_for_table(
            catalog,
            table_index,
            options.hide_system_fields,
            options.collapse_dense_cards,
        ));
    }
    for name in &selection.ghosts {
        shapes.push(shape_for_ghost(name));
    }

    let sizes: Vec<Size> = shapes.iter().map(|s| s.size).collect();
    let coords: Vec<Option<Coordinates>> = shapes.iter().map(|s| s.coordinates).collect();

    let edge_pairs: Vec<(usize, usize)> = selection
        .relations
        .iter()
        .map(|r| (card_index(selection, r.from), card_index(selection, r.to)))
        .collect();

    let coverage = coordinate_coverage(&coords, selection.tables.len());
    let layout_fallback =
        options.layout == LayoutMode::AsDesigned && coverage < COORDINATE_COVERAGE_THRESHOLD;
    let effective_layout = if layout_fallback {
        LayoutMode::Auto
    } else {
        options.layout
    };

    let mut positions = match effective_layout {
        LayoutMode::AsDesigned => layout::as_designed(&coords, &sizes),
        LayoutMode::Auto => layout::auto(&sizes, &edge_pairs),
    };
    let (width, height) = layout::normalize(&mut positions, &sizes, MARGIN);

    let table_count = selection.tables.len();
    let mut cards: Vec<Card> = Vec::with_capacity(shapes.len());
    for (i, shape) in shapes.into_iter().enumerate() {
        let ghost = i >= table_count;
        let position = positions[i];
        if ghost {
            let name = selection.ghosts[i - table_count].clone();
            let display_name =
                fonts::ellipsize(&name, TITLE_SIZE, shape.size.w - PAD_X * 2.0, false);
            cards.push(Card {
                id: format!("g{}", i - table_count),
                name,
                display_name,
                ghost: true,
                x: layout::round2(position.x),
                y: layout::round2(position.y),
                w: layout::round2(shape.size.w),
                h: layout::round2(shape.size.h),
                accent: DEFAULT_TABLE_COLOR.to_hex(),
                accent_dark: DEFAULT_TABLE_COLOR.darken(0.25).to_hex(),
                dark_text: true,
                rows: Vec::new(),
                field_count: 0,
                hidden_field_count: 0,
                relation_count: 0,
                primary_key: None,
                collapsed_rows: None,
                expanded_h: layout::round2(shape.size.h),
            });
        } else {
            let table = &catalog.tables[selection.tables[i]];
            let accent = table.color.unwrap_or(DEFAULT_TABLE_COLOR);
            let display_name = fonts::ellipsize(
                &table.name,
                TITLE_SIZE,
                shape.size.w - PAD_X * 2.0 - 56.0,
                false,
            );
            cards.push(Card {
                id: format!("t{i}"),
                name: table.name.clone(),
                display_name,
                ghost: false,
                x: layout::round2(position.x),
                y: layout::round2(position.y),
                w: layout::round2(shape.size.w),
                h: layout::round2(shape.size.h),
                accent: accent.to_hex(),
                accent_dark: accent.darken(0.32).to_hex(),
                dark_text: accent.luminance() > 0.55,
                rows: shape.rows,
                field_count: table.fields.len(),
                hidden_field_count: table.fields.iter().filter(|f| f.hidden).count(),
                relation_count: catalog.relations_in(&table.name)
                    + catalog.relations_out(&table.name),
                primary_key: table.primary_key.clone(),
                collapsed_rows: shape.collapsed_rows,
                expanded_h: layout::round2(shape.expanded_h),
            });
        }
    }

    let edges = route_edges(catalog, selection, &cards);

    Scene {
        title: options.title.clone(),
        layout: effective_layout.as_str(),
        hide_system_fields: options.hide_system_fields,
        width,
        height,
        cards,
        edges,
        layout_fallback,
    }
}

fn card_index(selection: &Selection, endpoint: Endpoint) -> usize {
    match endpoint {
        Endpoint::Table(i) => i,
        Endpoint::Ghost(i) => selection.tables.len() + i,
    }
}

fn shape_for_ghost(name: &str) -> CardShape {
    let width =
        (fonts::measure_sans_bold(name, TITLE_SIZE) + PAD_X * 2.0 + 18.0).clamp(130.0, 240.0);
    CardShape {
        size: Size {
            w: layout::round2(width),
            h: GHOST_H,
        },
        collapsed_rows: None,
        expanded_h: GHOST_H,
        rows: Vec::new(),
        coordinates: None,
    }
}

fn shape_for_table(
    catalog: &Catalog,
    table_index: usize,
    hide_system: bool,
    collapse: bool,
) -> CardShape {
    let table = &catalog.tables[table_index];
    let visible: Vec<(usize, &crate::model::Field)> = table.visible_fields(hide_system).collect();

    let meta = format!(
        "{} field{}",
        table.fields.len(),
        if table.fields.len() == 1 { "" } else { "s" }
    );
    let header_need = PAD_X
        + fonts::measure_sans_bold(&table.name, TITLE_SIZE)
        + 14.0
        + fonts::measure_sans(&meta, META_SIZE)
        + PAD_X;

    let mut content_need: f64 = header_need;
    for (_, field) in &visible {
        let badges = badges_for(field);
        let badge_w = if badges.is_empty() {
            0.0
        } else {
            badges.len() as f64 * (BADGE_W + BADGE_GAP)
        };
        let need = PAD_X
            + ICON_W
            + ICON_GAP
            + fonts::measure_sans(&field.name, NAME_SIZE)
            + NAME_TYPE_GAP
            + fonts::measure_mono(&type_badge_text(field), TYPE_SIZE)
            + BADGE_GAP
            + badge_w
            + PAD_X;
        content_need = content_need.max(need);
    }

    let width = layout::round2(content_need.clamp(MIN_CARD_W, MAX_CARD_W));
    let collapsed_rows = if collapse && visible.len() > COLLAPSE_MIN_FIELDS {
        Some(COLLAPSE_VISIBLE_ROWS)
    } else {
        None
    };
    // The toggle row sits at a fixed offset immediately below the kept rows and
    // the remaining rows are pushed one row further down, so expanding never
    // has to move anything.
    let toggle_row = usize::from(collapsed_rows.is_some());
    let expanded_h = HEADER_H + (visible.len() + toggle_row) as f64 * ROW_H + PAD_BOTTOM;
    let height = match collapsed_rows {
        Some(keep) => HEADER_H + (keep + 1) as f64 * ROW_H + PAD_BOTTOM,
        None => expanded_h,
    };

    // Reserve horizontal room for the type badge and status badges, then give
    // whatever is left to the field name.
    let rows = visible
        .iter()
        .enumerate()
        .map(|(n, (field_index, field))| {
            let badges = badges_for(field);
            let badge_w = if badges.is_empty() {
                0.0
            } else {
                badges.len() as f64 * (BADGE_W + BADGE_GAP)
            };
            let type_text = type_badge_text(field);
            let name_budget = width
                - PAD_X * 2.0
                - ICON_W
                - ICON_GAP
                - NAME_TYPE_GAP
                - fonts::measure_mono(&type_text, TYPE_SIZE)
                - BADGE_GAP
                - badge_w;
            Row {
                field_index: *field_index,
                y: layout::round2(
                    HEADER_H
                        + (n + usize::from(collapsed_rows.is_some_and(|keep| n >= keep))) as f64
                            * ROW_H,
                ),
                name: field.name.clone(),
                display_name: fonts::ellipsize(
                    &field.name,
                    NAME_SIZE,
                    name_budget.max(24.0),
                    false,
                ),
                type_label: type_text,
                type_color: field.type_color().to_string(),
                glyph: glyph_name(field.type_glyph()),
                badges,
                hidden: field.hidden,
                tip: field.tip.clone(),
            }
        })
        .collect();

    CardShape {
        size: Size {
            w: width,
            h: layout::round2(height),
        },
        rows,
        coordinates: table.coordinates,
        collapsed_rows,
        expanded_h: layout::round2(expanded_h),
    }
}

fn type_badge_text(field: &crate::model::Field) -> String {
    match field.limiting_length {
        Some(len) if len > 0 => format!("{}({len})", field.type_label()),
        _ => field.type_label(),
    }
}

fn badges_for(field: &crate::model::Field) -> Vec<Badge> {
    let mut badges = Vec::new();
    if field.is_primary_key {
        badges.push(Badge::Key);
    }
    if field.unique && !field.is_primary_key {
        badges.push(Badge::Unique);
    }
    if field.indexed {
        badges.push(Badge::Indexed);
    }
    if field.mandatory || field.never_null {
        badges.push(Badge::Mandatory);
    }
    if field.hidden {
        badges.push(Badge::Hidden);
    }
    badges
}

fn glyph_name(glyph: types::Glyph) -> &'static str {
    match glyph {
        types::Glyph::Boolean => "boolean",
        types::Glyph::Number => "number",
        types::Glyph::Date => "date",
        types::Glyph::Time => "time",
        types::Glyph::Text => "text",
        types::Glyph::Blob => "blob",
        types::Glyph::Image => "image",
        types::Glyph::Object => "object",
        types::Glyph::Unknown => "unknown",
    }
}

/// Vertical centre of the row for `field`, or the header centre when that field
/// is not currently rendered.
fn anchor_y(card: &Card, field_name: &str, catalog_table: Option<&crate::model::Table>) -> f64 {
    if let Some(table) = catalog_table {
        if let Some(field_index) = table.fields.iter().position(|f| f.name == field_name) {
            if let Some(row) = card.rows.iter().find(|r| r.field_index == field_index) {
                return card.y + row.y + ROW_H / 2.0;
            }
        }
    }
    card.y + HEADER_H / 2.0
}

fn route_edges(catalog: &Catalog, selection: &Selection, cards: &[Card]) -> Vec<Edge> {
    let mut edges = Vec::with_capacity(selection.relations.len());

    for (n, selected) in selection.relations.iter().enumerate() {
        let relation = &catalog.relations[selected.relation];
        let from_card = card_index(selection, selected.from);
        let to_card = card_index(selection, selected.to);
        let a = &cards[from_card];
        let b = &cards[to_card];

        let ay = anchor_y(a, &relation.from_field, catalog.table(&relation.from_table));
        let by = anchor_y(b, &relation.to_field, catalog.table(&relation.to_table));

        let (from_left, to_left) = if from_card == to_card {
            (false, false)
        } else {
            let a_centre = a.x + a.w / 2.0;
            let b_centre = b.x + b.w / 2.0;
            if b_centre >= a_centre {
                (false, true)
            } else {
                (true, false)
            }
        };

        let ax = if from_left { a.x } else { a.x + a.w };
        let bx = if to_left { b.x } else { b.x + b.w };

        let (path, from_label, to_label) = if from_card == to_card {
            self_loop(ax, ay, by)
        } else {
            bezier(ax, ay, bx, by, from_left, to_left)
        };

        edges.push(Edge {
            id: format!("r{n}"),
            from_card,
            to_card,
            from_table: relation.from_table.clone(),
            from_field: relation.from_field.clone(),
            to_table: relation.to_table.clone(),
            to_field: relation.to_field.clone(),
            name_nto1: relation.name_nto1.clone(),
            name_1ton: relation.name_1ton.clone(),
            color: legible_edge_color(relation.color.unwrap_or(DEFAULT_RELATION_COLOR)).to_hex(),
            path,
            from_label,
            to_label,
            from_anchor_left: from_left,
            to_anchor_left: to_left,
            auto_load_nto1: relation.auto_load_nto1,
            auto_load_1ton: relation.auto_load_1ton,
            integrity: relation.integrity.clone(),
        });
    }

    edges
}

/// Relation colours in the catalog are often pastel, which disappears against a
/// light background. Darken only the ones that are actually too light.
fn legible_edge_color(color: Color) -> Color {
    let luminance = color.luminance();
    if luminance > 0.62 {
        color.darken(((luminance - 0.62) * 1.4).min(0.45))
    } else {
        color
    }
}

fn bezier(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    from_left: bool,
    to_left: bool,
) -> (String, [f64; 2], [f64; 2]) {
    let reach = ((bx - ax).abs() * 0.45).clamp(46.0, 170.0);
    let c1x = if from_left { ax - reach } else { ax + reach };
    let c2x = if to_left { bx - reach } else { bx + reach };

    let path = format!(
        "M{} {}C{} {},{} {},{} {}",
        layout::round2(ax),
        layout::round2(ay),
        layout::round2(c1x),
        layout::round2(ay),
        layout::round2(c2x),
        layout::round2(by),
        layout::round2(bx),
        layout::round2(by)
    );

    let at = |t: f64| -> [f64; 2] {
        let mt = 1.0 - t;
        let x =
            mt * mt * mt * ax + 3.0 * mt * mt * t * c1x + 3.0 * mt * t * t * c2x + t * t * t * bx;
        let y = mt * mt * mt * ay + 3.0 * mt * mt * t * ay + 3.0 * mt * t * t * by + t * t * t * by;
        [layout::round2(x), layout::round2(y - 6.0)]
    };

    (path, at(0.16), at(0.84))
}

/// A relation whose two endpoints live on the same card loops out to the right.
fn self_loop(x: f64, ay: f64, by: f64) -> (String, [f64; 2], [f64; 2]) {
    let reach = 70.0;
    let path = format!(
        "M{} {}C{} {},{} {},{} {}",
        layout::round2(x),
        layout::round2(ay),
        layout::round2(x + reach),
        layout::round2(ay),
        layout::round2(x + reach),
        layout::round2(by),
        layout::round2(x),
        layout::round2(by)
    );
    (
        path,
        [layout::round2(x + reach * 0.6), layout::round2(ay - 6.0)],
        [layout::round2(x + reach * 0.6), layout::round2(by - 6.0)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::{self, SelectionSpec};

    fn fixture() -> Catalog {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/references/InvoicesDemo.xml"
        ))
        .unwrap();
        crate::parse::parse_bytes(&bytes).unwrap()
    }

    fn default_spec() -> SelectionSpec {
        SelectionSpec {
            depth: 1,
            max_tables: 300,
            ..Default::default()
        }
    }

    #[test]
    fn cards_never_overlap() {
        let catalog = fixture();
        let selection = select::select(&catalog, &default_spec()).unwrap();
        for mode in [LayoutMode::AsDesigned, LayoutMode::Auto] {
            let scene = build(
                &catalog,
                &selection,
                &SceneOptions {
                    title: "t".into(),
                    layout: mode,
                    hide_system_fields: false,
                    collapse_dense_cards: false,
                },
            );
            for i in 0..scene.cards.len() {
                for j in (i + 1)..scene.cards.len() {
                    let a = &scene.cards[i];
                    let b = &scene.cards[j];
                    let apart = a.x + a.w <= b.x
                        || b.x + b.w <= a.x
                        || a.y + a.h <= b.y
                        || b.y + b.h <= a.y;
                    assert!(apart, "{} overlaps {} in {:?}", a.name, b.name, mode);
                }
            }
        }
    }

    #[test]
    fn card_width_is_computed_not_taken_from_xml() {
        let catalog = fixture();
        let selection = select::select(&catalog, &default_spec()).unwrap();
        let scene = build(
            &catalog,
            &selection,
            &SceneOptions {
                title: "t".into(),
                layout: LayoutMode::AsDesigned,
                hide_system_fields: false,
                collapse_dense_cards: false,
            },
        );
        for card in &scene.cards {
            assert!(card.w >= 130.0 && card.w <= MAX_CARD_W);
        }
    }

    #[test]
    fn hiding_system_fields_shrinks_cards() {
        let catalog = fixture();
        let selection = select::select(&catalog, &default_spec()).unwrap();
        let options = |hide| SceneOptions {
            title: "t".into(),
            layout: LayoutMode::AsDesigned,
            hide_system_fields: hide,
            collapse_dense_cards: false,
        };
        let shown = build(&catalog, &selection, &options(false));
        let hidden = build(&catalog, &selection, &options(true));
        let total = |s: &Scene| s.cards.iter().map(|c| c.rows.len()).sum::<usize>();
        assert!(total(&hidden) <= total(&shown));
    }
}
