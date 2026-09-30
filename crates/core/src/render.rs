//! SVG rendering. The output is self-contained (styles and markers inline)
//! and annotated with `data-*` attributes so the web viewer can add
//! interaction on top of it.
use std::fmt::Write;

use crate::config::{EdgeStyle, Theme, ViewConfig};
use crate::diff::Status;
use crate::graph::{metrics::*, EdgeKind, Graph, NodeKind, RowKind};
use crate::layout::Layout;
use crate::route::{route, Routed};

pub const MARGIN: f64 = 40.0;

pub const CSS: &str = r#"
.sv{--sv-bg:#f6f8fa;--sv-node:#ffffff;--sv-border:#d0d7de;--sv-header:#f0f3f6;--sv-title:#1f2328;--sv-text:#24292f;--sv-muted:#6e7781;--sv-faint:#afb8c1;--sv-edge:#8c959f;--sv-hl:#0969da;--sv-add:#1a7f37;--sv-add-bg:rgba(46,160,67,.13);--sv-del:#cf222e;--sv-del-bg:rgba(248,81,73,.12);--sv-mod:#9a6700;--sv-mod-bg:rgba(212,167,44,.17);--sv-pk:#9a6700;--sv-fk:#0969da;--sv-uq:#8250df;--sv-shadow:rgba(31,35,40,.08);font-family:ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,"Liberation Mono",monospace;font-size:12px}
.sv.sv-dark{--sv-bg:#0d1117;--sv-node:#161b22;--sv-border:#30363d;--sv-header:#1c2230;--sv-title:#e6edf3;--sv-text:#c9d1d9;--sv-muted:#8b949e;--sv-faint:#484f58;--sv-edge:#6e7681;--sv-hl:#58a6ff;--sv-add:#3fb950;--sv-add-bg:rgba(46,160,67,.2);--sv-del:#f85149;--sv-del-bg:rgba(248,81,73,.18);--sv-mod:#d29922;--sv-mod-bg:rgba(187,128,9,.22);--sv-pk:#d29922;--sv-fk:#58a6ff;--sv-uq:#bc8cff;--sv-shadow:rgba(0,0,0,.4)}
.sv text{fill:var(--sv-text);white-space:pre}
.sv .sv-bg{fill:var(--sv-bg)}
.sv-group rect{fill-opacity:.06;stroke-opacity:.45;stroke-width:1.2}
.sv-group text{font-size:13px;font-weight:700;fill-opacity:.9}
.sv-body{fill:var(--sv-node);stroke:var(--sv-border);stroke-width:1;filter:drop-shadow(0 1px 2px var(--sv-shadow))}
.sv-header{fill:var(--sv-header)}
.sv-hline{stroke:var(--sv-border);stroke-width:1}
.sv-title{font-size:13px;font-weight:700;fill:var(--sv-title)!important}
.sv-schema{fill:var(--sv-muted)!important;font-weight:400}
.sv-tag{font-size:9.5px;font-weight:700;letter-spacing:.04em}
.sv-tag-kind,.sv-tag-parts,.sv-tag-ext{fill:var(--sv-muted)!important}
.sv-tag-added{fill:var(--sv-add)!important}.sv-tag-removed{fill:var(--sv-del)!important}.sv-tag-modified{fill:var(--sv-mod)!important}
.sv-row-bg{fill:transparent}
.sv-row:hover .sv-row-bg{fill:var(--sv-header)}
.sv-key{font-size:9px;font-weight:700}
.sv-pk{fill:var(--sv-pk)!important}.sv-fk{fill:var(--sv-fk)!important}.sv-uq{fill:var(--sv-uq)!important}
.sv-type,.sv-null,.sv-default{fill:var(--sv-muted)!important}
.sv-old{fill:var(--sv-del)!important;text-decoration:line-through}
.sv-arrow{fill:var(--sv-muted)!important}
.sv-sign{font-weight:700;font-size:11px}
.sv-more text,.sv-section text{fill:var(--sv-muted)!important;font-style:italic}
.sv-section text{font-size:10px;font-style:normal;letter-spacing:.04em}
.sv-idx .sv-col{fill:var(--sv-muted)!important}
.sv-row.sv-st-added .sv-row-bg{fill:var(--sv-add-bg)}.sv-row.sv-st-added .sv-sign{fill:var(--sv-add)!important}
.sv-row.sv-st-removed .sv-row-bg{fill:var(--sv-del-bg)}.sv-row.sv-st-removed .sv-sign{fill:var(--sv-del)!important}
.sv-row.sv-st-removed .sv-col,.sv-row.sv-st-removed .sv-type{text-decoration:line-through;opacity:.75}
.sv-row.sv-st-modified .sv-row-bg{fill:var(--sv-mod-bg)}.sv-row.sv-st-modified .sv-sign{fill:var(--sv-mod)!important}
.sv-node.sv-st-added .sv-body{stroke:var(--sv-add);stroke-width:2}
.sv-node.sv-st-removed .sv-body{stroke:var(--sv-del);stroke-width:2;stroke-dasharray:6 4}
.sv-node.sv-st-removed .sv-title{text-decoration:line-through}
.sv-node.sv-st-modified .sv-body{stroke:var(--sv-mod);stroke-width:2}
.sv-node.sv-kind-view .sv-body,.sv-node.sv-kind-materialized_view .sv-body{stroke-dasharray:4 3}
.sv-node.sv-focused .sv-body{stroke:var(--sv-hl);stroke-width:2.5}
.sv-edge-line{fill:none;stroke:var(--sv-edge);stroke-width:1.4}
.sv-edge-hit{fill:none;stroke:transparent;stroke-width:10}
.sv-edge.sv-kind-inferred .sv-edge-line{stroke-dasharray:2 4}
.sv-edge.sv-kind-view_dependency .sv-edge-line{stroke-dasharray:6 4;opacity:.8}
.sv-edge.sv-kind-enum_use .sv-edge-line{stroke-dasharray:2 3;opacity:.85}
.sv-node.sv-kind-enum .sv-header{fill:var(--sv-uq);fill-opacity:.13}
.sv-node.sv-kind-enum .sv-body{stroke-dasharray:3 3}
.sv-node.sv-kind-enum.sv-st-modified .sv-body,.sv-node.sv-kind-enum.sv-st-added .sv-body,.sv-node.sv-kind-enum.sv-st-removed .sv-body{stroke-dasharray:none}
.sv-edge.sv-st-added .sv-edge-line{stroke:var(--sv-add);stroke-width:2}
.sv-edge.sv-st-removed .sv-edge-line{stroke:var(--sv-del);stroke-width:2;stroke-dasharray:6 4}
.sv-edge.sv-st-modified .sv-edge-line{stroke:var(--sv-mod);stroke-width:2}
.sv-edge-label{font-size:10px;fill:var(--sv-muted)!important;paint-order:stroke;stroke:var(--sv-bg);stroke-width:3px}
.sv-marker{fill:var(--sv-node);stroke:context-stroke;stroke-width:1.4}
.sv.sv-hovering .sv-node:not(.sv-hl){opacity:.35}
.sv.sv-hovering .sv-edge:not(.sv-hl){opacity:.12}
.sv.sv-hovering .sv-group{opacity:.5}
.sv-edge.sv-hl .sv-edge-line{stroke:var(--sv-hl);stroke-width:2.4}
.sv-node.sv-selected .sv-body{stroke:var(--sv-hl);stroke-width:2.5}
.sv-node.sv-search-hit .sv-body{stroke:var(--sv-hl);stroke-width:3}
.sv-node{cursor:pointer}
"#;

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn status_class(s: Status) -> &'static str {
    match s {
        Status::Unchanged => "",
        Status::Added => " sv-st-added",
        Status::Removed => " sv-st-removed",
        Status::Modified => " sv-st-modified",
    }
}

fn kind_class(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Table => "table",
        NodeKind::View => "view",
        NodeKind::MaterializedView => "materialized_view",
        NodeKind::Enum => "enum",
    }
}

