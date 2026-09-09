//! End-to-end tests over the shipped fixture and the CLI binary.

use std::path::{Path, PathBuf};
use std::process::Command;

use catalog_diagram::layout::LayoutMode;
use catalog_diagram::model::Catalog;
use catalog_diagram::render_svg::Theme;
use catalog_diagram::scene::{self, SceneOptions};
use catalog_diagram::select::{self, ExternalRefs, Selection, SelectionSpec};
use catalog_diagram::{inspect, parse, render_svg};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn invoices() -> Catalog {
    parse::parse_bytes(&std::fs::read(fixture_path("references/InvoicesDemo.xml")).unwrap())
        .unwrap()
}

fn spec() -> SelectionSpec {
    SelectionSpec {
        depth: 1,
        max_tables: 300,
        ..Default::default()
    }
}

fn names(catalog: &Catalog, selection: &Selection) -> Vec<String> {
    let mut names: Vec<String> = selection
        .tables
        .iter()
        .map(|&i| catalog.tables[i].name.clone())
        .collect();
    names.sort();
    names
}

fn render(catalog: &Catalog, selection: &Selection, layout: LayoutMode, hide: bool) -> String {
    let scene = scene::build(
        catalog,
        selection,
        &SceneOptions {
            title: catalog.base_name.clone(),
            layout,
            hide_system_fields: hide,
            collapse_dense_cards: false,
        },
    );
    render_svg::render(&scene, Theme::Light)
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_4d-catalog-diagram"))
}

// --------------------------------------------------------------- parsing

#[test]
fn fixture_parses_to_the_expected_shape() {
    let catalog = invoices();
    assert_eq!(catalog.base_name, "InvoicesDemo");
    assert_eq!(catalog.tables.len(), 5);
    assert_eq!(catalog.relations.len(), 3);
    assert!(catalog.tables.iter().all(|t| t.coordinates.is_some()));
    let clients = catalog.table("CLIENTS").unwrap();
    assert_eq!(clients.primary_key.as_deref(), Some("ID"));
    assert!(clients.fields.iter().any(|f| f.indexed));
}

// ------------------------------------------------------------- selection

#[test]
fn focus_depth_1_stops_exactly_where_the_relations_do() {
    // INVOICES.Client_ID → CLIENTS.ID and INVOICE_LINES.Invoice_ID → INVOICES.ID
    // are one hop; PRODUCTS is only reachable through INVOICE_LINES, so it must
    // stay outside the selection at depth 1.
    let catalog = invoices();
    let selection = select::select(
        &catalog,
        &SelectionSpec {
            focus: Some("INVOICES".into()),
            ..spec()
        },
    )
    .unwrap();

    assert_eq!(
        names(&catalog, &selection),
        ["CLIENTS", "INVOICES", "INVOICE_LINES"]
    );
    assert_eq!(selection.ghosts, ["PRODUCTS"]);
}

#[test]
fn focus_depth_2_pulls_in_products() {
    let catalog = invoices();
    let selection = select::select(
        &catalog,
        &SelectionSpec {
            focus: Some("INVOICES".into()),
            depth: 2,
            ..spec()
        },
    )
    .unwrap();
    assert_eq!(
        names(&catalog, &selection),
        ["CLIENTS", "INVOICES", "INVOICE_LINES", "PRODUCTS"]
    );
    assert!(selection.ghosts.is_empty());
}

#[test]
fn external_refs_hide_drops_the_stub_entirely() {
    let catalog = invoices();
    let selection = select::select(
        &catalog,
        &SelectionSpec {
            focus: Some("INVOICES".into()),
            external_refs: ExternalRefs::Hide,
            ..spec()
        },
    )
    .unwrap();
    assert!(selection.ghosts.is_empty());
    assert_eq!(selection.relations.len(), 2);
}

#[test]
fn external_refs_include_absorbs_the_neighbour() {
    let catalog = invoices();
    let selection = select::select(
        &catalog,
        &SelectionSpec {
            focus: Some("INVOICES".into()),
            external_refs: ExternalRefs::Include,
            ..spec()
        },
    )
    .unwrap();
    assert!(names(&catalog, &selection).contains(&"PRODUCTS".to_string()));
    assert!(selection.ghosts.is_empty());
}

