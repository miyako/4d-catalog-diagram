//! Plain-text diagram exports: Mermaid `erDiagram` and Graphviz `digraph`.
//!
//! These reuse the same catalog + selection as the drawn formats but skip
//! layout and rasterization entirely — the target tool does its own layout, so
//! stored editor coordinates are irrelevant here.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::model::{Catalog, Field, Table};
use crate::select::{Endpoint, Selection};

/// Identifiers in both Mermaid and Graphviz record labels are far more
/// restrictive than 4D object names, which may contain spaces, quotes and
/// non-ASCII characters. Map every selected table to a safe, unique id once and
/// keep the original around for the human-readable label.
struct Names {
    /// Safe id per selected table, in `Selection::tables` order.
    tables: Vec<String>,
    /// Safe id per ghost, in `Selection::ghosts` order.
    ghosts: Vec<String>,
}

impl Names {
    fn of(&self, endpoint: Endpoint) -> &str {
        match endpoint {
            Endpoint::Table(i) => &self.tables[i],
            Endpoint::Ghost(i) => &self.ghosts[i],
        }
    }
}

/// Reduce a name to `[A-Za-z0-9_]`, never empty and never leading with a digit.
fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.is_empty() {
        out.push('_');
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'T');
    }
    out
}

fn names(catalog: &Catalog, selection: &Selection) -> Names {
    // Sanitizing is lossy, so two distinct tables can collapse onto the same
    // id. Disambiguate deterministically rather than silently merging entities.
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut unique = |raw: &str| {
        let base = sanitize(raw);
        let count = seen.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            base
        } else {
            format!("{base}_{}", *count)
        }
    };

    let tables: Vec<String> = selection
        .tables
        .iter()
        .map(|&i| unique(&catalog.tables[i].name))
        .collect();
    let ghosts: Vec<String> = selection.ghosts.iter().map(|n| unique(n)).collect();
    Names { tables, ghosts }
}

/// Relation labels are free text, so keep them readable but strip everything
/// that could terminate a Mermaid string or an enclosing HTML context.
fn label_text(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| match c {
            '"' => '\'',
            '<' | '>' | '\\' | '&' => '_',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        })
        .collect();
    cleaned.chars().take(80).collect()
}

fn field_key(table: &Table, field: &Field) -> Option<&'static str> {
    if field.is_primary_key || table.primary_key.as_deref() == Some(field.name.as_str()) {
        Some("PK")
    } else if field.unique {
        Some("UK")
    } else {
        None
    }
}

/// Mermaid `erDiagram` source.
pub fn mermaid(catalog: &Catalog, selection: &Selection, hide_system_fields: bool) -> String {
    let names = names(catalog, selection);
    let mut out = String::from("erDiagram\n");

    for (position, &table_index) in selection.tables.iter().enumerate() {
        let table = &catalog.tables[table_index];
        let _ = writeln!(out, "    {} {{", names.tables[position]);
        for (_, field) in table.visible_fields(hide_system_fields) {
            let key = match field_key(table, field) {
                Some(k) => format!(" {k}"),
                None => String::new(),
            };
            let _ = writeln!(
                out,
                "        {} {}{key}",
                sanitize(&field.type_label()),
                sanitize(&field.name)
            );
        }
        out.push_str("    }\n");
    }

    // Tables referenced by a relation but outside the selection still need to
    // exist as entities or the relationship lines dangle.
    for ghost in &names.ghosts {
        let _ = writeln!(out, "    {ghost} {{\n    }}");
    }

    for selected in &selection.relations {
        let relation = &catalog.relations[selected.relation];
        let label = relation
            .name_nto1
            .clone()
            .or_else(|| relation.name_1ton.clone())
            .unwrap_or_else(|| format!("{}_{}", relation.from_field, relation.to_field));
        // `to` is the "1" side, `from` is the "N" side.
        let _ = writeln!(
            out,
            "    {} ||--o{{ {} : \"{}\"",
            names.of(selected.to),
            names.of(selected.from),
            label_text(&label)
        );
    }

    out
}

/// Escape a string for use inside a Graphviz double-quoted record label.
fn dot_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' | '\\' | '{' | '}' | '|' | '<' | '>' | ' ' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' | '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// Graphviz `digraph` source with record-shaped nodes.
pub fn graphviz(catalog: &Catalog, selection: &Selection, hide_system_fields: bool) -> String {
    let names = names(catalog, selection);
    let mut out = String::from("digraph catalog {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str("  graph [splines=ortho, nodesep=0.4, ranksep=0.9];\n");
    out.push_str("  node [shape=record, fontname=\"Helvetica\", fontsize=10];\n");
    out.push_str("  edge [fontname=\"Helvetica\", fontsize=9];\n");

    // Ports let edges attach to the field that actually carries the relation
    // rather than to the middle of the node.
    let mut ports: Vec<BTreeMap<String, String>> = Vec::with_capacity(selection.tables.len());

    for (position, &table_index) in selection.tables.iter().enumerate() {
        let table = &catalog.tables[table_index];
        let mut label = format!("{{{}", dot_escape(&table.name));
        let mut table_ports = BTreeMap::new();
        for (field_index, field) in table.visible_fields(hide_system_fields) {
            let port = format!("f{field_index}");
            let key = match field_key(table, field) {
                Some(k) => format!(" [{k}]"),
                None => String::new(),
            };
            label.push('|');
            let _ = write!(
                label,
                "<{port}> {} : {}{}",
                dot_escape(&field.name),
                dot_escape(&field.type_label()),
                dot_escape(&key)
            );
            table_ports.insert(field.name.clone(), port);
        }
        label.push('}');
        let _ = writeln!(out, "  {} [label=\"{label}\"];", names.tables[position]);
        ports.push(table_ports);
    }

    for ghost in &names.ghosts {
        let _ = writeln!(
            out,
            "  {ghost} [label=\"{{{ghost}|(not in selection)}}\", style=dashed, fontcolor=\"#6b7280\", color=\"#9ca3af\"];"
        );
    }

    for selected in &selection.relations {
        let relation = &catalog.relations[selected.relation];
        let anchor = |endpoint: Endpoint, field: &str| match endpoint {
            Endpoint::Table(i) => match ports[i].get(field) {
                Some(port) => format!("{}:{port}", names.tables[i]),
                None => names.tables[i].clone(),
            },
            Endpoint::Ghost(i) => names.ghosts[i].clone(),
        };
        let label = relation
            .name_nto1
            .clone()
            .or_else(|| relation.name_1ton.clone())
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "  {} -> {} [label=\"{}\", arrowhead=none, arrowtail=crow, dir=back];",
            anchor(selected.to, &relation.to_field),
            anchor(selected.from, &relation.from_field),
            dot_escape(&label_text(&label))
        );
    }

    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_produces_safe_identifiers() {
        assert_eq!(sanitize("Order Lines"), "Order_Lines");
        assert_eq!(sanitize("2020 Sales"), "T2020_Sales");
        assert_eq!(sanitize(""), "_");
        assert_eq!(sanitize("<script>"), "_script_");
    }

    #[test]
    fn label_text_strips_string_terminators() {
        assert_eq!(
            label_text("</script><svg onload=x>"),
            "_/script__svg onload=x_"
        );
        assert_eq!(label_text("say \"hi\""), "say 'hi'");
    }

    #[test]
    fn dot_escape_neutralizes_record_syntax() {
        assert_eq!(dot_escape("a|b{c}"), "a\\|b\\{c\\}");
        assert_eq!(dot_escape("he said \"hi\""), "he\\ said\\ \\\"hi\\\"");
    }
}
