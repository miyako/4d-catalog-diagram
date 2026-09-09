# Build Spec: `4d-catalog-diagram` — a static-binary visualizer for 4D database catalogs

Hand this entire document to a coding agent (or a human engineer) as the delegation
prompt for building the tool from scratch. It is self-contained: it explains the
input format, the required behavior, the CLI contract, the rendering requirements,
and the engineering constraints. Nothing else should need to be supplied except the
sample files referenced in "Reference fixtures" below.

---

## 1. What you're building

A single, self-contained, statically-linked command-line program named **`4d-catalog-diagram`**
that reads a **4D database "Structure" catalog** (an XML file describing tables,
fields, indexes, and relations) and renders it as either:

1. a **static image** (SVG, optionally rasterized to PNG), or
2. a **single-file interactive HTML document** (pan/zoom ER diagram, searchable,
   filterable, no external assets, no network calls, opens by double-click in any
   browser),

for the **whole catalog** or for **any user-selected subset** of it (a list of
tables, a regex, or a "neighborhood" of tables reachable within N relation-hops
of a given table).

This tool will be invoked by a coding agent (e.g. Claude Code, or Claude via a
markdown "skill") on the user's behalf, non-interactively, potentially many times
per session with different filters. Treat the CLI surface and stderr/stdout
contract as an API, not just a human convenience tool.

### Non-negotiable constraints