fn edge_kind_class(k: EdgeKind) -> &'static str {
    match k {
        EdgeKind::ForeignKey => "fk",
        EdgeKind::Inferred => "inferred",
        EdgeKind::ViewDependency => "view_dependency",
        EdgeKind::EnumUse => "enum_use",
    }
}

const MARKERS: &str = r#"<defs>
<marker id="sv-m-one" viewBox="-20 -9 22 18" refX="0" refY="0" markerWidth="22" markerHeight="18" markerUnits="userSpaceOnUse" orient="auto-start-reverse"><path class="sv-marker" d="M-6,-6 L-6,6 M-10,-6 L-10,6"/></marker>
<marker id="sv-m-zero-one" viewBox="-20 -9 22 18" refX="0" refY="0" markerWidth="22" markerHeight="18" markerUnits="userSpaceOnUse" orient="auto-start-reverse"><path class="sv-marker" d="M-6,-6 L-6,6"/><circle class="sv-marker" cx="-13" cy="0" r="3.6"/></marker>
<marker id="sv-m-many" viewBox="-20 -9 22 18" refX="0" refY="0" markerWidth="22" markerHeight="18" markerUnits="userSpaceOnUse" orient="auto-start-reverse"><path class="sv-marker" style="fill:none" d="M0,-6 L-11,0 L0,6 M0,0 L-11,0"/></marker>
<marker id="sv-m-zero-many" viewBox="-20 -9 22 18" refX="0" refY="0" markerWidth="22" markerHeight="18" markerUnits="userSpaceOnUse" orient="auto-start-reverse"><path class="sv-marker" style="fill:none" d="M0,-6 L-11,0 L0,6 M0,0 L-11,0"/><circle class="sv-marker" cx="-15" cy="0" r="3.6"/></marker>
<marker id="sv-m-arrow" viewBox="-12 -7 14 14" refX="0" refY="0" markerWidth="14" markerHeight="14" markerUnits="userSpaceOnUse" orient="auto-start-reverse"><path class="sv-marker" style="fill:context-stroke" d="M0,0 L-9,-4.5 L-9,4.5 Z"/></marker>
<marker id="sv-m-none" viewBox="0 0 1 1" markerWidth="1" markerHeight="1"></marker>
<marker id="sv-m-dot" viewBox="-6 -6 12 12" refX="0" refY="0" markerWidth="12" markerHeight="12" markerUnits="userSpaceOnUse" orient="auto"><circle class="sv-marker" style="fill:context-stroke" cx="-2.5" cy="0" r="2.5"/></marker>
</defs>"#;

