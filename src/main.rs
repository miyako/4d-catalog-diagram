//! CLI entry point.
//!
//! The stdout/stderr/exit-code contract here is treated as an API: an agent
//! calls this non-interactively and branches on the exit code (see
//! BUILD-SPEC §5). Exit codes: 0 success, 1 usage, 2 input file, 3 malformed
//! XML, 4 selection, 5 render.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

use catalog_diagram::error::{AppError, ExitCode, Result};
use catalog_diagram::layout::LayoutMode;
use catalog_diagram::model::Catalog;
use catalog_diagram::render_html::{self, HtmlOptions, Variant};
use catalog_diagram::render_svg::{self, Theme};
use catalog_diagram::scene::{self, SceneOptions};
use catalog_diagram::select::{self, ExternalRefs, SelectionSpec};
use catalog_diagram::{inspect, parse, raster};

const ABOUT: &str = "Render a 4D database catalog (structure XML) as an interactive HTML diagram, a standalone SVG, or a PNG.";

const AFTER_HELP: &str = "\
EXAMPLES:
  4d-catalog-diagram InvoicesDemo.xml
      Render the whole catalog to InvoicesDemo.html next to the input.

  4d-catalog-diagram inspect InvoicesDemo.xml
      Print the catalog as JSON without rendering anything.

  4d-catalog-diagram render InvoicesDemo.xml --focus INVOICES --depth 1 -o out.svg
      Render INVOICES and everything one relation-hop away.

LAYOUT:
  --layout as-designed reuses the position each table was given in 4D's own
  structure editor. Card *size* is always recomputed from this tool's own text
  metrics, never taken from the XML. Tables that carry no stored coordinates
  are auto-placed into free space below the ones that do.

EXIT CODES:
  0 success   1 usage   2 input file   3 malformed XML   4 selection   5 render";

#[derive(Parser, Debug)]
#[command(
    name = "4d-catalog-diagram",
    version,
    about = ABOUT,
    after_help = AFTER_HELP,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Catalog XML file, or `-` to read from stdin.
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,

    #[command(flatten)]
    render: RenderArgs,

    #[command(flatten)]
    global: GlobalArgs,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Render a catalog (or a subset of it) to SVG, PNG, or HTML.
    Render {
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        #[command(flatten)]
        render: RenderArgs,
        #[command(flatten)]
        global: GlobalArgs,
    },
    /// Parse a catalog and print its structure as JSON. No rendering.
    Inspect {
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        /// Reserved for future formats; only `json` is supported today.
        #[arg(long, default_value = "json")]
        format: String,
        #[command(flatten)]
        global: GlobalArgs,
    },
    /// Parse a catalog and report warnings. Exits 0 if it is parseable.
    Validate {
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        #[command(flatten)]
        global: GlobalArgs,
    },
}

#[derive(Args, Debug, Clone)]
struct GlobalArgs {
    /// `json` emits machine-readable diagnostics on stderr, one object per line.
    #[arg(long, value_enum, default_value_t = ErrorFormat::Text, global = true)]
    error_format: ErrorFormat,

    /// Suppress non-error chatter.
    #[arg(short, long, action = ArgAction::SetTrue, global = true)]
    quiet: bool,
}

#[derive(Args, Debug, Clone)]
struct RenderArgs {
    /// Output file. Defaults to <input-stem>.<ext> next to the input. `-` writes
    /// svg/html to stdout.
    #[arg(short, long, value_name = "PATH")]
    output: Option<PathBuf>,

    #[arg(short, long, value_enum, default_value_t = Format::Html)]
    format: Format,

    /// Explicit table allow-list, comma separated.
    #[arg(long, value_name = "CSV")]
    tables: Option<String>,

    /// Regex allow-list over table names.
    #[arg(long, value_name = "REGEX")]
    tables_match: Option<String>,

    /// Centre a neighbourhood selection on this table.
    #[arg(long, value_name = "TABLE")]
    focus: Option<String>,

    /// Focus on one field: its table plus tables joined through that field.
    #[arg(long, value_name = "TABLE.FIELD")]
    field: Option<String>,

    /// Hop count for --focus/--field.
    #[arg(long, default_value_t = 1)]
    depth: usize,

    #[arg(long, value_enum, default_value_t = ExternalRefsArg::Ghost)]
    external_refs: ExternalRefsArg,

    /// Drop 4D-internal (visible="false") fields entirely.
    #[arg(long, action = ArgAction::SetTrue)]
    hide_system_fields: bool,

    #[arg(long, value_enum, default_value_t = LayoutArg::AsDesigned)]
    layout: LayoutArg,

    /// Defaults to `auto` for html and `light` for svg/png.
    #[arg(long, value_enum)]
    theme: Option<ThemeArg>,

    /// Overrides the displayed diagram title.
    #[arg(long, value_name = "STRING")]
    title: Option<String>,

    /// PNG raster scale factor.
    #[arg(long, default_value_t = 2.0)]
    scale: f64,

