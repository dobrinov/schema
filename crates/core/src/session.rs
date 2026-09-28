//! Stateful facade used by the WASM bindings and the CLI: parse once,
//! re-render cheaply as the view config changes.
use serde::Serialize;
use serde_json::{json, Value};

use crate::config::ViewConfig;
use crate::diff::{diff, SchemaDiff, Status};
use crate::graph::{build, Graph};
use crate::layout::{compute, Layout, Rect};
use crate::model::{display_id, Schema};
use crate::parser::parse;
use crate::render::{render_svg, route_all};
use crate::route::route;

#[derive(Default)]
pub struct Session {
    current: Schema,
    base: Option<Schema>,
    diff: Option<SchemaDiff>,
    last: Option<(Graph, Layout, ViewConfig)>,
}

#[derive(Serialize)]
pub struct NodeInfo {
    pub id: String,
    pub label: String,
    pub status: Status,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Serialize)]
pub struct EdgeInfo {
    pub id: String,
    pub from: String,
    pub to: String,
    pub status: Status,
}

#[derive(Serialize)]
pub struct ViewResult {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub nodes: Vec<NodeInfo>,
    pub edges: Vec<EdgeInfo>,
    pub stats: crate::graph::Stats,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> &Schema {
        &self.current
    }
    pub fn base(&self) -> Option<&Schema> {
        self.base.as_ref()
    }
    pub fn diff(&self) -> Option<&SchemaDiff> {
        self.diff.as_ref()
    }

    pub fn set_sql(&mut self, sql: &str) -> Value {
        self.current = parse(sql);
        self.recompute_diff();
        self.summary()
    }

    pub fn set_base_sql(&mut self, sql: Option<&str>) -> Value {
        self.base = sql.map(parse);
        self.recompute_diff();
        self.summary()
    }

    pub fn set_schema(&mut self, current: Schema, base: Option<Schema>) {
        self.current = current;
        self.base = base;
        self.recompute_diff();
    }

    fn recompute_diff(&mut self) {
        self.diff = self.base.as_ref().map(|b| diff(b, &self.current));
        self.last = None;
    }

    pub fn summary(&self) -> Value {
        json!({
            "tables": self.current.tables.len(),
            "views": self.current.views.len(),
            "enums": self.current.enums.len(),
            "functions": self.current.functions.len(),
            "schemas": self.current.schemas,
            "warnings": self.current.warnings,
            "has_base": self.base.is_some(),
            "diff": self.diff.as_ref().map(|d| &d.summary),
        })
    }

    pub fn graph_and_layout(&self, cfg: &ViewConfig) -> (Graph, Layout) {
        let g = build(&self.current, self.base.as_ref(), self.diff.as_ref(), cfg);
        let l = compute(&g, cfg);
        (g, l)
    }

    pub fn view(&mut self, cfg: ViewConfig) -> ViewResult {
        let (g, l) = self.graph_and_layout(&cfg);
        let svg = render_svg(&g, &l, &cfg);
        let nodes = g
            .nodes
            .iter()
            .zip(&l.nodes)
            .map(|(n, r)| NodeInfo { id: n.id.clone(), label: n.label.clone(), status: n.status, x: r.x, y: r.y, w: r.w, h: r.h })
            .collect();
        let edges = g.edges.iter().map(|e| EdgeInfo { id: e.id.clone(), from: e.from.clone(), to: e.to.clone(), status: e.status }).collect();
        let res = ViewResult { svg, width: l.width, height: l.height, nodes, edges, stats: g.stats.clone() };
        self.last = Some((g, l, cfg));
        res
    }

    /// Move a node (after a drag) and return re-routed paths for its edges.
    pub fn move_node(&mut self, id: &str, x: f64, y: f64) -> Value {
        let Some((g, l, cfg)) = self.last.as_mut() else { return json!([]) };
        let idx = g.node_index();
        let Some(&i) = idx.get(id) else { return json!([]) };
        l.nodes[i] = Rect { x, y, ..l.nodes[i] };
        let mut out = Vec::new();
        for (ei, e) in g.edges.iter().enumerate() {
            if e.from != id && e.to != id {
                continue;
            }
            l.waypoints[ei].clear();
            let (a, b) = (idx[e.from.as_str()], idx[e.to.as_str()]);
            let r = route(e, &g.nodes[a], &l.nodes[a], &g.nodes[b], &l.nodes[b], &[], &cfg.edges, cfg.layout.direction);
            out.push(json!({"id": e.id, "d": r.d, "label": [r.label.0, r.label.1]}));
        }
        Value::Array(out)
    }