pub struct EdgeRender {
    pub id: String,
    pub routed: Routed,
}

pub fn route_all(g: &Graph, l: &Layout, cfg: &ViewConfig) -> Vec<EdgeRender> {
    if cfg.edges.style == EdgeStyle::Orthogonal {
        let routed = crate::ortho::route_all(g, l, &cfg.edges, cfg.layout.direction);
        return g.edges.iter().zip(routed).map(|(e, r)| EdgeRender { id: e.id.clone(), routed: r }).collect();
    }
    let idx = g.node_index();
    g.edges
        .iter()
        .enumerate()
        .map(|(ei, e)| {
            let (a, b) = (idx[e.from.as_str()], idx[e.to.as_str()]);
            let wps = l.waypoints.get(ei).map(|v| v.as_slice()).unwrap_or(&[]);
            EdgeRender { id: e.id.clone(), routed: route(e, &g.nodes[a], &l.nodes[a], &g.nodes[b], &l.nodes[b], wps, &cfg.edges, cfg.layout.direction) }
        })
        .collect()
}

pub fn render_svg(g: &Graph, l: &Layout, cfg: &ViewConfig) -> String {
    let routes = route_all(g, l, cfg);
    let mut s = String::with_capacity(4096 + g.nodes.len() * 2048);
    let (w, h) = (l.width + MARGIN * 2.0, l.height + MARGIN * 2.0);
    let theme = if cfg.theme == Theme::Dark { " sv-dark" } else { "" };
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" class="sv{theme}" viewBox="{:.0} {:.0} {:.0} {:.0}" width="{:.0}" height="{:.0}" data-width="{:.0}" data-height="{:.0}">"#,
        -MARGIN, -MARGIN, w, h, w, h, l.width, l.height
    );
    if let Some(t) = &cfg.title {
        let _ = write!(s, "<title>{}</title>", esc(t));
    }
    let _ = write!(s, "<style>{CSS}</style>{MARKERS}");
    let _ = write!(s, r#"<g class="sv-viewport"><rect class="sv-bg" x="{:.0}" y="{:.0}" width="{:.0}" height="{:.0}"/>"#, -MARGIN, -MARGIN, w, h);

    // groups
    s.push_str(r#"<g class="sv-groups">"#);
    for gb in &l.groups {
        let c = esc(&gb.color);
        let _ = write!(
            s,
            r#"<g class="sv-group" data-group="{}"><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="14" style="fill:{c};stroke:{c}"/><text x="{:.1}" y="{:.1}" style="fill:{c}">{}</text></g>"#,
            esc(&gb.name),
            gb.rect.x,
            gb.rect.y,
            gb.rect.w,
            gb.rect.h,
            gb.rect.x + 16.0,
            gb.rect.y + 24.0,
            esc(&gb.name)
        );
    }
    s.push_str("</g>");

    // edges
    s.push_str(r#"<g class="sv-edges">"#);
    if cfg.edges.style != EdgeStyle::Hidden {
        for (e, r) in g.edges.iter().zip(&routes) {
            let (ms, me) = if e.kind == EdgeKind::EnumUse {
                ("sv-m-dot", "sv-m-none")
            } else if e.kind == EdgeKind::ViewDependency {
                ("sv-m-dot", "sv-m-arrow")
            } else if cfg.edges.cardinality {
                let child = if e.one_to_one { "sv-m-zero-one" } else { "sv-m-zero-many" };
                let parent = if e.optional { "sv-m-zero-one" } else { "sv-m-one" };
                (child, parent)
            } else {
                ("sv-m-dot", "sv-m-arrow")
            };
            let _ = write!(
                s,
                r#"<g class="sv-edge sv-kind-{}{}" data-id="{}" data-from="{}" data-to="{}"><title>{}</title><path class="sv-edge-hit" d="{}"/><path class="sv-edge-line" d="{}" marker-start="url(#{ms})" marker-end="url(#{me})"/>"#,
                edge_kind_class(e.kind),
                status_class(e.status),
                esc(&e.id),
                esc(&e.from),
                esc(&e.to),
                esc(&e.tooltip),
                r.routed.d,
                r.routed.d
            );
            if cfg.edges.labels {
                if let Some(name) = &e.name {
                    let _ = write!(
                        s,
                        r#"<text class="sv-edge-label" x="{:.1}" y="{:.1}" text-anchor="middle">{}</text>"#,
                        r.routed.label.0,
                        r.routed.label.1 - 4.0,
                        esc(name)
                    );
                }
            }
            s.push_str("</g>");
        }
    }
    s.push_str("</g>");

    // nodes
    s.push_str(r#"<g class="sv-nodes">"#);
    for (n, r) in g.nodes.iter().zip(&l.nodes) {
        render_node(&mut s, n, r.x, r.y);
    }
    s.push_str("</g></g></svg>");
    s
}

fn render_node(s: &mut String, n: &crate::graph::Node, x: f64, y: f64) {
    let w = n.width;
    let h = n.height;
    let _ = write!(
        s,
        r#"<g class="sv-node sv-kind-{}{}{}" data-id="{}" transform="translate({:.1},{:.1})">"#,
        kind_class(n.kind),
        status_class(n.status),
        if n.focused { " sv-focused" } else { "" },
        esc(&n.id),
        x,
        y
    );
    let mut title = n.id.clone();
    if let Some(c) = &n.comment {
        title.push_str(&format!("\n{c}"));
    }
    if let Some(note) = &n.note {
        title.push_str(&format!("\n📝 {note}"));
    }
    if let Some(c) = &n.change_summary {
        title.push_str(&format!("\nChanged: {c}"));
    }
    if n.hidden_columns > 0 {
        title.push_str(&format!("\n{} of {} columns hidden", n.hidden_columns, n.total_columns));
    }
    let _ = write!(s, "<title>{}</title>", esc(&title));
    let _ = write!(s, r#"<rect class="sv-body" width="{w:.0}" height="{h:.0}" rx="7"/>"#);
    let hh = HEADER_H;
    let header_h = if n.rows.is_empty() { h } else { hh };
    let _ = write!(
        s,
        r#"<path class="sv-header" d="M0.5,{hb:.1} L0.5,7 Q0.5,0.5 7,0.5 L{r:.1},0.5 Q{r2:.1},0.5 {r2:.1},7 L{r2:.1},{hb:.1} {close}"/>"#,
        hb = if n.rows.is_empty() { header_h - 7.0 } else { header_h },
        r = w - 7.0,
        r2 = w - 0.5,
        close = if n.rows.is_empty() {
            format!("Q{:.1},{:.1} {:.1},{:.1} L7,{:.1} Q0.5,{:.1} 0.5,{:.1} Z", w - 0.5, h - 0.5, w - 7.0, h - 0.5, h - 0.5, h - 0.5, h - 7.0)
        } else {
            "Z".to_string()
        }
    );
    if let Some(c) = &n.color {
        let _ = write!(s, r#"<path d="M0,7 Q0,0 7,0 L{:.1},0 Q{w:.1},0 {w:.1},7 L{w:.1},4 L0,4 Z" style="fill:{}"/>"#, w - 7.0, esc(c));
    }
    if !n.rows.is_empty() {
        let _ = write!(s, r#"<line class="sv-hline" x1="0" y1="{hh}" x2="{w:.0}" y2="{hh}"/>"#);
    }
    // title with muted schema prefix
    let ty = 21.0;
    match n.label.find('.') {
        Some(i) if n.kind != NodeKind::Table || n.schema != "public" => {
            let _ = write!(
                s,
                r#"<text class="sv-title" x="{PAD_X}" y="{ty}"><tspan class="sv-schema">{}.</tspan>{}</text>"#,
                esc(&n.label[..i]),
                esc(&n.label[i + 1..])
            );
        }
        _ => {
            let _ = write!(s, r#"<text class="sv-title" x="{PAD_X}" y="{ty}">{}</text>"#, esc(&n.label));
        }
    }
    // header tags, right to left
    let mut tx = w - PAD_X;
    let mut tag = |s: &mut String, text: &str, class: &str, tip: &str| {
        let _ = write!(s, r#"<text class="sv-tag {class}" x="{tx:.1}" y="{ty}" text-anchor="end"><title>{}</title>{}</text>"#, esc(tip), esc(text));
        tx -= text.chars().count() as f64 * 6.6 + 8.0;
    };
    match n.status {
        Status::Added => tag(s, "NEW", "sv-tag-added", "table added"),
        Status::Removed => tag(s, "DROPPED", "sv-tag-removed", "table removed"),
        Status::Modified => tag(s, "CHANGED", "sv-tag-modified", &n.change_summary.as_deref().map(|c| format!("changed: {c}")).unwrap_or_else(|| "table modified".into())),
        Status::Unchanged => {}
    }
    match n.kind {
        NodeKind::View => tag(s, "VIEW", "sv-tag-kind", "view"),
        NodeKind::MaterializedView => tag(s, "MVIEW", "sv-tag-kind", "materialized view"),
        NodeKind::Enum => tag(s, "ENUM", "sv-tag-kind", "enum type"),
        NodeKind::Table => {}
    }
    if n.partitions > 0 {
        tag(s, &format!("+{}P", n.partitions), "sv-tag-parts", &format!("{} partitions (hidden)", n.partitions));
    }
    if n.external_refs > 0 {
        tag(s, &format!("↗{}", n.external_refs), "sv-tag-ext", &format!("{} relations to hidden tables", n.external_refs));
    }

    let mut ry = hh;
    for r in &n.rows {
        let rh = if r.kind == RowKind::Section { SECTION_H } else { ROW_H };
        let base = ry + 14.0;
        match r.kind {
            RowKind::Section => {
                let _ = write!(
                    s,
                    r#"<g class="sv-section" transform="translate(0,{ry:.0})"><line class="sv-hline" x1="0" y1="2" x2="{w:.0}" y2="2"/><text x="{PAD_X}" y="13">{}</text></g>"#,
                    esc(&r.name.to_uppercase())
                );
            }
            RowKind::More => {
                let _ = write!(
                    s,
                    r#"<g class="sv-row sv-more" data-more="1" transform="translate(0,{ry:.0})"><rect class="sv-row-bg" width="{w:.0}" height="{rh}"/><text x="{}" y="14">{}</text></g>"#,
                    PAD_X + BADGE_W,
                    esc(&r.name)
                );
            }
            RowKind::Column | RowKind::Index | RowKind::Constraint | RowKind::ForeignKey => {
                let cls = match r.kind {
                    RowKind::Column => "",
                    _ => " sv-idx",
                };
                let data = if r.kind == RowKind::Column { format!(r#" data-col="{}""#, esc(&r.name)) } else { String::new() };
                let _ = write!(s, r#"<g class="sv-row{cls}{}"{data} transform="translate(0,{ry:.0})">"#, status_class(r.status));
                let _ = write!(s, r#"<rect class="sv-row-bg" x="1" width="{:.0}" height="{rh}"/>"#, w - 2.0);
                if !r.tooltip.is_empty() {
                    let _ = write!(s, "<title>{}</title>", esc(&r.tooltip));
                }
                let sign = match r.status {
                    Status::Added => "+",
                    Status::Removed => "−",
                    Status::Modified => "~",
                    Status::Unchanged => "",
                };
                if !sign.is_empty() {
                    let _ = write!(s, r#"<text class="sv-sign" x="2.5" y="14">{sign}</text>"#);
                }
                let key = match r.kind {
                    RowKind::Index => Some((if r.unique { "UQ" } else { "IX" }, "sv-uq")),
                    RowKind::Constraint => Some(("CK", "sv-uq")),
                    RowKind::ForeignKey => Some(("FK", "sv-fk")),
                    _ if r.pk => Some(("PK", "sv-pk")),
                    _ if r.fk => Some(("FK", "sv-fk")),
                    _ if r.unique => Some(("UQ", "sv-uq")),
                    _ => None,
                };
                if let Some((k, c)) = key {
                    let _ = write!(s, r#"<text class="sv-key {c}" x="{PAD_X}" y="{:.1}">{k}</text>"#, 13.5);
                }
                let _ = write!(s, r#"<text class="sv-col" x="{}" y="14">{}</text>"#, PAD_X + BADGE_W, esc(&r.name));
                let tx = w - PAD_X - NULL_W;
                if !r.data_type.is_empty() || r.default.is_some() {
                    let _ = write!(s, r#"<text class="sv-type" x="{tx:.1}" y="14" text-anchor="end">"#);
                    if let Some(o) = &r.old_type {
                        let _ = write!(s, r#"<tspan class="sv-old">{}</tspan><tspan class="sv-arrow"> → </tspan>"#, esc(o));
                    }
                    let _ = write!(s, "{}", esc(&r.data_type));
                    if let Some(d) = &r.default {
                        let _ = write!(s, r#"<tspan class="sv-default"> = {}</tspan>"#, esc(d));
                    }
                    s.push_str("</text>");
                }
                if r.kind == RowKind::Column && r.nullable {
                    let _ = write!(s, r#"<text class="sv-null" x="{:.1}" y="14" text-anchor="end"><title>nullable</title>?</text>"#, w - PAD_X + 2.0);
                }
                s.push_str("</g>");
            }
        }
        let _ = base;
        ry += rh;
    }
    s.push_str("</g>");
}
