#!/usr/bin/env bash
# Prove a freshly built binary actually works, on the platform it was built for.
# Used by both the CI and release workflows so the two cannot drift apart.
set -euo pipefail

bin=$1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

"$bin" --version

# Every output format produces a non-empty file.
"$bin" references/InvoicesDemo.xml -o "$work/out.html" -q
"$bin" references/InvoicesDemo.xml -f svg -o "$work/out.svg" -q
"$bin" references/InvoicesDemo.xml -f png -o "$work/out.png" -q
"$bin" references/InvoicesDemo.xml -f mmd -o "$work/out.mmd" -q
"$bin" references/InvoicesDemo.xml -f dot -o "$work/out.dot" -q
for f in out.html out.svg out.png out.mmd out.dot; do
  test -s "$work/$f" || { echo "::error::$f is empty"; exit 1; }
done

# The HTML is self-contained: no fetchable external reference.
if grep -oE 'https?://[^"'"'"' )]+' "$work/out.html" | grep -v '^http://www.w3.org/'; then
  echo "::error::HTML output references an external resource"
  exit 1
fi

# The text formats carry their grammar's preamble and every selected table.
head -n 1 "$work/out.mmd" | grep -qx 'erDiagram' || {
  echo "::error::mmd output is not a Mermaid erDiagram"
  exit 1
}
head -n 1 "$work/out.dot" | grep -q '^digraph catalog {' || {
  echo "::error::dot output is not a Graphviz digraph"
  exit 1
}
for t in CLIENTS INVOICES INVOICE_LINES PRODUCTS; do
  grep -q "$t" "$work/out.mmd" || { echo "::error::$t missing from mmd"; exit 1; }
  grep -q "$t" "$work/out.dot" || { echo "::error::$t missing from dot"; exit 1; }
done

# A still image must never hide fields behind a disclosure toggle.
if grep -q 'class="row-more"' "$work/out.svg"; then
  echo "::error::static SVG must not collapse rows"
  exit 1
fi

# Malformed XML exits 3 and says where.
printf '<?xml version="1.0"?>\n<base>\n  <table 1name="A"/>\n</base>\n' > "$work/bad.xml"
set +e
"$bin" validate "$work/bad.xml" --error-format json 2> "$work/bad.json"
code=$?
set -e
test $code -eq 3 || { echo "::error::expected exit 3 for malformed XML"; exit 1; }
python3 - "$work/bad.json" <<'BADPY'
import json, sys
d = json.loads(open(sys.argv[1]).readline())
assert d["kind"] == "malformed_xml", d
assert d["line"] == 3, d
assert d["column"] > 0, d
BADPY

# inspect emits valid JSON with the expected counts.
"$bin" inspect references/InvoicesDemo.xml > "$work/catalog.json"
python3 - "$work/catalog.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["table_count"] == 5, d["table_count"]
assert d["relation_count"] == 3, d["relation_count"]
PY

# The documented acceptance case: one hop from INVOICES is exactly CLIENTS,
# INVOICES and INVOICE_LINES, with PRODUCTS left as a ghost.
"$bin" references/InvoicesDemo.xml --focus INVOICES --depth 1 -f svg -o "$work/focus.svg" -q
for t in CLIENTS INVOICES INVOICE_LINES; do
  grep -q "data-table=\"$t\"" "$work/focus.svg" || {
    echo "::error::$t missing from the focused render"
    exit 1
  }
done
grep -q 'class="card ghost" data-card="g0" data-table="PRODUCTS"' "$work/focus.svg" || {
  echo "::error::PRODUCTS should be rendered as a ghost"
  exit 1
}

# Identical command, identical bytes.
"$bin" references/InvoicesDemo.xml -o - -q > "$work/first.html"
"$bin" references/InvoicesDemo.xml -o - -q > "$work/second.html"
cmp "$work/first.html" "$work/second.html"

# Documented exit codes.
set +e
"$bin" render does-not-exist.xml -q 2>/dev/null
test $? -eq 2 || { echo "::error::expected exit 2 for a missing input"; exit 1; }
"$bin" render references/InvoicesDemo.xml --tables-match '^NOPE$' -o - -q 2>/dev/null
test $? -eq 4 || { echo "::error::expected exit 4 for an empty selection"; exit 1; }
"$bin" render references/InvoicesDemo.xml --nonsense-flag 2>/dev/null
test $? -eq 1 || { echo "::error::expected exit 1 for a usage error"; exit 1; }

echo "smoke test passed: $bin"
