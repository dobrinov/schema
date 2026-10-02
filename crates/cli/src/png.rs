//! `schema png`: rasterise the static SVG with resvg. resvg has no CSS custom
//! properties, so the theme's `var(--sv-…)` values are substituted first.
use std::sync::Arc;

use resvg::{tiny_skia, usvg};

/// Largest side of the image in pixels; bigger diagrams are scaled down to fit.
const MAX_SIDE: f32 = 16384.0;

/// Monospace fonts to draw the diagram with, best first (the SVG asks for
/// `ui-monospace`, which only browsers resolve).
const MONO: &[&str] = &["SF Mono", "Menlo", "JetBrains Mono", "DejaVu Sans Mono", "Liberation Mono", "Cascadia Mono", "Consolas", "Courier New"];

pub fn render(svg: &str, scale: f32) -> Result<Vec<u8>, String> {
    let svg = flatten_css(svg);
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    let families: Vec<String> = db.faces().flat_map(|f| f.families.iter().map(|(n, _)| n.clone())).collect();
    if let Some(mono) = MONO.iter().find(|m| families.iter().any(|f| f == *m)) {
        db.set_monospace_family(*mono);
    }
    let opt = usvg::Options { fontdb: Arc::new(db), ..Default::default() };
    let tree = usvg::Tree::from_str(&svg, &opt).map_err(|e| format!("could not read the SVG: {e}"))?;
    let size = tree.size();
    let scale = scale.min(MAX_SIDE / size.width().max(size.height()));
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("the diagram is too large for a PNG")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| format!("could not encode the PNG: {e}"))
}

/// Replace `var(--name)` in the `<style>` block with the theme's values (the
/// dark ones when the root has `sv-dark`) and drop the variable definitions.
fn flatten_css(svg: &str) -> String {
    let (Some(start), Some(end)) = (svg.find("<style>"), svg.find("</style>")) else { return svg.to_string() };
    let css = &svg[start + "<style>".len()..end];
    let dark = svg[..start].contains("sv-dark");
    let mut vars: Vec<(String, String)> = Vec::new();
    let mut rules = String::new();
    for rule in css.split_inclusive('}') {
        let Some((selector, body)) = rule.split_once('{') else { continue };
        let selector = selector.trim();
        if selector == ".sv" || selector == ".sv.sv-dark" {
            if selector == ".sv" || dark {
                for decl in body.trim_end_matches('}').split(';') {
                    if let Some((k, v)) = decl.split_once(':') {
                        let k = k.trim().to_string();
                        let v = v.trim().to_string();
                        if let Some(name) = k.strip_prefix("--") {
                            vars.retain(|(n, _)| n != name);
                            vars.push((name.to_string(), v));
                        } else {
                            // plain declarations of the root rule (font) stay
                            rules.push_str(&format!(".sv{{{k}:{v}}}"));
                        }
                    }
                }
            }
            continue;
        }
        rules.push_str(rule);
    }
    // longest names first, so --sv-add does not eat into --sv-add-bg
    vars.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
    for (name, value) in &vars {
        rules = rules.replace(&format!("var(--{name})"), value);
    }
    format!("{}<style>{}</style>{}", &svg[..start], rules, &svg[end + "</style>".len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" class="sv" width="10" height="10"><style>
.sv{--sv-add:#1b7f3a;--sv-add-bg:rgba(27,127,58,.12);font-size:12px}
.sv.sv-dark{--sv-add:#5cc47a;--sv-add-bg:rgba(92,196,122,.16)}
.a{fill:var(--sv-add)}.b{fill:var(--sv-add-bg)}
</style><rect class="a" width="5" height="5"/></svg>"#;

    #[test]
    fn substitutes_theme_variables() {
        let out = flatten_css(SVG);
        assert!(out.contains(".a{fill:#1b7f3a}") && out.contains(".b{fill:rgba(27,127,58,.12)}"), "{out}");
        assert!(!out.contains("var(") && !out.contains("--sv-"), "{out}");
        assert!(out.contains(".sv{font-size:12px}"), "{out}");
        let dark = flatten_css(&SVG.replace(r#"class="sv""#, r#"class="sv sv-dark""#));
        assert!(dark.contains(".a{fill:#5cc47a}"), "{dark}");
    }

    #[test]
    fn renders_a_png() {
        let png = render(SVG, 2.0).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
