//! XML → [`Catalog`].
//!
//! Parsing is deliberately permissive: unknown elements and attributes are
//! ignored, missing optional data yields defaults, and only a genuinely
//! not-well-formed document is a hard failure. External entities and DTDs are
//! never resolved — `roxmltree` has no I/O of any kind, so a document carrying
//! `<!DOCTYPE base SYSTEM "http://www.4d.com/dtd/2007/base.dtd">` parses
//! identically with or without network access.

use crate::error::{AppError, Result};
use crate::model::{Catalog, Color, Coordinates, Field, Index, Relation, Table};

/// Decode raw bytes to a `String`, honouring a BOM or the encoding declared in
/// the XML prolog. Older 4D exports are sometimes Windows-1252.
pub fn decode(bytes: &[u8]) -> (String, Vec<String>) {
    let mut warnings = Vec::new();

    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let (text, _, had_errors) = encoding_rs::UTF_8.decode(&bytes[3..]);
        if had_errors {
            warnings.push("input contained invalid UTF-8 sequences; they were replaced".into());
        }
        return (text.into_owned(), warnings);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let enc = if bytes[0] == 0xFF {
            encoding_rs::UTF_16LE
        } else {
            encoding_rs::UTF_16BE
        };
        let (text, _, _) = enc.decode(&bytes[2..]);
        return (text.into_owned(), warnings);
    }

    let declared = declared_encoding(bytes);
    let encoding = declared
        .as_deref()
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()));

    if let Some(label) = &declared {
        if encoding.is_none() {
            warnings.push(format!(
                "unknown declared encoding {label:?}; decoded as UTF-8"
            ));
        }
    }

    let encoding = encoding.unwrap_or(encoding_rs::UTF_8);
    let (text, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        warnings.push(format!(
            "input contained byte sequences invalid for {}; they were replaced",
            encoding.name()
        ));
    }
    (text.into_owned(), warnings)
}