    /// Safety cap on the number of selected tables.
    #[arg(long, default_value_t = 300)]
    max_tables: usize,

    /// Embed a generation timestamp. Deliberately breaks byte-for-byte
    /// reproducibility, so it is off by default.
    #[arg(long, action = ArgAction::SetTrue)]
    embed_timestamp: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    Svg,
    Png,
    Html,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ErrorFormat {
    Text,
    Json,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ExternalRefsArg {
    Ghost,
    Hide,
    Include,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum LayoutArg {
    AsDesigned,
    Auto,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ThemeArg {
    Light,
    Dark,
    Auto,
}

fn main() {
    // `4d-catalog-diagram foo.xml` and `4d-catalog-diagram render foo.xml` are
    // equivalent: clap resolves the positional form when the first argument is
    // not a known subcommand.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            let code = match err.kind() {
                clap::error::ErrorKind::DisplayHelp
                | clap::error::ErrorKind::DisplayVersion
                | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
                    ExitCode::Success
                }
                _ => ExitCode::Usage,
            };
            std::process::exit(code as i32);
        }
    };

    let error_format = current_error_format(&cli);
    let quiet = current_quiet(&cli);

    match run(cli) {
        Ok(warnings) => {
            if !quiet {
                emit_warnings(&warnings, error_format);
            }
        }
        Err(err) => {
            match error_format {
                ErrorFormat::Json => eprintln!("{}", err.to_json_line()),
                ErrorFormat::Text => eprintln!("error: {err}"),
            }
            std::process::exit(err.exit_code() as i32);
        }
    }
}

fn current_error_format(cli: &Cli) -> ErrorFormat {
    match &cli.command {
        Some(Command::Render { global, .. })
        | Some(Command::Inspect { global, .. })
        | Some(Command::Validate { global, .. }) => global.error_format,
        None => cli.global.error_format,
    }
}

fn current_quiet(cli: &Cli) -> bool {
    match &cli.command {
        Some(Command::Render { global, .. })
        | Some(Command::Inspect { global, .. })
        | Some(Command::Validate { global, .. }) => global.quiet,
        None => cli.global.quiet,
    }
}

fn emit_warnings(warnings: &[String], format: ErrorFormat) {
    for warning in warnings {
        match format {
            ErrorFormat::Json => {
                let line = serde_json::json!({ "level": "warning", "message": warning });
                eprintln!("{line}");
            }
            ErrorFormat::Text => eprintln!("warning: {warning}"),
        }
    }
}

fn run(cli: Cli) -> Result<Vec<String>> {
    match cli.command {
        Some(Command::Inspect { input, format, .. }) => {
            if format != "json" {
                return Err(AppError::Usage(format!(
                    "inspect only supports --format json, got {format:?}"
                )));
            }
            let catalog = load(&input)?;
            println!("{}", inspect::to_string_pretty(&catalog));
            Ok(Vec::new())
        }
        Some(Command::Validate { input, global }) => {
            let catalog = load(&input)?;
            if !global.quiet {
                println!(
                    "{}: {} tables, {} relations, {} indexes, {} warnings",
                    catalog.base_name,
                    catalog.tables.len(),
                    catalog.relations.len(),
                    catalog.indexes.len(),
                    catalog.warnings.len()
                );
            }
            Ok(catalog.warnings.clone())
        }
        Some(Command::Render {
            input,
            render,
            global,
        }) => do_render(&input, &render, &global),
        None => {
            let input = cli.input.ok_or_else(|| {
                AppError::Usage("an input catalog XML file is required".to_string())
            })?;
            do_render(&input, &cli.render, &cli.global)
        }
    }
}

fn load(input: &Path) -> Result<Catalog> {
    let bytes = if input == Path::new("-") {
        let mut buffer = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buffer)
            .map_err(|e| AppError::InputFile(format!("could not read stdin: {e}")))?;
        buffer
    } else {
        std::fs::read(input)
            .map_err(|e| AppError::InputFile(format!("could not read {}: {e}", input.display())))?
    };
    parse::parse_bytes(&bytes)
}

