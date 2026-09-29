#!/usr/bin/env bash
# Renders site/og.html to site/og.png (1200×630) and site/apple-touch-icon.png
# (180×180) with agent-browser. The PNGs are committed, so CI needs no browser.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

agent-browser set viewport 1200 630 >/dev/null
agent-browser open "file://$ROOT/site/og.html" >/dev/null
agent-browser wait 500 >/dev/null
agent-browser screenshot "$ROOT/site/og.png" >/dev/null

cat > "$TMP/icon.html" <<EOF
<body style="margin:0;width:180px;height:180px;background:#0d1117;display:flex;align-items:center;justify-content:center"><img src="file://$ROOT/web/favicon.svg" style="width:128px;height:128px;display:block"></body>
EOF
agent-browser set viewport 180 180 >/dev/null
agent-browser open "file://$TMP/icon.html" >/dev/null
agent-browser wait 300 >/dev/null
agent-browser screenshot "$ROOT/site/apple-touch-icon.png" >/dev/null
agent-browser close >/dev/null || true
echo "wrote site/og.png and site/apple-touch-icon.png"
