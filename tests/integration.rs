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