    /// All edge paths of the last view (used after bulk position changes).
    pub fn edge_paths(&self) -> Value {
        let Some((g, l, cfg)) = self.last.as_ref() else { return json!([]) };
        let r = route_all(g, l, cfg);
        Value::Array(r.into_iter().map(|e| json!({"id": e.id, "d": e.routed.d})).collect())
    }

    /// Sidebar listing: every table / view with its diff status and degree.
    pub fn tables(&self) -> Value {
        let mut rows = Vec::new();
        let visible: std::collections::HashSet<&str> =
            self.last.as_ref().map(|(g, _, _)| g.nodes.iter().map(|n| n.id.as_str()).collect()).unwrap_or_default();
        let mut refs_in: std::collections::HashMap<String, usize> = Default::default();
        for t in &self.current.tables {
            for f in &t.foreign_keys {
                *refs_in.entry(f.ref_table.clone()).or_default() += 1;
            }
        }
        let status = |id: &str| self.diff.as_ref().map_or(Status::Unchanged, |d| d.table_status(id));
        for t in &self.current.tables {
            let id = t.id();
            rows.push(json!({
                "id": id, "label": display_id(&id), "schema": t.schema, "kind": "table",
                "columns": t.columns.len(), "fk_out": t.foreign_keys.len(), "fk_in": refs_in.get(&id).copied().unwrap_or(0),
                "status": status(&id), "visible": visible.contains(id.as_str()), "partition_of": t.partition_of,
                "comment": t.comment,
            }));
        }
        if let (Some(b), Some(d)) = (&self.base, &self.diff) {
            for t in &b.tables {
                let id = t.id();
                if d.table_status(&id) == Status::Removed {
                    rows.push(json!({
                        "id": id, "label": display_id(&id), "schema": t.schema, "kind": "table",
                        "columns": t.columns.len(), "fk_out": t.foreign_keys.len(), "fk_in": 0,
                        "status": Status::Removed, "visible": visible.contains(id.as_str()), "partition_of": t.partition_of,
                    }));
                }
            }
        }
        for v in &self.current.views {
            let id = v.id();
            let st = self.diff.as_ref().map_or(Status::Unchanged, |d| d.view_status(&id));
            rows.push(json!({
                "id": id, "label": display_id(&id), "schema": v.schema,
                "kind": if v.materialized { "materialized_view" } else { "view" },
                "columns": 0, "fk_out": v.depends_on.len(), "fk_in": 0, "status": st, "visible": visible.contains(id.as_str()),
            }));
        }
        rows.sort_by(|a, b| a["label"].as_str().cmp(&b["label"].as_str()));
        Value::Array(rows)
    }

    /// Full detail for one table or view, including its diff and reverse references.
    pub fn table(&self, id: &str) -> Value {
        let cur = self.current.table(id);
        let old = self.base.as_ref().and_then(|b| b.table(id));
        let td = self.diff.as_ref().and_then(|d| d.table(id));
        let mut referenced_by = Vec::new();
        for t in &self.current.tables {
            for f in &t.foreign_keys {
                if f.ref_table == id {
                    referenced_by.push(json!({"table": t.id(), "columns": f.columns, "ref_columns": f.ref_columns, "name": f.name, "on_delete": f.on_delete}));
                }
            }
        }
        let views: Vec<String> = self.current.views.iter().filter(|v| v.depends_on.iter().any(|d| d == id)).map(|v| v.id()).collect();
        let triggers: Vec<&crate::model::Trigger> = self.current.triggers.iter().filter(|t| t.table == id).collect();
        let view = self.current.view(id).or_else(|| self.base.as_ref().and_then(|b| b.view(id)));
        json!({
            "id": id,
            "table": cur.or(old),
            "base": old,
            "view": view,
            "status": td.map_or(if cur.is_none() && old.is_some() { Status::Removed } else { Status::Unchanged }, |t| t.status),
            "diff": td,
            "referenced_by": referenced_by,
            "used_by_views": views,
            "triggers": triggers,
        })
    }

    pub fn diff_json(&self) -> Value {
        match &self.diff {
            Some(d) => serde_json::to_value(d).unwrap_or(Value::Null),
            None => Value::Null,
        }
    }

    pub fn diff_markdown(&self) -> String {
        self.diff.as_ref().map(crate::diff::to_markdown).unwrap_or_default()
    }

    pub fn schema_json(&self) -> Value {
        serde_json::to_value(&self.current).unwrap_or(Value::Null)
    }
}
