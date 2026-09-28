#!/usr/bin/env bash
# Builds the static GitHub Pages site (landing page, examples, playground).
# Usage: scripts/build-site.sh [OUT_DIR]   (default: ./docs)
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/docs}
BIN=${SCHEMA_BIN:-$ROOT/target/release/schema}
[ -x "$BIN" ] || { echo "missing $BIN — run 'make release' first" >&2; exit 1; }

REPO_URL=${SITE_REPO_URL:-$(git -C "$ROOT" remote get-url origin 2>/dev/null | sed -E 's#^git@github.com:#https://github.com/#; s#\.git$##' || true)}
REPO_URL=${REPO_URL:-https://github.com/}

rm -rf "$OUT"
mkdir -p "$OUT/app/pkg" "$OUT/data" "$OUT/examples"
subst() { sed "s#{{REPO_URL}}#$REPO_URL#g" "$1" > "$2"; }

subst "$ROOT/site/index.html" "$OUT/index.html"
cp "$ROOT/site/site.css" "$OUT/site.css"
subst "$ROOT/site/examples/index.html" "$OUT/examples/index.html"
cp "$ROOT/examples/structure.sql" "$ROOT/examples/structure.next.sql" "$OUT/data/"

# one shared bundle (JS + inlined WASM) for every embedded diagram
"$BIN" embed -o "$OUT/schema.embed.js" 2>/dev/null

# playground: the full app in serverless mode
cp "$ROOT/web/app.js" "$ROOT/web/app.css" "$ROOT/web/viewer.js" "$OUT/app/"
cp "$ROOT/web/pkg/schema_wasm.js" "$ROOT/web/pkg/schema_wasm_bg.wasm" "$OUT/app/pkg/"
STATIC='<script>window.SCHEMA_STATIC = { examples: [
  { id: "diff", name: "Diff: structure.sql → structure.next.sql", url: "../data/structure.next.sql", base_url: "../data/structure.sql" },
  { id: "full", name: "Rails SaaS schema (23 tables)", url: "../data/structure.sql" }
] };</script>'
python3 - "$ROOT/web/index.html" "$OUT/app/index.html" "$STATIC" <<'PY'
import sys
src, dst, static = sys.argv[1:4]
html = open(src).read()
html = html.replace('<script src="pkg/schema_wasm.js"></script>', static + '\n<script src="pkg/schema_wasm.js"></script>')
html = html.replace('<title>schema</title>', '<title>Playground — schema</title>')
open(dst, "w").write(html)
PY

# standalone single-file exports
(cd "$OUT/data" && "$BIN" html structure.next.sql --base-file structure.sql -o ../examples/diff.html --title "PR #482 — schema changes" 2>/dev/null)
(cd "$OUT/data" && "$BIN" html structure.sql --focus 'billing.*' --depth 1 --columns keys -o ../examples/focus.html --title "Billing" 2>/dev/null)

touch "$OUT/.nojekyll"
echo "site written to $OUT ($(du -sh "$OUT" | cut -f1))"