/// Extract `encoding="…"` from the XML declaration without a full parse.
fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let head = &bytes[..bytes.len().min(256)];
    let head = String::from_utf8_lossy(head);
    let decl_end = head.find("?>")?;
    let decl = &head[..decl_end];
    let at = decl.find("encoding")?;
    let rest = &decl[at + "encoding".len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &rest[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

pub fn parse_bytes(bytes: &[u8]) -> Result<Catalog> {
    let (text, mut warnings) = decode(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();

    let options = roxmltree::ParsingOptions {
        // Permit an internal subset / DOCTYPE declaration. `roxmltree` performs
        // no I/O, so an external SYSTEM identifier is never fetched.
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let doc = roxmltree::Document::parse_with_options(&text, options)
        .map_err(|e| AppError::MalformedXml(format!("input is not well-formed XML: {e}")))?;

    let root = doc.root_element();
    if root.tag_name().name() != "base" {
        warnings.push(format!(
            "root element is <{}>, expected <base>; parsing anyway",
            root.tag_name().name()
        ));
    }

    let mut catalog = Catalog {
        base_name: attr(&root, "name").unwrap_or_else(|| "Catalog".to_string()),
        warnings,
        ..Default::default()
    };

    for node in root.children().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "table" => {
                if let Some(table) = parse_table(&node, &mut catalog.warnings) {
                    catalog.tables.push(table);
                }
            }
            "relation" => {
                if let Some(relation) = parse_relation(&node, &mut catalog.warnings) {
                    catalog.relations.push(relation);
                }
            }
            "index" => {
                let index = parse_index(&node);
                catalog.indexes.insert(index.uuid.clone(), index);
            }
            // Unknown top-level elements (schema, and anything future 4D adds)
            // are ignored on purpose.
            _ => {}
        }
    }

    validate_relations(&mut catalog);
    Ok(catalog)
}

fn attr(node: &roxmltree::Node, name: &str) -> Option<String> {
    node.attribute(name).map(|s| s.to_string())
}

fn attr_bool(node: &roxmltree::Node, name: &str) -> Option<bool> {
    match node.attribute(name)?.trim() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn attr_i64(node: &roxmltree::Node, name: &str) -> Option<i64> {
    node.attribute(name)?.trim().parse().ok()
}

fn attr_f64(node: &roxmltree::Node, name: &str) -> Option<f64> {
    let v: f64 = node.attribute(name)?.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

fn child<'a, 'i>(node: &roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

fn parse_color(node: &roxmltree::Node) -> Option<Color> {
    let c = child(node, "color")?;
    let comp = |name: &str, default: u8| {
        attr_i64(&c, name)
            .map(|v| v.clamp(0, 255) as u8)
            .unwrap_or(default)
    };
    Some(Color {
        r: comp("red", 0),
        g: comp("green", 0),
        b: comp("blue", 0),
        a: comp("alpha", 255),
    })
}

fn parse_table(node: &roxmltree::Node, warnings: &mut Vec<String>) -> Option<Table> {
    let name = match attr(node, "name") {
        Some(n) if !n.is_empty() => n,
        _ => {
            warnings.push("skipped a <table> with no name attribute".into());
            return None;
        }
    };

    let mut fields = Vec::new();
    for f in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "field")
    {
        if let Some(field) = parse_field(&f, warnings, &name) {
            fields.push(field);
        }
    }

    let primary_key = child(node, "primary_key").and_then(|pk| attr(&pk, "field_name"));

    let table_extra = child(node, "table_extra");
    let editor_info = table_extra
        .as_ref()
        .and_then(|e| child(e, "editor_table_info"));
    let color = editor_info.as_ref().and_then(parse_color);
    let coordinates = editor_info
        .as_ref()
        .and_then(|e| child(e, "coordinates"))
        .and_then(|c| {
            Some(Coordinates {
                left: attr_f64(&c, "left")?,
                top: attr_f64(&c, "top")?,
                width: attr_f64(&c, "width").unwrap_or(0.0),
                height: attr_f64(&c, "height").unwrap_or(0.0),
            })
        });

    let mut table = Table {
        name,
        uuid: attr(node, "uuid").unwrap_or_default(),
        id: attr_i64(node, "id"),
        fields,
        primary_key: primary_key.clone(),
        color,
        coordinates,
    };

    if let Some(pk) = &primary_key {
        for field in &mut table.fields {
            if &field.name == pk {
                field.is_primary_key = true;
            }
        }
    }

    Some(table)
}

fn parse_field(node: &roxmltree::Node, warnings: &mut Vec<String>, table: &str) -> Option<Field> {
    let name = match attr(node, "name") {
        Some(n) if !n.is_empty() => n,
        _ => {
            warnings.push(format!(
                "skipped a <field> with no name attribute in table {table:?}"
            ));
            return None;
        }
    };

    let extra = child(node, "field_extra");
    let editor_info = extra.as_ref().and_then(|e| child(e, "editor_field_info"));

    let tip = extra
        .as_ref()
        .and_then(|e| child(e, "tip"))
        .and_then(|t| t.text().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());

    // A field is indexed if it points at any top-level <index>; the reference
    // direction is field → index, joined by uuid.
    let index_refs: Vec<String> = node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "index_ref")
        .filter_map(|n| attr(&n, "uuid"))
        .collect();

    Some(Field {
        name,
        uuid: attr(node, "uuid").unwrap_or_default(),
        id: attr_i64(node, "id"),
        type_code: attr_i64(node, "type").unwrap_or(-1),
        unique: attr_bool(node, "unique").unwrap_or(false),
        autosequence: attr_bool(node, "autosequence").unwrap_or(false),
        never_null: attr_bool(node, "never_null").unwrap_or(false),
        limiting_length: attr_i64(node, "limiting_length"),
        mandatory: extra
            .as_ref()
            .and_then(|e| attr_bool(e, "mandatory"))
            .unwrap_or(false),
        modifiable: extra
            .as_ref()
            .and_then(|e| attr_bool(e, "modifiable"))
            .unwrap_or(true),
        hidden: !extra
            .as_ref()
            .and_then(|e| attr_bool(e, "visible"))
            .unwrap_or(true),
        tip,
        color: editor_info.as_ref().and_then(parse_color),
        indexed: !index_refs.is_empty(),
        is_primary_key: false,
    })
}

fn parse_relation(node: &roxmltree::Node, warnings: &mut Vec<String>) -> Option<Relation> {
    let endpoint = |kind: &str| -> Option<(String, String)> {
        let related = node.children().find(|n| {
            n.is_element()
                && n.tag_name().name() == "related_field"
                && n.attribute("kind") == Some(kind)
        })?;
        let field_ref = child(&related, "field_ref")?;
        let table_ref = child(&field_ref, "table_ref")?;
        Some((attr(&table_ref, "name")?, attr(&field_ref, "name")?))
    };

    let (from_table, from_field) = match endpoint("source") {
        Some(v) => v,
        None => {
            warnings.push("skipped a <relation> with no resolvable source field".into());
            return None;
        }
    };
    let (to_table, to_field) = match endpoint("destination") {
        Some(v) => v,
        None => {
            warnings.push(format!(
                "skipped a <relation> from {from_table}.{from_field} with no resolvable destination field"
            ));
            return None;
        }
    };

    let extra = child(node, "relation_extra");
    let editor_info = extra
        .as_ref()
        .and_then(|e| child(e, "editor_relation_info"));

    Some(Relation {
        uuid: attr(node, "uuid").unwrap_or_default(),
        name_nto1: attr(node, "name_Nto1").filter(|s| !s.is_empty()),
        name_1ton: attr(node, "name_1toN").filter(|s| !s.is_empty()),
        from_table,
        from_field,
        to_table,
        to_field,
        auto_load_nto1: attr_bool(node, "auto_load_Nto1").unwrap_or(false),
        auto_load_1ton: attr_bool(node, "auto_load_1toN").unwrap_or(false),
        foreign_key: attr_bool(node, "foreign_key").unwrap_or(false),
        integrity: attr(node, "integrity"),
        color: editor_info.as_ref().and_then(parse_color),
        prefers_left: editor_info
            .as_ref()
            .and_then(|e| attr_bool(e, "prefers_left"))
            .unwrap_or(false),
    })
}

fn parse_index(node: &roxmltree::Node) -> Index {
    Index {
        uuid: attr(node, "uuid").unwrap_or_default(),
        kind: attr(node, "kind"),
        unique_keys: attr_bool(node, "unique_keys").unwrap_or(false),
        type_code: attr_i64(node, "type"),
        tables: node
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "table_ref")
            .filter_map(|n| attr(&n, "name"))
            .collect(),
    }
}

fn validate_relations(catalog: &mut Catalog) {
    let mut warnings = Vec::new();
    for relation in &catalog.relations {
        for (table_name, field_name) in [
            (&relation.from_table, &relation.from_field),
            (&relation.to_table, &relation.to_field),
        ] {
            match catalog.table(table_name) {
                None => warnings.push(format!(
                    "relation references unknown table {table_name:?}; it will be drawn as an external reference"
                )),
                Some(table) => {
                    if !table.fields.iter().any(|f| &f.name == field_name) {
                        warnings.push(format!(
                            "relation references unknown field {table_name}.{field_name}"
                        ));
                    }
                }
            }
        }
    }
    warnings.dedup();
    catalog.warnings.extend(warnings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_catalog() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
        <base name="Tiny">
          <table name="A" id="1">
            <field name="ID" type="4" unique="true" id="1"/>
            <primary_key field_name="ID"/>
          </table>
        </base>"#;
        let catalog = parse_bytes(xml).unwrap();
        assert_eq!(catalog.base_name, "Tiny");
        assert_eq!(catalog.tables.len(), 1);
        assert!(catalog.tables[0].fields[0].is_primary_key);
    }

    #[test]
    fn external_doctype_is_never_fetched() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE base SYSTEM "http://www.4d.com/dtd/2007/base.dtd">
        <base name="WithDoctype"><table name="A"/></base>"#;
        let catalog = parse_bytes(xml).unwrap();
        assert_eq!(catalog.base_name, "WithDoctype");
    }

    #[test]
    fn unknown_elements_and_attributes_are_ignored() {
        let xml = br#"<base name="X" future_attr="1">
          <brand_new_element foo="bar"/>
          <table name="A" mystery="yes"><field name="F" type="999"><novel/></field></table>
        </base>"#;
        let catalog = parse_bytes(xml).unwrap();
        assert_eq!(catalog.tables[0].fields[0].type_label(), "Type 999");
    }

    #[test]
    fn malformed_xml_is_a_hard_error() {
        let err = parse_bytes(b"<base><table></base>").unwrap_err();
        assert_eq!(err.exit_code(), crate::error::ExitCode::MalformedXml);
    }

    #[test]
    fn detects_declared_encoding() {
        assert_eq!(
            declared_encoding(br#"<?xml version="1.0" encoding="windows-1252"?><base/>"#),
            Some("windows-1252".to_string())
        );
    }

    #[test]
    fn decodes_windows_1252() {
        let mut bytes = br#"<?xml version="1.0" encoding="windows-1252"?><base name=""#.to_vec();
        bytes.push(0xE9); // é in Windows-1252
        bytes.extend_from_slice(br#""/>"#);
        let catalog = parse_bytes(&bytes).unwrap();
        assert_eq!(catalog.base_name, "é");
    }

    #[test]
    fn strips_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"<base name="Bom"/>"#);
        assert_eq!(parse_bytes(&bytes).unwrap().base_name, "Bom");
    }
}
