//! Standalone outputs: SVG, self-contained HTML and the embeddable JS bundle.
use schema_core::{Session, ViewConfig};
use serde_json::json;

use crate::assets::{self, base64, text};
use crate::render_esc;

/// A single classic <script> that defines `window.Schema` (and
/// `window.SchemaViewer`) with the WASM module inlined as base64.
pub fn embed_bundle() -> String {
    let mut s = String::with_capacity(assets::WASM.len() * 4 / 3 + 200_000);
    s.push_str(&format!("/*! schema {} embed bundle — https://github.com/dobrinov/schema — MIT */\n", env!("CARGO_PKG_VERSION")));
    s.push_str("(function(){\n\"use strict\";\n");
    s.push_str(text(assets::GLUE_JS));
    s.push_str("\nvar SCHEMA_WASM_B64 = \"");
    s.push_str(&base64(assets::WASM));
    s.push_str("\";\nvar SCHEMA_EMBED_CSS = ");
    s.push_str(&serde_json::to_string(text(assets::EMBED_CSS)).unwrap());
    s.push_str(";\nvar SCHEMA_WASM_INIT = function(bytes){ return wasm_bindgen({ module_or_path: bytes }).then(function(){ return wasm_bindgen; }); };\n");
    s.push_str(text(assets::VIEWER_JS));
    s.push('\n');
    s.push_str(text(assets::EMBED_JS));
    s.push_str("\n})();\n");
    s
}

/// Static-only bundle: no WASM, pan/zoom/hover over a pre-rendered SVG.
fn static_bundle() -> String {
    let mut s = String::new();
    s.push_str("(function(){\n\"use strict\";\nvar SCHEMA_WASM_B64 = null; var SCHEMA_WASM_INIT = null;\nvar SCHEMA_EMBED_CSS = ");
    s.push_str(&serde_json::to_string(text(assets::EMBED_CSS)).unwrap());
    s.push_str(";\n");
    s.push_str(text(assets::VIEWER_JS));
    s.push('\n');
    s.push_str(text(assets::EMBED_JS));
    s.push_str("\n})();\n");
    s
}

/// JSON that is safe to place inside a <script> element.
fn script_json(v: &serde_json::Value) -> String {
    serde_json::to_string(v).unwrap().replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026")
}

pub struct HtmlInput<'a> {
    pub sql: &'a str,
    pub base_sql: Option<&'a str>,
    pub cfg: &'a ViewConfig,
    pub title: String,
    pub subtitle: String,
    pub static_only: bool,
}

pub fn html(input: &HtmlInput) -> String {
    let mut session = Session::new();
    session.set_sql(input.sql);
    if input.base_sql.is_some() {
        session.set_base_sql(input.base_sql);
    }
    let svg = session.view(input.cfg.clone()).svg;
    let data = json!({
        "sql": if input.static_only { None } else { Some(input.sql) },
        "base_sql": if input.static_only { None } else { input.base_sql },
        "config": input.cfg,
        "title": input.title,
        "subtitle": input.subtitle,
        "toolbar": true,
    });
    let bundle = if input.static_only { static_bundle() } else { embed_bundle() };
    let dark = input.cfg.theme == schema_core::config::Theme::Dark;
    format!(
        r#"<!doctype html>
<html lang="en"{dark_attr}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="schema {version}">
<link rel="icon" type="image/svg+xml" href="data:image/svg+xml;base64,{favicon}">
<title>{title}</title>
<style>html,body{{margin:0;height:100%;}}body{{font:14px/1.4 -apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;}}</style>
</head>
<body>
<div id="schema" class="sch-host" style="height:100vh">{svg}</div>
<script type="application/json" id="schema-data">{data}</script>
<script>{bundle}</script>
<script>Schema.mount(document.getElementById("schema"), JSON.parse(document.getElementById("schema-data").textContent));</script>
</body>
</html>
"#,
        dark_attr = if dark { r#" data-theme="dark""# } else { "" },
        version = env!("CARGO_PKG_VERSION"),
        favicon = base64(assets::FAVICON),
        title = render_esc(&input.title),
        svg = svg,
        data = script_json(&data),
        bundle = bundle.replace("</script", "<\\/script"),
    )
}