- **Language:** Rust (preferred) or Go. Do not use Python or Node.js anywhere in
  the shipped artifact or its runtime path (build-time tooling in the chosen
  language's own ecosystem is fine).
- **Distribution:** a single static executable per platform, **no dynamic
  library dependencies** beyond what the OS itself always provides (no libxml2,
  no cairo, no system Skia, no system fontconfig at runtime, no JVM, no
  interpreter). If you need XML parsing, SVG rasterization, or font shaping,
  vendor a pure-language crate/module and statically link it
  (e.g., in Rust: `quick-xml` or `roxmltree` for parsing, `resvg` + `tiny-skia`
  for headless SVG→PNG rasterization — both are pure Rust and support fully
  static linking; embed any font with `rust-embed`/`include_bytes!` rather than
  reading system fonts).
- **Cross-platform:** must build and run on Linux (x86_64 + aarch64), macOS
  (x86_64 + arm64, universal or per-arch), and Windows (x86_64). Prefer static
  linking with `musl` on Linux for maximum portability across distros.
- **No network access, ever.** No telemetry, no update checks, no font/CDN
  fetches. This must work fully air-gapped.
- **No files written except the requested output file(s).** No config directories,
  no caches, unless explicitly requested via a flag.
- **Deterministic output:** identical input file + identical flags ⇒
  byte-identical output (modulo an explicit `--embed-timestamp` opt-in). This
  matters because the tool will be called repeatedly by an agent and diffed.
- **Fast startup and fast rendering:** sub-100ms cold start for typical catalogs
  (tens of tables); catalogs with hundreds of tables should still render in low
  single-digit seconds.
- **Never crash on unexpected input.** Unknown XML elements/attributes must be
  ignored gracefully (this format has evolved across many 4D versions — see
  §2.4). A malformed or partially-unknown file should still render whatever it
  can, with a warning on stderr, not a hard failure — unless the file isn't
  well-formed XML at all, in which case fail clearly with a JSON-formatted error
  when `--error-format json` is set (see §6).

---

## 2. Input format: the 4D catalog XML

### 2.1 What this format is

4D (formerly "4th Dimension") is a relational database + application platform.
Its schema — tables, fields, indexes, relations — can be exported as an XML
document usually named `structure.xml` or similar, and referred to by the user
as a **"catalog"**. This is the sole input format for `4d-catalog-diagram`. There is no
formal published XSD; treat the structure below (reverse-engineered from a real
export, `InvoicesDemo.xml`, included as a fixture) as the authoritative shape,
and design the parser to be forward/backward tolerant per §2.4.

### 2.2 Document shape

```xml
<base name="InvoicesDemo" uuid="…" collation_locale="en">
  <schema name="DEFAULT_SCHEMA"/>

  <table name="CLIENTS" uuid="…" id="1">
    <field name="ID" uuid="…" type="4" unique="true" autosequence="true"
           never_null="true" id="1">
      <index_ref uuid="…"/>                 <!-- 0..N -->
      <field_extra modifiable="false" mandatory="true" enumeration_id="-1"
                    visible="true|false">
        <tip>Free-text description shown as a tooltip in the 4D editor</tip>
        <editor_field_info>
          <color red="0" green="158" blue="96" alpha="255"/>
        </editor_field_info>
      </field_extra>
    </field>
    <!-- more <field> … -->
    <primary_key field_name="ID" field_uuid="…"/>
    <table_extra trigger_insert="true" trigger_update="true">
      <editor_table_info displayable_fields_count="20">
        <color red="168" green="206" blue="226" alpha="255"/>
        <coordinates left="21.28" top="6.46" width="144" height="471.33"/>
      </editor_table_info>
    </table_extra>
  </table>
  <!-- more <table> … -->

  <relation uuid="…" name_Nto1="Product" name_1toN="Lines_Fm_Product"
            auto_load_Nto1="true" auto_load_1toN="true"
            foreign_key="false" state="1" integrity="reject">
    <related_field kind="source">
      <field_ref uuid="…" name="Product_ID">
        <table_ref uuid="…" name="INVOICE_LINES"/>
      </field_ref>
    </related_field>
    <related_field kind="destination">
      <field_ref uuid="…" name="ID">
        <table_ref uuid="…" name="PRODUCTS"/>
      </field_ref>
    </related_field>
    <relation_extra entry_wildchar="false" entry_create="false"
                    choice_field="0" entry_autofill="false">
      <editor_relation_info via_point_x="-1" via_point_y="-1"
                            prefers_left="false" smartlink="true">
        <color red="255" green="153" blue="63" alpha="255"/>
      </editor_relation_info>
    </relation_extra>
  </relation>
  <!-- more <relation> … -->

  <index kind="regular" unique_keys="true" uuid="…" type="7">
    <table_ref uuid="…" name="CLIENTS"/>
    <!-- indexes reference the fields that point to them via index_ref,
         not the other way around; join on uuid -->
  </index>
  <!-- more <index> … -->
</base>
```

Key relationships between elements (join keys are always `uuid`, never
position/order):

- A `<field>`'s `<index_ref uuid="X"/>` points to an `<index uuid="X">` at the
  document's top level.
- A `<relation>` connects exactly one **source** field (the "many" side, N) to
  one **destination** field (the "one" side, 1), each identified by
  `<field_ref uuid name>` nested in a `<table_ref uuid name>`. `name_Nto1` is
  the label 4D shows when navigating from the many side to the one side;
  `name_1toN` is the reverse-direction label.
- Every `<table>` and `<relation>` carries an **editor layout block**
  (`table_extra/editor_table_info/coordinates` and
  `relation_extra/editor_relation_info`) that is literally the x/y/width/height
  the human designer arranged in 4D's own graphical Structure editor, plus a
  colour and (for relations) routing hints (`via_point_x/y`, `prefers_left`,
  `smartlink`). **This is a gift: it means a faithful, already-legible layout
  exists in the source data and does not need to be computed.** See §4.2.

### 2.3 Field `type` code reference (best-effort; do not hard-fail on unknown codes)

Reverse-engineered from an existing XSLT stylesheet that ships alongside this
format. Treat as the built-in default mapping, expose it as data (not scattered
across code) so it's easy to extend, and **always** fall back to a generic
`"Type {n}"` label + neutral color for any code not in the table below rather
than erroring:

| code | label     | notes |
|-----:|-----------|-------|
| 1    | Boolean   | |
| 2    | Byte      | |
| 3    | Integer   | 16-bit |
| 4    | Longint   | 32-bit, common for IDs / primary keys |
| 5    | Long 64   | 64-bit |
| 6    | Real      | |
| 7    | Float     | |
| 8    | Date      | |
| 9    | Time      | |
| 10   | Alpha     | fixed/limited-length text; check `@limiting_length` |
| 11   | Blob      | |
| 12   | Image     | 4D calls this "Picture" |
| 14   | Alpha     | variant, same rendering as 10 |
| 17   | Text      | unlimited-length text |
| 18   | Blob      | variant, same rendering as 11 |
| 21   | Blob      | large object; often has `@blob_switch_size` |

Additional attributes worth surfacing in the UI (all optional, all boolean
unless noted):
`unique`, `autosequence`, `never_null`, `limiting_length` (numeric),
`field_extra/@mandatory`, `field_extra/@modifiable`, `field_extra/@visible`
(fields with `visible="false"` are 4D-internal/hidden fields — hide them behind
a toggle, don't drop them from the data model), `field_extra/tip` (free text —
show as a tooltip), `field_extra/editor_field_info/color` (the field's own
accent colour, distinct from the table's colour).

### 2.4 Forward/backward compatibility rules

- Parse permissively: unknown elements and attributes anywhere in the document
  must be silently ignored (store them nowhere, don't error).
  - This applies to `field_extra` and `table_extra` sub-elements that this spec
    doesn't call out — as long as the properties we know about are surviving,
    ignore the rest.
- Missing optional elements/attributes must produce sensible defaults, not
  panics: no `coordinates` block ⇒ that table has no known position and falls
  into auto-layout (§4.2); no `<tip>` ⇒ no tooltip; no `<primary_key>` ⇒ no
  key indicator.
- Never use an XML parser configuration that resolves external entities or
  fetches DTDs/schemas over network or filesystem (disable XXE by construction,
  not by hoping the library defaults are safe — verify and set explicitly).
  Some real-world exports carry a remote DOCTYPE, e.g.:
  ```xml
  <!DOCTYPE base SYSTEM "http://www.4d.com/dtd/2007/base.dtd">
  ```
  Parsing must succeed identically whether or not that URL is reachable —
  the DTD must never be fetched, and its absence/unreachability must never
  cause a failure or even a delay. Test this explicitly with network access
  disabled in CI.
- Handle Windows-1252/UTF-8 BOM edge cases some older 4D exports have; detect
  encoding declared in the XML prolog and honor it.
- **Every string that originates from the XML (table names, field names,
  relation names, tips/comments, schema names) is untrusted input** and must
  be correctly escaped wherever it's written into generated output — HTML text
  nodes, HTML attributes, inline `<script>` JSON payloads, and SVG text/attrs
  all have different escaping rules; use each target format's own escaping
  primitives rather than a single ad-hoc string-replace. A table literally
  named `<script>alert(1)</script>` (or containing `"`, `</script>`,
  `javascript:` in a comment, etc.) must never result in executable script or
  broken markup in the rendered output. This must have a dedicated test
  fixture and be treated as a security requirement, not a formatting nicety.

---

## 3. Selecting a subset ("in part or full")

The tool must support rendering:

1. **The whole catalog** (default).
2. **An explicit table list**: `--tables CLIENTS,INVOICES,PRODUCTS`.
3. **A pattern**: `--tables-match '^INVOICE'` (regex over table names).
4. **A neighborhood**: `--focus INVOICES --depth 2` → the named table plus every
   table reachable by following relations (either direction) up to `depth`
   hops. `--depth 0` means just that one table. This is the mode an agent will
   reach for most often ("show me how INVOICES relates to things").
5. Any combination of the above is unioned into one working set of tables.
6. Relations between two tables that are *both* outside the selected set are
   dropped; relations where exactly one endpoint is outside the set are, by
   default, drawn as a stub edge to a small "ghost" node labeled with the
   external table's name (so the viewer knows a connection exists without
   pulling in the whole catalog) — controllable with
   `--external-refs=ghost|hide|include`.
7. `--hide-system-fields` (default: fields with `field_extra/@visible="false"`
   are shown, but visually muted; this flag hides them entirely).
8. A hard safety cap `--max-tables` (default: 300) — if the selected set exceeds
   it, refuse with a clear error suggesting `--focus`/`--tables-match`, rather
   than silently producing a useless giant render.

Provide an **`inspect`** subcommand (see §6.1) that dumps the parsed catalog as
JSON with zero rendering, specifically so an agent can decide *what* subset is
worth rendering before paying the rendering cost.

---

## 4. Rendering requirements

### 4.1 Two output families, one shared model

Parse once into an internal model (tables, fields, indexes, relations, plus
resolved layout), then have two independent renderers consume it:

- **`svg` renderer** → a single `.svg` file. Also support `--format png` by
  rasterizing that same SVG headlessly at a given `--scale`/`--dpi`, still with
  zero runtime dependencies (statically-linked pure-language rasterizer, see
  §1). SVG must be valid standalone SVG (openable directly in a browser or
  image viewer, embeddable in a Word/PDF doc, etc.) — no reliance on external
  stylesheets or fonts; inline any needed font subset or fall back to declaring
  a generic sans-serif/monospace stack.
- **`html` renderer** → a single `.html` file with all CSS and JS inlined
  (no `<script src="https://…">`, no CDN, no web fonts fetched over network).
  Must open correctly via `file://` with no local server. See §4.3 for
  required interactivity.

Both renderers must produce a legible, modern diagram, not a literal reskin of
the old XSLT output. Suggested visual language (adapt with good design
judgment — this is the "make it attractive" part of the ask):

- Each table = a card: header bar in the table's own editor colour (softened/
  desaturated for a light theme, brightened for dark theme) showing the table
  name and field count; body = a compact field list, each row showing an icon
  or colored dot for the field type, the field name, the type label, and small
  badges for: primary key (e.g. a key glyph), unique, indexed, mandatory,
  hidden/system.
- Relations = curved or orthogonal connectors between the relevant field rows
  (or between table cards if per-field anchoring gets too busy at low zoom),
  labeled with `name_Nto1` / `name_1toN` near each end, using the relation's
  own colour when present, with a crow's-foot or 1/N marker at each end to show
  cardinality direction.
- Don't ship the old bitmap field-type icons (`images/Field_*.png`) — redraw
  small icons as inline vector shapes (or a tiny embedded icon font/SVG
  sprite) generated by the tool itself, so the output truly has zero external
  asset dependencies.
- Provide both a light and a dark theme (`--theme light|dark|auto`); default
  to `auto` in HTML (respects `prefers-color-scheme`) and `light` for static
  image export.
- Typography: legible sans-serif for names/labels, monospace for type codes;
  sensible default canvas size that fits the selected subset without manual
  fiddling; auto-fit/auto-zoom-to-content on load.

### 4.2 Layout: prefer the design, fall back to auto-layout

- Default (`--layout as-designed`): use each table's
  `table_extra/editor_table_info/coordinates` verbatim (scaled to a consistent
  unit), and route relations using the `via_point_x/y` / `prefers_left` hints
  when present, falling back to a clean bezier/orthogonal router when absent.
  This preserves the layout the human database designer already made
  legible — don't fight it.
- `--layout auto`: ignore stored coordinates and compute a fresh layout.
  Implement a deterministic layered/hierarchical layout (Sugiyama-style: rank
  tables by relation topology, minimize edge crossings, then assign
  coordinates) as the primary algorithm; a force-directed layout with a fixed
  seed is an acceptable fallback if layered layout proves too complex, but must
  still be deterministic run-to-run (no reliance on hashmap iteration order or
  wall-clock-seeded randomness).
- If some tables in the selection have stored coordinates and others don't
  (possible with certain 4D versions/exports), `as-designed` mode should use
  real coordinates where available and auto-place only the ones missing them,
  slotting them into free space without overlapping existing cards. (A
  stricter, simpler alternative some reviewers of this spec preferred: treat
  it as all-or-nothing — fall back to full auto-layout the moment *any*
  included table lacks coordinates, rather than mixing sources. Either
  behavior is acceptable; pick one, document it in `--help`, and be
  consistent.)
- Always guarantee **no overlapping table cards** in the final output,
  regardless of mode (nudge/pack as a post-process if needed).
- **Card size is always computed from actual rendered content, never trusted
  from the XML.** The stored `width`/`height` in
  `editor_table_info/coordinates` reflects measurements taken by *4D's own
  editor*, using its own font and metrics — not this tool's. Blindly reusing
  it risks clipped or overlapping text once this tool's fonts/renderer are in
  play. Use the stored `coordinates` only for **position** (`left`/`top`);
  derive card width/height yourself from the longest field name, its type
  badge, the table name, and the visible field count, then run the
  overlap-resolution pass against those computed sizes. A reasonable sizing
  approach: measure/estimate text width for the table name and each field row
  at the chosen font and size, take the max across all rows plus padding,
  clamp to a sensible min/max (e.g. 200–360px wide), and set height from
  header height + (visible field count × row height) + padding.
- For auto-layout, a straightforward, dependency-free approach that scales
  reasonably is a deterministic (fixed-seed) force-directed layout — e.g.
  Fruchterman-Reingold — over the table/relation graph, followed by the same
  overlap-resolution pass. A layered/hierarchical (Sugiyama-style) layout is
  an equally valid and often more readable alternative; pick whichever your
  team can implement well rather than both. If the resulting bounding box
  ends up notably taller than wide, consider swapping X/Y — diagrams read
  better wide than tall.

### 4.3 Required interactivity in the HTML output

- Pan (click-drag background) and zoom (scroll/pinch + on-screen +/− buttons +
  "fit to screen").
- A sidebar: searchable/filterable list of tables (typing narrows the list and
  highlights matches on the canvas); clicking a table centers/zooms to it.
- Click a table to highlight it and all its direct relations; click a relation
  to show a small popup with both field names and the relation's two names.
- Toggle: show/hide hidden(system) fields.
- Toggle: light/dark theme.
- Toggle: "as designed" vs "auto layout" (recompute client-side is *not*
  required — acceptable to require re-running the CLI with `--layout auto` and
  regenerating the file; but if feasible, precompute both layouts server-side
  at generation time and let the toggle swap between two embedded layouts with
  no re-run needed — prefer this if it doesn't blow up file size unreasonably).
- "Export view as SVG/PNG" button that serializes the current on-screen SVG
  client-side (no server, no network) and triggers a browser download.
- Must degrade gracefully with JavaScript disabled: still show a static,
  legible (non-interactive) rendering of the diagram, not a blank page.
- Vanilla JS/CSS/SVG only — do not take a dependency on D3, Cytoscape, React,
  Vue, or any comparable framework/graph library unless there is an
  exceptionally strong justification, and even then it must be fully vendored
  inline into the HTML at build time (no CDN reference, ever) and add no more
  than roughly 50KB. Default to not needing one at all: pan/zoom/drag/search/
  highlight are all straightforward to implement directly against the SVG DOM.
- For catalogs with many tables, consider **semantic zoom** as a quality
  improvement (not a hard requirement): show just table names at low zoom,
  table name + field/index counts at medium zoom, and full field lists only
  at high zoom. This keeps large diagrams legible and the DOM lighter, though
  it's reasonable to ship without it in a first version and add it later.

---

## 5. CLI contract

```
4d-catalog-diagram [command] <input.xml> [options]

The simplest possible invocation must work and be genuinely simple:
  4d-catalog-diagram InvoicesDemo.xml
  → parses it, renders the full catalog as interactive HTML, and writes
    InvoicesDemo.html next to the input (same stem, `.html` extension) —
    no output path required for the common case.

`<input.xml>` is a required positional argument (or `-` / piped stdin).
`render` is the implicit default command when the first argument is a file
path rather than a known subcommand name, so `4d-catalog-diagram foo.xml` and
`4d-catalog-diagram render foo.xml` are equivalent.

Commands:
  render     Render a catalog (subset) to SVG, PNG, or HTML. (default)
  inspect    Parse a catalog and print its structure as JSON (no rendering).
  validate   Parse a catalog and report warnings/errors; exit 0 if parseable.

Global options:
  --error-format <text|json>  Default: text (human). json emits machine-readable
                               errors/warnings on stderr, one JSON object per line.
  -q, --quiet                 Suppress non-error stdout/stderr chatter.
  -V, --version
  -h, --help

`render` options:
  -o, --output <path>          Output file path. Default: <input-stem>.<ext>
                               next to the input file, extension from --format.
                               `-o -` writes svg/html to stdout (not valid for png).
  --format, -f <svg|png|html>  Default: html.
  --tables <csv>              Explicit table name allow-list.
  --tables-match <regex>      Regex allow-list over table names.
  --focus <table>             Center a neighborhood selection on this table.
  --field <table.field>       Focus on one field: include its table plus every
                               table connected to it via a relation touching
                               that specific field (a narrower cut than
                               --focus at the whole-table level).
  --depth <n>                 Hop count for --focus/--field. Default 1.
  --external-refs <ghost|hide|include>   Default: ghost.
  --hide-system-fields
  --layout <as-designed|auto> Default: as-designed.
  --theme <light|dark|auto>   Default: auto for html, light for svg/png.
  --title <string>            Overrides the diagram's displayed title
                               (default: the <base name="…"> from the file).
  --scale <float>             PNG/raster scale factor. Default 2.0 (i.e. 2x).
  --max-tables <n>            Safety cap. Default 300.
  --embed-timestamp           Opt-in to embedding a generation timestamp
                               (breaks determinism deliberately, off by default).

`inspect` options:
  <input.xml>                  Positional, as above.
  --format <json>             (reserved for future formats; json only for now)

`validate` options:
  <input.xml>                  Positional, as above.
```

Exit codes: `0` success, `1` usage error, `2` input file not found/unreadable,
`3` XML not well-formed, `4` selection produced an empty or over-cap table set,
`5` internal rendering error. Keep this stable — an agent will branch on it.

### 6.1 `inspect` output shape (JSON, stable field names, arrays sorted by name)

```json
{
  "base_name": "InvoicesDemo",
  "table_count": 5,
  "relation_count": 3,
  "tables": [
    {
      "name": "CLIENTS",
      "field_count": 20,
      "has_layout": true,
      "primary_key": "ID",
      "relations_in": 1,
      "relations_out": 0,
      "fields": [
        {"name": "ID", "type_code": 4, "type_label": "Longint",
         "unique": true, "mandatory": true, "hidden": false, "indexed": true}
      ]
    }
  ],
  "relations": [
    {"name_Nto1": "Client", "name_1toN": "Invoice_List",
     "from_table": "INVOICES", "from_field": "Client_ID",
     "to_table": "CLIENTS", "to_field": "ID"}
  ],
  "warnings": []
}
```

`relations_in`/`relations_out` count relations where this table is the
destination ("1" side) vs. the source ("N" side) respectively — cheap to
compute during parsing and useful for an agent judging which tables are
central vs. peripheral without rendering anything.

Also include an optional `analysis` object alongside `tables`/`relations`
with derived, non-speculative facts that are purely graph-topology (never
invent business meaning the catalog doesn't state): `tables_without_primary_key`
(array of names), `isolated_tables` (no relations in or out), and
`most_connected_tables` (top N by `relations_in + relations_out`). These are
cheap byproducts of graph construction and meaningfully help an agent decide
where to focus a diagram on an unfamiliar, large catalog.

This is the primary way an agent should decide *what to render* before
spending time on a full diagram — keep it cheap and fast (should run in
milliseconds to low tens of milliseconds for realistic catalogs).

---

## 6. Engineering deliverables

- Source repo with a clear module split: `parse` (XML → internal model),
  `select` (subset filtering), `layout` (as-designed + auto), `render_svg`,
  `render_html`, `raster` (SVG→PNG), `cli`.
- Unit tests per module; golden-file / snapshot tests that render the provided
  fixture (`InvoicesDemo.xml`) in every mode combination and diff against
  committed expected output, to catch regressions and confirm determinism (run
  the same render twice in CI and diff the two outputs byte-for-byte).
- A fuzz target (e.g. `cargo fuzz` or Go's native fuzzing) on the XML parser,
  fed with truncated/mutated versions of the fixture, asserting: never panics,
  never hangs, never allocates unboundedly.
- GitHub Actions (or equivalent) release workflow producing static binaries
  for: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
  `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`
  (or `-gnu` if that's what static linking allows), each verified with `file`/
  `ldd`/`otool -L` in CI to confirm no unexpected dynamic dependencies.
- `README.md` with install instructions (download the right binary, chmod +x,
  done — no package manager, no build step required for end users), full CLI
  reference (can be generated from `--help`), and 3–4 worked examples using the
  fixture.
- `LICENSE` — pick a permissive license (MIT or Apache-2.0) since this will be
  redistributed as part of a "skill" alongside other tooling.
- Keep the binary size reasonable (rough target: under ~15MB per platform);
  avoid embedding large font files — a small embedded subset (Latin + common
  symbols) is enough.

## 7. Reference fixtures to build/test against

Use the attached sample as ground truth for parsing and as the primary visual
target while iterating on the renderer:

- `InvoicesDemo.xml` — a small real catalog (5 tables: `CLIENTS`, `INVOICES`,
  `INVOICE_LINES`, `PRODUCTS`, `DEFAULT_SETTINGS`; 3 relations; multiple
  indexes; every field type variant described in §2.3 appears at least once;
  every table has editor coordinates, so it's a good check for "as-designed"
  layout fidelity).
- Existing XSLT stylesheets (`structure_to_html.xsl`, `structure-to-svg.xsl`,
  `structure-to-sql.xsl`) — these are the *legacy* renderers being replaced.
  They're useful only as a reference for field-type-code mappings and for
  understanding which attributes 4D's own editor considers meaningful; do not
  port their visual design (the whole point of this project is a more
  attractive, responsive, modern replacement) and do not take a dependency on
  XSLT at runtime.

## 8. Definition of done

- `4d-catalog-diagram render InvoicesDemo.xml -o out.html` produces a single HTML file
  that opens correctly via `file://` in a current browser with no console
  errors, shows all 5 tables laid out per their stored coordinates, supports
  pan/zoom/search/theme toggle, and looks like a deliberately designed modern
  diagram rather than a database-tool default export.
- `4d-catalog-diagram render InvoicesDemo.xml --focus INVOICES --depth 1 -o out.svg`
  produces a valid standalone SVG containing exactly `INVOICES`, `CLIENTS`,
  and `INVOICE_LINES` — confirmed from the fixture's actual relations:
  `INVOICES.Client_ID → CLIENTS.ID` (direct, depth 1) and
  `INVOICE_LINES.Invoice_ID → INVOICES.ID` (direct, depth 1). `PRODUCTS` is
  *not* included at depth 1 from `INVOICES` — it's only reachable via
  `INVOICE_LINES.Product_ID → PRODUCTS.ID`, which is two hops from `INVOICES`
  (`INVOICES` → `INVOICE_LINES` → `PRODUCTS`) — so it should appear only as a
  `ghost` stub off `INVOICE_LINES` (default `--external-refs ghost`), or be
  fully included if the user reruns with `--depth 2`. Getting this exact
  boundary right is the actual point of this test.
- A dedicated test renders a mutated copy of the fixture where a table has
  been renamed to something like `<script>alert(1)</script>"><img src=x
  onerror=alert(1)>` and asserts the output HTML/SVG contains no executable
  script and remains well-formed markup.
- `4d-catalog-diagram inspect InvoicesDemo.xml` returns valid JSON matching §6.1's shape
  in well under a second.
- Running any `render` command twice produces byte-identical output.
- All release binaries run with zero installed dependencies on a clean VM/
  container for their target OS.
