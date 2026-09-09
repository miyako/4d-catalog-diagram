---
name: 4d-catalog-visualisation
description: Use this skill whenever the user wants to visualize, diagram, chart, or explore a 4D database catalog/structure XML file (tables, fields, relations) — as a static image (SVG/PNG) or as an interactive standalone HTML diagram, either for the whole catalog or a subset (specific tables, a regex match, or the "neighborhood" around one table). Trigger on mentions of a 4D "catalog", "structure.xml", "base" export, or an XML file containing <base>/<table>/<relation> elements, combined with any request to see, render, diagram, or explore its schema. Also use this to answer questions about a catalog's tables/fields/relations by inspecting it as structured JSON before deciding what to render. Do NOT use this for generic ER-diagram requests unrelated to a 4D catalog XML, and do not use XSLT-based approaches for this — this skill exists specifically to replace those.
license: MIT
---

# Visualizing a 4D catalog with `4d-catalog-diagram`

## What this is

`4d-catalog-diagram` is a self-contained, static CLI binary (no Python/Node/network
required) that turns a 4D database catalog XML file (tables, fields, indexes,
relations — sometimes called the "structure" or "base" export) into either:

- a modern, interactive, single-file HTML diagram, or
- a static SVG/PNG image,

for the whole catalog or any subset of it. It replaces older XSLT-based
approaches to the same problem — never fall back to XSLT for this task if
`4d-catalog-diagram` is available.

Binaries live at `/mnt/skills/tools/4d-catalog-diagram/bin/<platform>/4d-catalog-diagram` (adjust to
wherever this skill's assets are actually installed — locate with
`which 4d-catalog-diagram` first, or check the skill's own directory for a `bin/` folder).
If no binary is found, tell the user `4d-catalog-diagram` isn't installed rather than
attempting to reimplement it with XSLT or from scratch.

## Recognizing the input

The input is always one XML file with a `<base name="…">` root containing
`<table>`, `<relation>`, and `<index>` elements — this is a 4D catalog/structure
export. If the user hands you an XML file and you're not sure it's this format,
peek at the root element before proceeding:

```bash
head -c 500 catalog.xml
```

If it starts with `<base …>` and contains `<table …>` children, this is a
4D catalog — use this skill. If it's something else (a generic DB schema, a
DTD/XSD, an unrelated XML doc), this skill doesn't apply.

## Workflow

### Step 1 — Inspect before rendering

Always run `inspect` first, not `render`, when you don't already know the
shape of the catalog (table count, table names, relations). This costs almost
nothing and prevents two common mistakes: rendering an overwhelming full
diagram of a 200-table catalog when the user only cares about three tables,
and guessing at table names that don't exist.

```bash
4d-catalog-diagram inspect catalog.xml > /tmp/catalog.json
```

Read the JSON to get `table_count`, `relation_count`, and the list of table
names. Use this to:
- Answer direct questions ("how many tables does this have", "what fields does
  CLIENTS have", "what links INVOICES to CLIENTS") straight from the JSON
  without rendering anything at all, when the user asked a question rather
  than asking to *see* something.
- Decide the right selection strategy for Step 2 when a visual is warranted.

### Step 2 — Choose a selection strategy

| User asked for… | Use |
|---|---|
| "the whole schema" / "the catalog" / didn't specify a subset, and `table_count` is small (roughly ≤ 25) | full catalog, no filters |
| "the whole schema", but `table_count` is large | render full anyway if explicitly asked, but warn the user it may be dense; consider offering to focus instead |
| "how does X relate to things" / "what connects to X" / "just X and its neighbors" | `--focus X --depth 1` (bump to `--depth 2` only if the user asks to go further out) |
| "what uses this specific field" / "what's connected through Table.Field" | `--field Table.Field --depth 1` — narrower than `--focus`, since it follows only relations touching that one field rather than every relation the table has |
| "just show me A, B, and C" | `--tables A,B,C` |
| "everything starting with INVOICE" / a naming pattern | `--tables-match '<regex>'` |

Default `--external-refs ghost` (shows stub nodes for tables just outside the
selection) unless the user explicitly wants a clean, self-contained diagram
with no dangling references, in which case use `--external-refs hide`.

### Step 3 — Choose an output format

| User asked for… | Use |
|---|---|
| "show me", "visualize", explore/click around, or no format specified | `--format html` (default) — this is almost always the right default: it's interactive, self-contained, and opens in any browser. |
| explicitly "an image", "a PNG", "something I can paste into a doc/slide" | `--format png` |
| explicitly "an SVG", "a vector image", "something I can edit" | `--format svg` |

Default `--layout as-designed` — the source catalog already carries the
database designer's own layout coordinates, which are almost always more
legible than an automatic layout. Only pass `--layout auto` if the user asks
for a fresh/automatic arrangement, or if `inspect` shows many tables lack
layout coordinates (`has_layout: false`).

### Step 4 — Render

```bash
4d-catalog-diagram render catalog.xml \
  --focus INVOICES --depth 1 \
  --format html \
  --theme auto \
  -o /mnt/user-data/outputs/invoices_neighborhood.html
```

If the user just wants "the whole thing, quickly" with no particular output
path in mind, `4d-catalog-diagram`'s own default (`<input-stem>.html` next to the
input) is a fine starting point for the filename — just make sure the final
file ends up under `/mnt/user-data/outputs/` before you present it, since
that's the only place the user can actually see it.