fn do_render(input: &Path, args: &RenderArgs, global: &GlobalArgs) -> Result<Vec<String>> {
    let catalog = load(input)?;

    let spec = SelectionSpec {
        tables: args
            .tables
            .as_deref()
            .map(|csv| {
                csv.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        tables_match: args.tables_match.clone(),
        focus: args.focus.clone(),
        field: args.field.clone(),
        depth: args.depth,
        external_refs: match args.external_refs {
            ExternalRefsArg::Ghost => ExternalRefs::Ghost,
            ExternalRefsArg::Hide => ExternalRefs::Hide,
            ExternalRefsArg::Include => ExternalRefs::Include,
        },
        max_tables: args.max_tables,
    };
    let selection = select::select(&catalog, &spec)?;

    let title = args
        .title
        .clone()
        .unwrap_or_else(|| catalog.base_name.clone());
    let layout = match args.layout {
        LayoutArg::AsDesigned => LayoutMode::AsDesigned,
        LayoutArg::Auto => LayoutMode::Auto,
    };
    let theme = args
        .theme
        .map(|t| match t {
            ThemeArg::Light => Theme::Light,
            ThemeArg::Dark => Theme::Dark,
            ThemeArg::Auto => Theme::Auto,
        })
        .unwrap_or(match args.format {
            Format::Html => Theme::Auto,
            _ => Theme::Light,
        });

    let output = resolve_output(input, args)?;

    let bytes: Vec<u8> = match args.format {
        Format::Html => {
            let variants = build_variants(&catalog, &selection, &title, args);
            render_html::render(
                &variants,
                &HtmlOptions {
                    theme,
                    default_layout: layout.as_str(),
                    default_hide_system_fields: args.hide_system_fields,
                    generated_at: args.embed_timestamp.then(timestamp),
                    command: command_line(),
                },
            )
            .into_bytes()
        }
        Format::Svg => {
            let scene = scene::build(
                &catalog,
                &selection,
                &SceneOptions {
                    title,
                    layout,
                    hide_system_fields: args.hide_system_fields,
                },
            );
            render_svg::render(&scene, theme).into_bytes()
        }
        Format::Png => {
            let scene = scene::build(
                &catalog,
                &selection,
                &SceneOptions {
                    title,
                    layout,
                    hide_system_fields: args.hide_system_fields,
                },
            );
            let svg = render_svg::render(&scene, theme);
            raster::svg_to_png(&svg, args.scale)?
        }
    };

    match &output {
        None => {
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| AppError::Render(format!("could not write to stdout: {e}")))?;
        }
        Some(path) => {
            std::fs::write(path, &bytes).map_err(|e| {
                AppError::Render(format!("could not write {}: {e}", path.display()))
            })?;
            if !global.quiet {
                eprintln!(
                    "wrote {} ({} table{}, {} relation{})",
                    path.display(),
                    selection.tables.len(),
                    if selection.tables.len() == 1 { "" } else { "s" },
                    selection.relations.len(),
                    if selection.relations.len() == 1 {
                        ""
                    } else {
                        "s"
                    },
                );
            }
        }
    }

    Ok(catalog.warnings)
}

/// Pre-build the layout / system-field variants the HTML toggles switch
/// between, so the viewer never has to re-run the CLI. Large diagrams only get
/// the variant that was asked for, to keep the file from ballooning.
fn build_variants(
    catalog: &Catalog,
    selection: &select::Selection,
    title: &str,
    args: &RenderArgs,
) -> Vec<Variant> {
    const VARIANT_CARD_BUDGET: usize = 120;

    let has_hidden_fields = selection
        .tables
        .iter()
        .any(|&i| catalog.tables[i].fields.iter().any(|f| f.hidden));
    let small_enough = selection.tables.len() <= VARIANT_CARD_BUDGET;

    let requested = match args.layout {
        LayoutArg::AsDesigned => LayoutMode::AsDesigned,
        LayoutArg::Auto => LayoutMode::Auto,
    };
    let layouts: Vec<LayoutMode> = if small_enough {
        vec![LayoutMode::AsDesigned, LayoutMode::Auto]
    } else {
        vec![requested]
    };
    let hide_states: Vec<bool> = if has_hidden_fields && small_enough {
        vec![false, true]
    } else {
        vec![args.hide_system_fields]
    };

    let mut variants = Vec::new();
    for layout in layouts {
        for &hide in &hide_states {
            variants.push(Variant {
                layout: layout.as_str(),
                hide_system_fields: hide,
                scene: scene::build(
                    catalog,
                    selection,
                    &SceneOptions {
                        title: title.to_string(),
                        layout,
                        hide_system_fields: hide,
                    },
                ),
            });
        }
    }
    variants
}

/// `None` means "write to stdout".
fn resolve_output(input: &Path, args: &RenderArgs) -> Result<Option<PathBuf>> {
    let extension = match args.format {
        Format::Html => "html",
        Format::Svg => "svg",
        Format::Png => "png",
    };
    match &args.output {
        Some(path) if path == Path::new("-") => {
            if args.format == Format::Png {
                Err(AppError::Usage(
                    "PNG output cannot be written to stdout; pass -o <path>".to_string(),
                ))
            } else {
                Ok(None)
            }
        }
        Some(path) => Ok(Some(path.clone())),
        None => {
            if input == Path::new("-") {
                return Err(AppError::Usage(
                    "reading from stdin requires an explicit -o/--output path".to_string(),
                ));
            }
            Ok(Some(input.with_extension(extension)))
        }
    }
}

fn command_line() -> String {
    std::env::args()
        .skip(1)
        .map(|a| if a.contains(' ') { format!("{a:?}") } else { a })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Only ever called behind `--embed-timestamp`. Formatted by hand to avoid a
/// date/time dependency for a strictly opt-in feature.
fn timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    let time_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "Generated {year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// Howard Hinnant's `civil_from_days`, for a Unix day number.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}