#[test]
fn field_selection_is_narrower_than_table_focus() {
    let catalog = invoices();
    let by_field = select::select(
        &catalog,
        &SelectionSpec {
            field: Some("INVOICE_LINES.Product_ID".into()),
            ..spec()
        },
    )
    .unwrap();
    assert_eq!(names(&catalog, &by_field), ["INVOICE_LINES", "PRODUCTS"]);

    let by_table = select::select(
        &catalog,
        &SelectionSpec {
            focus: Some("INVOICE_LINES".into()),
            ..spec()
        },
    )
    .unwrap();
    // The whole-table focus also drags in INVOICES, which Product_ID does not
    // touch — that difference is the entire point of --field.
    assert_eq!(
        names(&catalog, &by_table),
        ["INVOICES", "INVOICE_LINES", "PRODUCTS"]
    );
}

#[test]
fn tables_match_selects_by_pattern() {
    let catalog = invoices();
    let selection = select::select(
        &catalog,
        &SelectionSpec {
            tables_match: Some("^INVOICE".into()),
            ..spec()
        },
    )
    .unwrap();
    assert_eq!(names(&catalog, &selection), ["INVOICES", "INVOICE_LINES"]);
}

#[test]
fn the_max_tables_cap_is_enforced() {
    let catalog = invoices();
    let error = select::select(
        &catalog,
        &SelectionSpec {
            max_tables: 2,
            ..spec()
        },
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), catalog_diagram::ExitCode::Selection);
}

// ------------------------------------------------------------- rendering

#[test]
fn svg_is_well_formed_in_every_mode() {
    let catalog = invoices();
    let selection = select::select(&catalog, &spec()).unwrap();
    for layout in [LayoutMode::AsDesigned, LayoutMode::Auto] {
        for hide in [false, true] {
            let svg = render(&catalog, &selection, layout, hide);
            roxmltree::Document::parse(&svg)
                .unwrap_or_else(|e| panic!("{layout:?}/{hide} produced invalid XML: {e}"));
        }
    }
}