Always write output under `/mnt/user-data/outputs/` and then present it with
the file-sharing tool so the user can open it — an HTML diagram is exactly
the kind of standalone artifact that needs a file card, not inline text.

For PNG/SVG requests, the same file goes to `/mnt/user-data/outputs/` and gets
presented the same way; SVGs may also be worth opening with `view` yourself
first to sanity-check the render before handing it to the user.

### Step 5 — Handle errors like an API, not like a human CLI

Pass `--error-format json` when calling `4d-catalog-diagram` programmatically so failures
are easy to branch on. Exit codes: `2` = input file problem (tell the user
the path is wrong or unreadable), `3` = not valid XML (tell the user the file
doesn't look like a well-formed catalog), `4` = your selection was empty or
too large (loosen or tighten `--tables`/`--tables-match`/`--focus`/`--depth`
and retry once automatically before bothering the user), `5` = internal
render error (don't retry blindly — report it).

## Examples

**"Can you show me this catalog?"** (file has 5 tables)
```bash
4d-catalog-diagram inspect catalog.xml            # confirm small size
4d-catalog-diagram render catalog.xml -o /mnt/user-data/outputs/catalog.html
```
Then present the HTML file. No need to ask which subset — 5 tables is small
enough to show in full by default.

**"What does the schema around INVOICES look like?"**
```bash
4d-catalog-diagram render catalog.xml --focus INVOICES --depth 1 \
  -o /mnt/user-data/outputs/invoices_neighborhood.html
```

**"Give me a PNG of just the CLIENTS and INVOICES tables for a slide"**
```bash
4d-catalog-diagram render catalog.xml --tables CLIENTS,INVOICES --format png \
  --scale 3 -o /mnt/user-data/outputs/clients_invoices.png
```

**"What else references the Product_ID field on INVOICE_LINES?"**
```bash
4d-catalog-diagram render catalog.xml --field INVOICE_LINES.Product_ID --depth 1 \
  -o /mnt/user-data/outputs/product_id_context.html
```
Narrower than `--focus INVOICE_LINES`, which would pull in *every* relation
that table has, not just the ones touching this specific field.

**"How many fields does the PRODUCTS table have?"** — don't render anything;
run `inspect`, find `"name": "PRODUCTS"` in the JSON, read `field_count`
(or `fields.length`), and answer directly.

**"This catalog has 180 tables, just show me everything"** — render the full
diagram as asked, but mention up front that a diagram this size is easier to
navigate by focusing (`--focus <table> --depth N`) and offer to do that
instead if the result turns out too dense to read.

## Things to avoid

- Don't hand-roll XSLT or a from-scratch parser for this task — `4d-catalog-diagram`
  exists precisely so this doesn't need to happen per-conversation.
- Don't render before inspecting when you don't already know the catalog's
  size and table names — you'll either overwhelm the user with a giant
  diagram or reference a table name that doesn't exist.
- Don't forget `-o /mnt/user-data/outputs/…` — output written elsewhere is
  invisible to the user.
- Don't claim the render is interactive if you rendered `--format svg` or
  `--format png` — only the HTML output is interactive; say so if the user
  seems to expect click/hover behavior from a static image.
