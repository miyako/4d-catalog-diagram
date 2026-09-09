//! Internal catalog model. This is the single shared representation that every
//! renderer consumes; parsing happens exactly once.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Mix towards white (`t` = 1.0 is pure white). Used to soften the
    /// designer's saturated editor colours for a light theme.
    pub fn lighten(self, t: f64) -> Color {
        let m = |c: u8| {
            (c as f64 + (255.0 - c as f64) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Color {
            r: m(self.r),
            g: m(self.g),
            b: m(self.b),
            a: self.a,
        }
    }

    /// Mix towards black.
    pub fn darken(self, t: f64) -> Color {
        let m = |c: u8| (c as f64 * (1.0 - t)).round().clamp(0.0, 255.0) as u8;
        Color {
            r: m(self.r),
            g: m(self.g),
            b: m(self.b),
            a: self.a,
        }
    }

    /// Perceptual luminance, used to pick a readable foreground colour.
    pub fn luminance(self) -> f64 {
        (0.2126 * self.r as f64 + 0.7152 * self.g as f64 + 0.0722 * self.b as f64) / 255.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinates {
    pub left: f64,
    pub top: f64,
    /// 4D's own editor measurements. Kept for reference only — card size is
    /// always recomputed from our own text metrics (see BUILD-SPEC §4.2).
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub uuid: String,
    pub id: Option<i64>,
    pub type_code: i64,
    pub unique: bool,
    pub autosequence: bool,
    pub never_null: bool,
    pub limiting_length: Option<i64>,
    pub mandatory: bool,
    pub modifiable: bool,
    /// `field_extra/@visible="false"` marks 4D-internal fields.
    pub hidden: bool,
    pub tip: Option<String>,
    pub color: Option<Color>,
    pub indexed: bool,
    pub is_primary_key: bool,
}

impl Field {
    /// The code as 4D presents it, which for string fields depends on whether
    /// a length limit is set.
    fn display_code(&self) -> i64 {
        crate::types::canonical_code(self.type_code, self.limiting_length)
    }

    pub fn type_label(&self) -> String {
        crate::types::type_label(self.display_code())
    }

    pub fn type_color(&self) -> &'static str {
        crate::types::type_color(self.display_code())
    }

    pub fn type_glyph(&self) -> crate::types::Glyph {
        crate::types::type_glyph(self.display_code())
    }
}

#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub uuid: String,
    pub id: Option<i64>,
    pub fields: Vec<Field>,
    pub primary_key: Option<String>,
    pub color: Option<Color>,
    pub coordinates: Option<Coordinates>,
}

impl Table {
    pub fn visible_fields(&self, hide_system: bool) -> impl Iterator<Item = (usize, &Field)> {
        self.fields
            .iter()
            .enumerate()
            .filter(move |(_, f)| !(hide_system && f.hidden))
    }
}

#[derive(Debug, Clone)]
pub struct Relation {
    pub uuid: String,
    /// Label shown when navigating from the "many" side to the "one" side.
    pub name_nto1: Option<String>,
    /// Label shown when navigating from the "one" side to the "many" side.
    pub name_1ton: Option<String>,
    /// The "N" side.
    pub from_table: String,
    pub from_field: String,
    /// The "1" side.
    pub to_table: String,
    pub to_field: String,
    pub auto_load_nto1: bool,
    pub auto_load_1ton: bool,
    pub foreign_key: bool,
    pub integrity: Option<String>,
    pub color: Option<Color>,
    pub prefers_left: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Index {
    pub uuid: String,
    pub kind: Option<String>,
    pub unique_keys: bool,
    pub type_code: Option<i64>,
    pub tables: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub base_name: String,
    pub tables: Vec<Table>,
    pub relations: Vec<Relation>,
    pub indexes: BTreeMap<String, Index>,
    pub warnings: Vec<String>,
}

impl Catalog {
    pub fn table_index(&self, name: &str) -> Option<usize> {
        self.tables.iter().position(|t| t.name == name)
    }

    pub fn table(&self, name: &str) -> Option<&Table> {
        self.table_index(name).map(|i| &self.tables[i])
    }

    /// Number of relations where `table` is the destination ("1") side.
    pub fn relations_in(&self, table: &str) -> usize {
        self.relations
            .iter()
            .filter(|r| r.to_table == table)
            .count()
    }

    /// Number of relations where `table` is the source ("N") side.
    pub fn relations_out(&self, table: &str) -> usize {
        self.relations
            .iter()
            .filter(|r| r.from_table == table)
            .count()
    }
}