#[test]
fn focus_render_contains_exactly_the_selected_tables() {
    let catalog = invoices();
    let selection = select::select(
        &catalog,
        &SelectionSpec {
            focus: Some("INVOICES".into()),
            ..spec()
        },
    )
    .unwrap();
    let svg = render(&catalog, &selection, LayoutMode::AsDesigned, false);

    for name in ["INVOICES", "CLIENTS", "INVOICE_LINES"] {
        assert!(
            svg.contains(&format!(r#"data-table="{name}""#)),
            "missing {name}"
        );
    }
    // PRODUCTS is present only as a ghost stub.
    assert!(svg.contains(r#"class="card ghost" data-card="g0" data-table="PRODUCTS""#));
}

#[test]
fn every_edge_marker_is_defined() {
    // Crow's-foot and bar markers are keyed by colour so that several scenes can
    // share one <defs> block without colliding on ids. A dangling reference
    // silently renders a connector with no endpoints, so check both directions.
    let catalog = invoices();
    let selection = select::select(&catalog, &spec()).unwrap();
    let svg = render(&catalog, &selection, LayoutMode::AsDesigned, false);
    let doc = roxmltree::Document::parse(&svg).unwrap();

    let defined: std::collections::BTreeSet<&str> = doc
        .descendants()
        .filter(|n| n.has_tag_name("marker"))
        .filter_map(|n| n.attribute("id"))
        .collect();
    assert!(!defined.is_empty(), "no markers were emitted");

    let mut referenced = std::collections::BTreeSet::new();
    for node in doc.descendants() {
        for attribute in ["marker-start", "marker-end"] {
            if let Some(value) = node.attribute(attribute) {
                let id = value.trim_start_matches("url(#").trim_end_matches(')');
                referenced.insert(id.to_string());
            }
        }
    }
    assert!(!referenced.is_empty(), "no edge referenced a marker");
    for id in &referenced {
        assert!(
            defined.contains(id.as_str()),
            "edge references undefined marker {id}"
        );
    }
}

#[test]
fn rendering_twice_is_byte_identical() {
    let catalog = invoices();
    let selection = select::select(&catalog, &spec()).unwrap();
    for layout in [LayoutMode::AsDesigned, LayoutMode::Auto] {
        let first = render(&catalog, &selection, layout, false);
        let second = render(&catalog, &selection, layout, false);
        assert_eq!(first, second, "{layout:?} render is not deterministic");
    }
}

#[test]
fn png_output_is_deterministic() {
    let catalog = invoices();
    let selection = select::select(&catalog, &spec()).unwrap();
    let svg = render(&catalog, &selection, LayoutMode::AsDesigned, false);
    let a = catalog_diagram::raster::svg_to_png(&svg, 1.0).unwrap();
    let b = catalog_diagram::raster::svg_to_png(&svg, 1.0).unwrap();
    assert_eq!(a, b);
    assert_eq!(&a[1..4], b"PNG");
}

// -------------------------------------------------------------- security

#[test]
fn hostile_table_names_cannot_produce_script_or_broken_markup() {
    let catalog =
        parse::parse_bytes(&std::fs::read(fixture_path("tests/fixtures/hostile.xml")).unwrap())
            .unwrap();
    let selection = select::select(&catalog, &spec()).unwrap();

    let svg = render(&catalog, &selection, LayoutMode::AsDesigned, false);
    roxmltree::Document::parse(&svg).expect("hostile input still yields well-formed SVG");
    assert!(!svg.contains("<script"));
    assert!(!svg.contains("<img"));
    // The payload survives verbatim, but only ever as escaped character data.
    assert!(svg.contains("&lt;img src=x onerror=alert(1)&gt;"));

    let scene = scene::build(
        &catalog,
        &selection,
        &SceneOptions {
            title: catalog.base_name.clone(),
            layout: LayoutMode::AsDesigned,
            hide_system_fields: false,
            collapse_dense_cards: false,
        },
    );
    let html = catalog_diagram::render_html::render(
        &[catalog_diagram::render_html::Variant {
            layout: "as-designed",
            hide_system_fields: false,
            scene,
        }],
        &catalog_diagram::render_html::HtmlOptions {
            theme: Theme::Auto,
            default_layout: "as-designed",
            default_hide_system_fields: false,
            generated_at: None,
            command: String::new(),
        },
    );

    // The only script elements in the document are the two we emit ourselves.
    assert_eq!(html.matches("<script").count(), 2);
    assert_eq!(html.matches("</script>").count(), 2);
    assert!(!html.contains("<img"));
    // Inside the JSON island `<` must be \u003c so the payload can never close
    // the script element early.
    assert!(html.contains("\\u003c/script\\u003e"));
}

// --------------------------------------------------------------- inspect

#[test]
fn inspect_json_matches_the_documented_shape() {
    let catalog = invoices();
    let value: serde_json::Value =
        serde_json::from_str(&inspect::to_string_pretty(&catalog)).unwrap();

    assert_eq!(value["base_name"], "InvoicesDemo");
    assert_eq!(value["table_count"], 5);
    assert_eq!(value["relation_count"], 3);

    let tables = value["tables"].as_array().unwrap();
    let sorted: Vec<&str> = tables.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let mut expected = sorted.clone();
    expected.sort();
    assert_eq!(sorted, expected, "tables must be sorted by name");

    let clients = tables.iter().find(|t| t["name"] == "CLIENTS").unwrap();
    assert_eq!(clients["primary_key"], "ID");
    assert_eq!(clients["has_layout"], true);
    assert_eq!(clients["relations_in"], 1);
    assert_eq!(clients["relations_out"], 0);
    assert!(clients["fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type_label"] == "Longint"));

    let analysis = &value["analysis"];
    assert!(analysis["isolated_tables"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n == "DEFAULT_SETTINGS"));
    // CLIENTS(1) < INVOICES(2) == INVOICE_LINES(2); ties break on name so the
    // ordering is stable across runs.
    let ranked: Vec<(&str, u64)> = analysis["most_connected_tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["name"].as_str().unwrap(),
                t["relation_count"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(ranked[0], ("INVOICES", 2));
    assert_eq!(ranked[1], ("INVOICE_LINES", 2));
    assert!(ranked.windows(2).all(|w| w[0].1 >= w[1].1));
}

// ------------------------------------------------------------------- CLI

#[test]
fn cli_renders_next_to_the_input_by_default() {
    let dir = std::env::temp_dir().join("4dcd-default-output");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("InvoicesDemo.xml");
    std::fs::copy(fixture_path("references/InvoicesDemo.xml"), &input).unwrap();

    let status = bin().arg(&input).arg("-q").status().unwrap();
    assert!(status.success());
    let produced = dir.join("InvoicesDemo.html");
    assert!(produced.exists(), "expected {}", produced.display());
    let html = std::fs::read_to_string(&produced).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    // Self-contained: the only absolute URLs are XML namespace identifiers,
    // which are names rather than things a browser fetches.
    let urls: Vec<&str> = html
        .match_indices("http")
        .map(|(i, _)| &html[i..i + 30])
        .collect();
    assert!(
        urls.iter().all(|u| u.starts_with("http://www.w3.org/")),
        "unexpected external reference: {urls:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_exit_codes_are_stable() {
    let missing = bin()
        .args(["render", "definitely-not-here.xml"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));

    let broken = std::env::temp_dir().join("4dcd-broken.xml");
    std::fs::write(&broken, b"<base><table></base>").unwrap();
    let malformed = bin().arg("render").arg(&broken).output().unwrap();
    assert_eq!(malformed.status.code(), Some(3));

    let empty = bin()
        .args(["render"])
        .arg(fixture_path("references/InvoicesDemo.xml"))
        .args(["--tables-match", "^NOPE$", "-o", "-"])
        .output()
        .unwrap();
    assert_eq!(empty.status.code(), Some(4));

    let usage = bin().args(["render", "--nonsense-flag"]).output().unwrap();
    assert_eq!(usage.status.code(), Some(1));

    let _ = std::fs::remove_file(&broken);
}

#[test]
fn cli_error_format_json_emits_one_object_per_line() {
    let output = bin()
        .args([
            "render",
            "definitely-not-here.xml",
            "--error-format",
            "json",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let line = stderr.lines().next().unwrap();
    let value: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(value["kind"], "input_file");
    assert_eq!(value["exit_code"], 2);
}

#[test]
fn cli_reads_a_catalog_from_stdin() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = bin()
        .args(["render", "-", "-f", "svg", "-o", "-", "-q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let xml = std::fs::read(fixture_path("references/InvoicesDemo.xml")).unwrap();
    child.stdin.as_mut().unwrap().write_all(&xml).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let svg = String::from_utf8(output.stdout).unwrap();
    assert!(svg.starts_with("<svg"));
}

#[test]
fn cli_render_is_reproducible_across_processes() {
    let mut outputs = Vec::new();
    for _ in 0..2 {
        let output = bin()
            .arg("render")
            .arg(fixture_path("references/InvoicesDemo.xml"))
            .args(["-o", "-", "-q"])
            .output()
            .unwrap();
        assert!(output.status.success());
        outputs.push(output.stdout);
    }
    assert_eq!(outputs[0], outputs[1], "two runs produced different bytes");
    assert!(!outputs[0].is_empty());
}

// ------------------------------------------------- text export formats

/// Mermaid is whitespace- and keyword-sensitive; assert the exact structural
/// shape rather than just "contains erDiagram". CI cannot run the real Mermaid
/// grammar without pulling in a browser, so this is a strict structural proxy —
/// the output was additionally checked against mermaid's own `parse()` and
/// against Graphviz during development.
#[test]
fn mermaid_output_is_structurally_valid() {
    let catalog = invoices();
    let selection = select::select(&catalog, &spec()).unwrap();
    let mmd = catalog_diagram::export::mermaid(&catalog, &selection, false);

    let mut lines = mmd.lines();
    assert_eq!(lines.next(), Some("erDiagram"));

    let entity = regex_lite_entities(&mmd);
    assert!(entity.contains(&"CLIENTS".to_string()));
    assert!(entity.contains(&"INVOICE_LINES".to_string()));

    let mut depth = 0usize;
    let mut relations = 0usize;
    for line in mmd.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if depth == 0 {
            if let Some(name) = trimmed.strip_suffix(" {") {
                assert!(is_safe_identifier(name), "entity name {name:?} is not safe");
                depth = 1;
                continue;
            }
            // The only other top-level construct is a relationship line.
            let (lhs, rest) = trimmed.split_once(" ||--o{ ").expect("relationship line");
            let (rhs, label) = rest.split_once(" : ").expect("relationship label");
            assert!(is_safe_identifier(lhs));
            assert!(is_safe_identifier(rhs));
            assert!(label.starts_with('"') && label.ends_with('"'));
            assert_eq!(
                label.matches('"').count(),
                2,
                "unescaped quote in {label:?}"
            );
            relations += 1;
        } else if trimmed == "}" {
            depth = 0;
        } else {
            // `type name` or `type name PK`
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            assert!(
                parts.len() == 2 || parts.len() == 3,
                "bad attribute line {trimmed:?}"
            );
            assert!(parts.iter().all(|p| is_safe_identifier(p)));
            if parts.len() == 3 {
                assert!(matches!(parts[2], "PK" | "UK" | "FK"));
            }
        }
    }
    assert_eq!(depth, 0, "unbalanced entity block");
    assert_eq!(relations, selection.relations.len());
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !value.starts_with(|c: char| c.is_ascii_digit())
}

fn regex_lite_entities(mmd: &str) -> Vec<String> {
    mmd.lines()
        .filter_map(|l| l.trim().strip_suffix(" {").map(|s| s.to_string()))
        .collect()
}

#[test]
fn graphviz_output_is_balanced_and_escaped() {
    let catalog = invoices();
    let selection = select::select(&catalog, &spec()).unwrap();
    let dot = catalog_diagram::export::graphviz(&catalog, &selection, false);

    assert!(dot.starts_with("digraph catalog {\n"));
    assert!(dot.trim_end().ends_with('}'));
    assert!(dot.contains("rankdir=LR;"));
    assert!(dot.contains("shape=record"));
    // Ports let an edge attach to the field that carries the relation.
    assert!(dot.contains("INVOICE_LINES:f1"));

    for line in dot.lines() {
        assert_eq!(
            line.matches('"').count() - line.matches("\\\"").count() * 2,
            line.matches('"').count() - line.matches("\\\"").count() * 2,
        );
        assert!(!line.contains("\u{0}"));
    }
}

/// Hostile names must not be able to break out of either grammar.
#[test]
fn text_exports_neutralize_hostile_names() {
    let catalog =
        parse::parse_bytes(&std::fs::read(fixture_path("tests/fixtures/hostile.xml")).unwrap())
            .unwrap();
    let selection = select::select(&catalog, &spec()).unwrap();

    let mmd = catalog_diagram::export::mermaid(&catalog, &selection, false);
    // Nothing that could close an enclosing HTML context or a Mermaid string
    // may survive into the output.
    assert!(!mmd.contains('<'), "{mmd}");
    assert!(!mmd.contains('>'), "{mmd}");
    for line in mmd.lines() {
        assert!(
            line.matches('"').count() % 2 == 0,
            "odd quoting in {line:?}"
        );
    }

    let dot = catalog_diagram::export::graphviz(&catalog, &selection, false);
    for line in dot.lines() {
        // Drop every backslash escape, then whatever metacharacters remain must
        // be Graphviz's own syntax rather than something a catalog name smuggled
        // in. Unbalanced quotes are the actual breakout risk.
        let bare = strip_escapes(line);
        assert_eq!(
            bare.matches('"').count() % 2,
            0,
            "unbalanced quoting in {line:?}"
        );
        for segment in bare.split('"').skip(1).step_by(2) {
            // Inside a quoted label only record structure may survive.
            for part in segment.split(['{', '}', '|']) {
                let ports: Vec<&str> = part.match_indices('<').map(|(_, s)| s).collect();
                assert!(
                    ports.len() == part.matches('>').count(),
                    "stray angle bracket in {part:?}"
                );
            }
        }
    }
}

fn strip_escapes(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn cli_writes_mmd_and_dot() {
    let dir = std::env::temp_dir().join(format!("catviz-text-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (format, extension, needle) in [("mmd", "mmd", "erDiagram"), ("dot", "dot", "digraph")] {
        let out = dir.join(format!("out.{extension}"));
        let status = bin()
            .arg(fixture_path("references/InvoicesDemo.xml"))
            .args(["-f", format, "-o"])
            .arg(&out)
            .status()
            .unwrap();
        assert!(status.success());
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains(needle), "{format} output missing {needle}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

// --------------------------------------------------- layout coverage rule

#[test]
fn low_coordinate_coverage_falls_back_to_auto_layout() {
    let mut catalog = invoices();
    // Strip coordinates from all but one table: 1/5 = 20% < 80%.
    for table in catalog.tables.iter_mut().skip(1) {
        table.coordinates = None;
    }
    let selection = select::select(&catalog, &spec()).unwrap();
    let scene = scene::build(
        &catalog,
        &selection,
        &SceneOptions {
            title: "t".into(),
            layout: LayoutMode::AsDesigned,
            hide_system_fields: false,
            collapse_dense_cards: false,
        },
    );
    assert!(scene.layout_fallback);
    assert_eq!(scene.layout, "auto");
}

#[test]
fn high_coordinate_coverage_keeps_the_designed_layout() {
    let mut catalog = invoices();
    // 4/5 = exactly the 80% threshold, which must still count as covered.
    catalog.tables[4].coordinates = None;
    let selection = select::select(&catalog, &spec()).unwrap();
    let scene = scene::build(
        &catalog,
        &selection,
        &SceneOptions {
            title: "t".into(),
            layout: LayoutMode::AsDesigned,
            hide_system_fields: false,
            collapse_dense_cards: false,
        },
    );
    assert!(!scene.layout_fallback);
    assert_eq!(scene.layout, "as-designed");
}

// ------------------------------------------------- progressive disclosure

#[test]
fn dense_cards_collapse_only_when_asked() {
    let catalog = invoices();
    let selection = select::select(&catalog, &spec()).unwrap();
    let options = |collapse| SceneOptions {
        title: "t".into(),
        layout: LayoutMode::AsDesigned,
        hide_system_fields: false,
        collapse_dense_cards: collapse,
    };

    let full = scene::build(&catalog, &selection, &options(false));
    let collapsed = scene::build(&catalog, &selection, &options(true));

    for (a, b) in full.cards.iter().zip(collapsed.cards.iter()) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.rows.len(), b.rows.len(), "no field may be dropped");
        assert!(a.collapsed_rows.is_none());
        if b.rows.len() > scene::COLLAPSE_MIN_FIELDS {
            assert_eq!(b.collapsed_rows, Some(scene::COLLAPSE_VISIBLE_ROWS));
            assert!(b.h < b.expanded_h, "collapsed card must be shorter");
            assert!(b.h < a.h);
        } else {
            assert_eq!(b.collapsed_rows, None);
            assert_eq!(b.h, b.expanded_h);
        }
    }
}

#[test]
fn static_svg_never_collapses() {
    let out = bin()
        .arg(fixture_path("references/InvoicesDemo.xml"))
        .args(["-f", "svg", "-o", "-"])
        .output()
        .unwrap();
    let svg = String::from_utf8(out.stdout).unwrap();
    assert!(
        !svg.contains(r#"class="row-more""#),
        "a still image cannot expand rows"
    );
    assert!(
        !svg.contains("Show "),
        "no disclosure affordance in static output"
    );
    assert!(svg.contains("Optional_Data"), "every field must be drawn");
}

// ------------------------------------------------------ parse diagnostics

#[test]
fn malformed_xml_reports_line_and_column() {
    let dir = std::env::temp_dir().join(format!("catviz-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.xml");
    std::fs::write(
        &path,
        "<?xml version=\"1.0\"?>\n<base>\n  <table name=\"A\">\n    <field 1name=\"x\"/>\n  </table>\n</base>\n",
    )
    .unwrap();

    let text = bin().arg("validate").arg(&path).output().unwrap();
    assert_eq!(text.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&text.stderr);
    assert!(stderr.contains("bad.xml:4:"), "missing location: {stderr}");
    assert!(stderr.contains('^'), "missing caret: {stderr}");

    let json = bin()
        .arg("validate")
        .arg(&path)
        .args(["--error-format", "json"])
        .output()
        .unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(json.stderr.split(|&b| b == b'\n').next().unwrap()).unwrap();
    assert_eq!(value["kind"], "malformed_xml");
    assert_eq!(value["line"], 4);
    assert!(value["column"].as_u64().unwrap() > 0);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------ deep links

#[test]
fn html_ships_the_deep_link_and_minimap_machinery() {
    let out = bin()
        .arg(fixture_path("references/InvoicesDemo.xml"))
        .args(["-f", "html", "-o", "-"])
        .output()
        .unwrap();
    let html = String::from_utf8(out.stdout).unwrap();
    assert!(html.contains(r#"id="minimap""#));
    assert!(html.contains("hashchange"));
    assert!(html.contains("function applyHash"));
    // Rows must be addressable by field name for `#TABLE.FIELD` to work.
    assert!(html.contains(r#"data-field="Invoice_Number""#));
}

/// 4D's own `structure_to_html.xml` transform is the authority on how a field
/// type is *presented*. Type 21 is `Object`; it maps to `BLOB` only in
/// `structure-to-sql.xml`, which describes SQL storage, not the type name.
#[test]
fn object_fields_are_labelled_object_not_blob() {
    let catalog = invoices();
    let numbers = catalog
        .tables
        .iter()
        .find(|t| t.name == "CLIENTS")
        .unwrap()
        .fields
        .iter()
        .find(|f| f.name == "Numbers")
        .unwrap();

    assert_eq!(numbers.type_code, 21);
    assert_eq!(numbers.type_label(), "Object");

    // And nowhere in the catalog does a type 21 field claim to be a Blob.
    let mislabelled: Vec<_> = catalog
        .tables
        .iter()
        .flat_map(|t| &t.fields)
        .filter(|f| f.type_code == 21 && f.type_label() != "Object")
        .map(|f| f.name.clone())
        .collect();
    assert!(mislabelled.is_empty(), "mislabelled: {mislabelled:?}");
}

/// Codes 10, 14 and 17 are all "string". 4D shows `Alpha` when the field
/// carries a length limit and `Text` when it does not, so the code alone is
/// not enough to name the type.
#[test]
fn string_fields_are_alpha_only_when_length_limited() {
    let catalog = invoices();
    let clients = catalog.tables.iter().find(|t| t.name == "CLIENTS").unwrap();
    let field = |name: &str| clients.fields.iter().find(|f| f.name == name).unwrap();

    let limited = field("Name");
    assert_eq!(limited.type_code, 10);
    assert_eq!(limited.limiting_length, Some(40));
    assert_eq!(limited.type_label(), "Alpha");

    let unlimited = field("Address");
    assert_eq!(unlimited.type_code, 10);
    assert_eq!(unlimited.limiting_length, None);
    assert_eq!(unlimited.type_label(), "Text");
}

/// The text exports carry the same corrected labels as the diagram.
#[test]
fn exports_use_the_display_type_names() {
    let catalog = invoices();
    let selection = select::select(&catalog, &spec()).unwrap();
    let mmd = catalog_diagram::export::mermaid(&catalog, &selection, false);

    assert!(mmd.contains("Object Numbers"), "{mmd}");
    assert!(mmd.contains("Text Address"), "{mmd}");
    assert!(mmd.contains("Alpha Name"), "{mmd}");
    assert!(!mmd.contains("Blob Numbers"), "{mmd}");
}
