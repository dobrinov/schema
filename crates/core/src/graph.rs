//! Turns a schema (+ optional diff) and a view config into a drawable graph.
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::config::*;
use crate::diff::{SchemaDiff, Status};
use crate::glob::{any_table_matches, column_matches};
use crate::model::*;

/// Text metrics shared by layout and rendering. Nodes use a monospace font
/// so widths can be estimated without measuring text in a browser.
pub mod metrics {
    pub const FONT_SIZE: f64 = 12.0;
    pub const CHAR_W: f64 = 7.25;
    pub const HEADER_CHAR_W: f64 = 7.9;
    pub const HEADER_H: f64 = 32.0;
    pub const ROW_H: f64 = 20.0;
    pub const SECTION_H: f64 = 18.0;
    pub const PAD_X: f64 = 10.0;
    pub const BADGE_W: f64 = 24.0;
    pub const GAP: f64 = 18.0;
    pub const NULL_W: f64 = 10.0;
    pub const MIN_W: f64 = 150.0;
    pub const MAX_W: f64 = 520.0;
    pub const BOTTOM_PAD: f64 = 6.0;
    pub const MAX_NAME: usize = 40;
    pub const MAX_TYPE: usize = 30;
}
use metrics::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Table,
    View,
    MaterializedView,
    Enum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Column,
    Section,
    ForeignKey,
    Index,
    Constraint,
    More,
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub kind: RowKind,
    pub name: String,
    pub data_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_type: Option<String>,
    pub pk: bool,
    pub fk: bool,
    pub unique: bool,
    pub nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    pub status: Status,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tooltip: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: String,
    pub schema: String,
    pub name: String,
    pub label: String,
    pub kind: NodeKind,
    pub status: Status,
    pub rows: Vec<Row>,
    pub hidden_columns: usize,
    pub total_columns: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub partitions: usize,
    /// Relations to tables that are filtered out of the view.
    pub external_refs: usize,
    pub focused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Short description of what changed in a diff ("2 foreign keys renamed").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_summary: Option<String>,
    pub width: f64,
    pub height: f64,
}

impl Node {
    /// Vertical centre of the row showing `column`, relative to the node top.
    pub fn row_center(&self, column: &str) -> Option<f64> {
        let mut y = HEADER_H;
        for r in &self.rows {
            let h = if r.kind == RowKind::Section { SECTION_H } else { ROW_H };
            if r.kind == RowKind::Column && r.name == column {
                return Some(y + h / 2.0);
            }
            y += h;
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    ForeignKey,
    Inferred,
    ViewDependency,
    /// A column whose type is an enum drawn as a node.
    EnumUse,
}

#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub id: String,
    /// Referencing node (FK owner / view).
    pub from: String,
    /// Referenced node.
    pub to: String,
    pub from_columns: Vec<String>,
    pub to_columns: Vec<String>,
    pub kind: EdgeKind,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub one_to_one: bool,
    pub optional: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tooltip: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Stats {
    pub tables_total: usize,
    pub views_total: usize,
    pub nodes_visible: usize,
    /// Enum nodes among `nodes_visible`.
    pub enums_visible: usize,
    pub edges_visible: usize,
    pub hidden_by_filter: usize,
    /// Why tables are not shown (first matching reason per table).
    pub hidden: HiddenCounts,
    /// Tables matched by each focus pattern.
    pub focus_matches: BTreeMap<String, usize>,
    pub column_mode: String,
    pub has_diff: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HiddenCounts {
    pub partitions: usize,
    pub schema: usize,
    pub include: usize,
    pub exclude: usize,
    pub outside_focus: usize,
    pub unchanged: usize,
    pub isolated: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub stats: Stats,
}

impl Graph {
    pub fn node_index(&self) -> HashMap<&str, usize> {
        self.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect()
    }
}

pub const PALETTE: &[&str] = &[
    "#4f6bed", "#1a9b5b", "#d9822b", "#c2418c", "#7c4dde", "#0e8fa8", "#c9423a", "#6d8f1f", "#b58a00", "#2f7f76",
];

pub fn color_for(key: &str) -> String {
    let mut h: u32 = 2166136261;
    for b in key.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    PALETTE[(h as usize) % PALETTE.len()].to_string()
}

/// A table or view as it should be displayed: the current version, or the
/// base version for things removed in the diff.
struct Entity<'a> {
    id: String,
    schema: String,
    name: String,
    kind: NodeKind,
    status: Status,
    table: Option<&'a Table>,
    old: Option<&'a Table>,
    view: Option<&'a View>,
    /// (current, base) definitions of an enum node
    enum_def: Option<(Option<&'a EnumType>, Option<&'a EnumType>)>,
}

/// Value rows of an enum node; in a diff, added / removed values are marked.
fn enum_rows(cur: Option<&EnumType>, base: Option<&EnumType>, status: Status) -> Vec<Row> {
    let mk = |v: &str, st: Status| Row {
        kind: RowKind::Column,
        name: truncate(v, MAX_NAME),
        data_type: String::new(),
        old_type: None,
        pk: false,
        fk: false,
        unique: false,
        nullable: false,
        default: None,
        status: st,
        tooltip: String::new(),
    };
    let values = cur.or(base).map(|e| e.values.clone()).unwrap_or_default();
    if status != Status::Modified {
        return values.iter().map(|v| mk(v, Status::Unchanged)).collect();
    }
    let old: Vec<String> = base.map(|b| b.values.clone()).unwrap_or_default();
    let mut rows: Vec<Row> = values.iter().map(|v| mk(v, if old.contains(v) { Status::Unchanged } else { Status::Added })).collect();
    for (oi, ov) in old.iter().enumerate() {
        if !values.contains(ov) {
            let pos = old[..oi].iter().rev().find_map(|p| rows.iter().position(|r| &r.name == p)).map_or(0, |p| p + 1);
            rows.insert(pos, mk(ov, Status::Removed));
        }
    }
    rows
}

/// `+'paid', −'void'` for a modified enum.
fn enum_change_detail(cur: &EnumType, base: &EnumType) -> String {
    let mut parts: Vec<String> = cur.values.iter().filter(|v| !base.values.contains(v)).map(|v| format!("+'{v}'")).collect();
    parts.extend(base.values.iter().filter(|v| !cur.values.contains(v)).map(|v| format!("−'{v}'")));
    if parts.is_empty() {
        "values reordered".into()
    } else {
        parts.join(", ")
    }
}

struct Relation {
    edge: Edge,
}

pub fn build(schema: &Schema, base: Option<&Schema>, diff: Option<&SchemaDiff>, cfg: &ViewConfig) -> Graph {
    let has_diff = diff.is_some();
    let mut stats = Stats { has_diff, tables_total: schema.tables.len(), views_total: schema.views.len(), ..Default::default() };

    // ---- entities -------------------------------------------------------
    let mut entities: Vec<Entity> = Vec::new();
    let old_tables: HashMap<String, &Table> = base.map(|b| b.tables.iter().map(|t| (t.id(), t)).collect()).unwrap_or_default();
    for t in &schema.tables {
        let id = t.id();
        let status = diff.map_or(Status::Unchanged, |d| d.table_status(&id));
        entities.push(Entity {
            id: id.clone(),
            schema: t.schema.clone(),
            name: t.name.clone(),
            kind: NodeKind::Table,
            status,
            table: Some(t),
            old: old_tables.get(&id).copied(),
            view: None,
            enum_def: None,
        });
    }
    if let (Some(b), Some(d)) = (base, diff) {
        for t in &b.tables {
            let id = t.id();
            if d.table_status(&id) == Status::Removed {
                entities.push(Entity {
                    id,
                    schema: t.schema.clone(),
                    name: t.name.clone(),
                    kind: NodeKind::Table,
                    status: Status::Removed,
                    table: Some(t),
                    old: Some(t),
                    view: None,
                    enum_def: None,
                });
            }
        }
    }
    if cfg.show_views {
        for v in &schema.views {
            let id = v.id();
            entities.push(Entity {
                status: diff.map_or(Status::Unchanged, |d| d.view_status(&id)),
                id,
                schema: v.schema.clone(),
                name: v.name.clone(),
                kind: if v.materialized { NodeKind::MaterializedView } else { NodeKind::View },
                table: None,
                old: None,
                view: Some(v),
                enum_def: None,
            });
        }
        if let (Some(b), Some(d)) = (base, diff) {
            for v in &b.views {
                let id = v.id();
                if d.view_status(&id) == Status::Removed {
                    entities.push(Entity {
                        id,
                        schema: v.schema.clone(),
                        name: v.name.clone(),
                        kind: if v.materialized { NodeKind::MaterializedView } else { NodeKind::View },
                        status: Status::Removed,
                        table: None,
                        old: None,
                        view: Some(v),
                        enum_def: None,
                    });
                }
            }
        }
    }
    // enum types as nodes
    let enum_status = |id: &str| diff.and_then(|d| d.enums.iter().find(|x| x.name == id)).map_or(Status::Unchanged, |x| x.status);
    let show_enums = match cfg.enums {
        crate::config::EnumMode::None => false,
        crate::config::EnumMode::Changed => has_diff,
        crate::config::EnumMode::All => true,
    };
    if show_enums {
        for en in &schema.enums {
            let id = en.id();
            let status = enum_status(&id);
            if cfg.enums == crate::config::EnumMode::Changed && !status.is_changed() {
                continue;
            }
            let old = base.and_then(|b| b.enums.iter().find(|x| x.id() == id));
            entities.push(Entity { id, schema: en.schema.clone(), name: en.name.clone(), kind: NodeKind::Enum, status, table: None, old: None, view: None, enum_def: Some((Some(en), old)) });
        }
        if let Some(b) = base {
            for en in &b.enums {
                let id = en.id();
                if enum_status(&id) == Status::Removed {
                    entities.push(Entity { id, schema: en.schema.clone(), name: en.name.clone(), kind: NodeKind::Enum, status: Status::Removed, table: None, old: None, view: None, enum_def: Some((None, Some(en))) });
                }
            }
        }
    }

    // ---- partitions -----------------------------------------------------
    let mut partition_parent: HashMap<String, String> = HashMap::new();
    let mut partition_count: HashMap<String, usize> = HashMap::new();
    if !cfg.show_partitions {
        for e in &entities {
            if let Some(p) = e.table.and_then(|t| t.partition_of.clone()) {
                *partition_count.entry(p.clone()).or_default() += 1;
                partition_parent.insert(e.id.clone(), p);
            }
        }
    }
    let canon = |id: &str| -> String {
        let mut cur = id.to_string();
        for _ in 0..8 {
            match partition_parent.get(&cur) {
                Some(p) => cur = p.clone(),
                None => break,
            }
        }
        cur
    };

    // ---- base filter ----------------------------------------------------
    let mut hidden_by_filter = 0;
    let mut base_set: Vec<usize> = Vec::new();
    let mut hidden = HiddenCounts::default();
    for (i, e) in entities.iter().enumerate() {
        if partition_parent.contains_key(&e.id) {
            hidden.partitions += 1;
            continue;
        }
        if !(cfg.schemas.is_empty() || cfg.schemas.contains(&e.schema)) {
            hidden.schema += 1;
        } else if !(cfg.include.is_empty() || any_table_matches(&cfg.include, &e.id)) {
            hidden.include += 1;
        } else if any_table_matches(&cfg.exclude, &e.id) {
            hidden.exclude += 1;
        } else {
            base_set.push(i);
            continue;
        }
        hidden_by_filter += 1;
    }
    let in_base: HashMap<String, usize> = base_set.iter().map(|&i| (entities[i].id.clone(), i)).collect();

    // ---- relations ------------------------------------------------------
    let mut relations: Vec<Relation> = Vec::new();
    let mut seen_edge_ids: HashSet<String> = HashSet::new();
    let table_ids: HashSet<String> = entities.iter().filter(|e| e.kind == NodeKind::Table).map(|e| e.id.clone()).collect();
    for &i in &base_set {
        let e = &entities[i];
        let Some(t) = e.table else { continue };
        let td = diff.and_then(|d| d.table(&e.id));
        let mut fks: Vec<(ForeignKey, Status)> = t
            .foreign_keys
            .iter()
            .map(|f| {
                let st = if e.status == Status::Added || e.status == Status::Removed {
                    e.status
                } else {
                    // renamed constraints appear as "old → new"
                    td.and_then(|td| td.foreign_keys.iter().find(|x| x.name == f.key() || x.name.ends_with(&format!("→ {}", f.key()))))
                        .map_or(Status::Unchanged, |x| x.status)
                };
                (f.clone(), st)
            })
            .collect();
        if e.status == Status::Modified {
            if let (Some(old), Some(td)) = (e.old, td) {
                for f in &old.foreign_keys {
                    if td.foreign_keys.iter().any(|x| x.name == f.key() && x.status == Status::Removed) {
                        fks.push((f.clone(), Status::Removed));
                    }
                }
            }
        }
        for (k, (f, st)) in fks.into_iter().enumerate() {
            let to = canon(&f.ref_table);
            if !in_base.contains_key(&to) {
                continue;
            }
            if to == e.id && !cfg.edges.self_loops {
                continue;
            }
            let mut id = format!("fk:{}:{}", e.id, f.name.clone().unwrap_or_else(|| k.to_string()));
            if !seen_edge_ids.insert(id.clone()) {
                id = format!("{id}:{k}");
                seen_edge_ids.insert(id.clone());
            }
            let one_to_one = f.columns.len() == 1 && t.is_unique(&f.columns[0])
                || (!f.columns.is_empty() && t.primary_key.as_ref().is_some_and(|p| p.columns == f.columns));
            let optional = f.columns.iter().any(|c| t.column(c).is_some_and(|c| c.nullable));
            let tooltip = format!(
                "{}{}.{} → {}.{}{}",
                f.name.as_deref().map(|n| format!("{n}\n")).unwrap_or_default(),
                display_id(&e.id),
                f.columns.join(", "),
                display_id(&f.ref_table),
                f.ref_columns.join(", "),
                f.on_delete.as_deref().map(|d| format!("\nON DELETE {d}")).unwrap_or_default()
            );
            relations.push(Relation {
                edge: Edge {
                    id,
                    from: e.id.clone(),
                    to,
                    from_columns: f.columns.clone(),
                    to_columns: f.ref_columns.clone(),
                    kind: EdgeKind::ForeignKey,
                    status: st,
                    name: f.name.clone(),
                    one_to_one,
                    optional,
                    tooltip,
                },
            });
        }
        if cfg.edges.inferred {
            for c in &t.columns {
                let Some(stem) = c.name.strip_suffix("_id") else { continue };
                if stem.is_empty() || t.is_fk(&c.name) {
                    continue;
                }
                // polymorphic association: <stem>_type sibling
                if t.column(&format!("{stem}_type")).is_some() {
                    continue;
                }
                let target = plural_candidates(stem)
                    .into_iter()
                    .map(|n| qualify(&e.schema, &n))
                    .find(|id| table_ids.contains(id) && in_base.contains_key(id))
                    .or_else(|| {
                        plural_candidates(stem)
                            .into_iter()
                            .map(|n| qualify("public", &n))
                            .find(|id| table_ids.contains(id) && in_base.contains_key(id))
                    });
                if let Some(to) = target {
                    let to = canon(&to);
                    if to == e.id && !cfg.edges.self_loops {
                        continue;
                    }
                    relations.push(Relation {
                        edge: Edge {
                            id: format!("inf:{}:{}", e.id, c.name),
                            from: e.id.clone(),
                            to: to.clone(),
                            from_columns: vec![c.name.clone()],
                            to_columns: vec!["id".into()],
                            kind: EdgeKind::Inferred,
                            status: Status::Unchanged,
                            name: None,
                            one_to_one: false,
                            optional: c.nullable,
                            tooltip: format!("inferred: {}.{} → {}.id", display_id(&e.id), c.name, display_id(&to)),
                        },
                    });
                }
            }
        }
    }
    if cfg.edges.view_dependencies {
        for &i in &base_set {
            let e = &entities[i];
            let Some(v) = e.view else { continue };
            for dep in &v.depends_on {
                let to = canon(dep);
                if in_base.contains_key(&to) && to != e.id {
                    relations.push(Relation {
                        edge: Edge {
                            id: format!("dep:{}:{}", e.id, to),
                            from: e.id.clone(),
                            to,
                            from_columns: vec![],
                            to_columns: vec![],
                            kind: EdgeKind::ViewDependency,
                            status: e.status,
                            name: None,
                            one_to_one: false,
                            optional: false,
                            tooltip: format!("{} reads {}", display_id(&e.id), display_id(dep)),
                        },
                    });
                }
            }
        }
    }
    // columns typed with a drawn enum
    let mut affected: HashMap<String, Vec<(String, String, String)>> = HashMap::new(); // table → (column, enum id, change detail)
    let enum_entities: HashMap<String, usize> = base_set.iter().filter(|&&i| entities[i].kind == NodeKind::Enum).map(|&i| (entities[i].id.clone(), i)).collect();
    if !enum_entities.is_empty() {
        let mut by_name: HashMap<&str, Vec<&str>> = HashMap::new();
        for id in enum_entities.keys() {
            by_name.entry(split_id(id).1).or_default().push(id.as_str());
        }
        let resolve = |ty: &str| -> Option<String> {
            let ty = ty.trim().trim_end_matches("[]").trim();
            if enum_entities.contains_key(ty) {
                return Some(ty.to_string());
            }
            if !ty.contains('.') {
                let q = qualify("public", ty);
                if enum_entities.contains_key(&q) {
                    return Some(q);
                }
                if let Some(v) = by_name.get(ty) {
                    if v.len() == 1 {
                        return Some(v[0].to_string());
                    }
                }
            }
            None
        };
        for &i in &base_set {
            let e = &entities[i];
            let Some(t) = e.table else { continue };
            for c in &t.columns {
                let Some(eid) = resolve(&c.data_type) else { continue };
                let en = &entities[enum_entities[&eid]];
                let detail = match en.enum_def {
                    Some((Some(cur), Some(old))) if en.status == Status::Modified => Some(enum_change_detail(cur, old)),
                    _ => None,
                };
                if let Some(d) = &detail {
                    affected.entry(e.id.clone()).or_default().push((c.name.clone(), eid.clone(), d.clone()));
                }
                relations.push(Relation {
                    edge: Edge {
                        id: format!("enum:{}:{}", e.id, c.name),
                        from: e.id.clone(),
                        to: eid.clone(),
                        from_columns: vec![c.name.clone()],
                        to_columns: vec![],
                        kind: EdgeKind::EnumUse,
                        status: if en.status == Status::Modified { Status::Modified } else { Status::Unchanged },
                        name: None,
                        one_to_one: false,
                        optional: false,
                        tooltip: format!("{}.{} uses enum {}{}", display_id(&e.id), c.name, display_id(&eid), detail.map(|d| format!("\nchanged: {d}")).unwrap_or_default()),
                    },
                });
            }
        }
    }
    relations.dedup_by(|a, b| a.edge.id == b.edge.id);

    // ---- neighbourhood selection -----------------------------------------
    let mut out_adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in &relations {
        out_adj.entry(r.edge.from.as_str()).or_default().push(r.edge.to.as_str());
        in_adj.entry(r.edge.to.as_str()).or_default().push(r.edge.from.as_str());
    }
    let bfs = |seeds: &[String], depth: u32, dir: FocusDirection| -> HashSet<String> {
        let mut seen: HashSet<String> = seeds.iter().cloned().collect();
        let mut q: VecDeque<(String, u32)> = seeds.iter().map(|s| (s.clone(), 0)).collect();
        while let Some((id, d)) = q.pop_front() {
            if d >= depth {
                continue;
            }
            let mut next: Vec<&str> = Vec::new();
            if dir != FocusDirection::Incoming {
                next.extend(out_adj.get(id.as_str()).into_iter().flatten());
            }
            if dir != FocusDirection::Outgoing {
                next.extend(in_adj.get(id.as_str()).into_iter().flatten());
            }
            for n in next {
                if seen.insert(n.to_string()) {
                    q.push_back((n.to_string(), d + 1));
                }
            }
        }
        seen
    };

    let mut selected: HashSet<String> = in_base.keys().cloned().collect();
    let mut focus_seeds: Vec<String> = Vec::new();
    if !cfg.focus.is_empty() {
        // each pattern brings its own neighbourhood (depth per pattern)
        selected = HashSet::new();
        for pat in &cfg.focus {
            let seeds: Vec<String> = base_set
                .iter()
                .map(|&i| entities[i].id.clone())
                .filter(|id| crate::glob::table_matches(pat, id))
                .collect();
            stats.focus_matches.insert(pat.clone(), seeds.len());
            let depth = cfg.focus_depths.get(pat).copied().unwrap_or(cfg.focus_depth);
            selected.extend(bfs(&seeds, depth, cfg.focus_direction));
            for s in seeds {
                if !focus_seeds.contains(&s) {
                    focus_seeds.push(s);
                }
            }
        }
        if focus_seeds.is_empty() {
            stats.notices.push(format!("focus {:?} matched no visible table", cfg.focus));
        }
        hidden.outside_focus = base_set.len().saturating_sub(selected.len());
    }
    if cfg.changes_only {
        if has_diff {
            let changed: Vec<String> = base_set
                .iter()
                .map(|&i| &entities[i])
                .filter(|e| (e.status.is_changed() || affected.contains_key(&e.id)) && selected.contains(&e.id))
                .map(|e| e.id.clone())
                .collect();
            let near = bfs(&changed, cfg.changes_context, FocusDirection::Both);
            let before = selected.len();
            selected.retain(|id| near.contains(id));
            hidden.unchanged = before - selected.len();
        } else {
            stats.notices.push("changes_only has no effect without a base to compare against".into());
        }
    }
    if !cfg.show_isolated {
        let connected: HashSet<&str> = relations
            .iter()
            .filter(|r| r.edge.from != r.edge.to && selected.contains(&r.edge.from) && selected.contains(&r.edge.to))
            .flat_map(|r| [r.edge.from.as_str(), r.edge.to.as_str()])
            .collect();
        let seeds: HashSet<&String> = focus_seeds.iter().collect();
        let before = selected.len();
        selected.retain(|id| connected.contains(id.as_str()) || seeds.contains(id));
        hidden.isolated = before - selected.len();
    }

    // ---- nodes ----------------------------------------------------------
    let visible: Vec<usize> = base_set.iter().copied().filter(|&i| selected.contains(&entities[i].id)).collect();
    let effective_mode = match cfg.columns {
        ColumnMode::Auto => {
            if visible.len() <= 40 {
                ColumnMode::All
            } else {
                ColumnMode::Keys
            }
        }
        m => m,
    };
    stats.column_mode = format!("{effective_mode:?}").to_lowercase();
    let multi_schema = visible.iter().map(|&i| &entities[i].schema).collect::<HashSet<_>>().len() > 1;

    // group assignment
    let mut prefix_count: HashMap<String, usize> = HashMap::new();
    if cfg.layout.group_by == GroupBy::Prefix {
        for &i in &visible {
            if let Some(p) = name_prefix(&entities[i].name) {
                *prefix_count.entry(p).or_default() += 1;
            }
        }
    }
    let group_of = |e: &Entity| -> Option<(String, Option<String>)> {
        match cfg.layout.group_by {
            GroupBy::None => None,
            GroupBy::Schema => Some((e.schema.clone(), None)),
            GroupBy::Prefix => name_prefix(&e.name).filter(|p| prefix_count.get(p).copied().unwrap_or(0) >= 2).map(|p| (p, None)),
            GroupBy::Custom => cfg.groups.iter().find(|g| any_table_matches(&g.tables, &e.id)).map(|g| (g.name.clone(), g.color.clone())),
        }
    };

    // columns used by the relations that will be drawn
    let mut referenced: HashMap<&str, HashSet<&str>> = HashMap::new();
    for r in &relations {
        if selected.contains(&r.edge.from) && selected.contains(&r.edge.to) {
            referenced.entry(r.edge.from.as_str()).or_default().extend(r.edge.from_columns.iter().map(|c| c.as_str()));
            referenced.entry(r.edge.to.as_str()).or_default().extend(r.edge.to_columns.iter().map(|c| c.as_str()));
        }
    }

    let mut nodes: Vec<Node> = Vec::new();
    for &i in &visible {
        let e = &entities[i];
        let ov = cfg.table_override(&e.id);
        let base_mode = match cfg.unchanged_columns {
            Some(m) if has_diff && e.status == Status::Unchanged => m,
            _ => effective_mode,
        };
        let mode = if ov.is_some_and(|o| o.collapsed) { ColumnMode::None } else { ov.and_then(|o| o.columns).unwrap_or(base_mode) };
        let mode = if mode == ColumnMode::Auto { effective_mode } else { mode };
        let refs = referenced.get(e.id.as_str());
        let aff = affected.get(&e.id).map(|v| v.as_slice()).unwrap_or(&[]);
        let (rows, hidden, total) = match (e.table, e.enum_def) {
            (Some(t), _) => build_rows(e, t, diff, cfg, ov, mode, has_diff, refs, aff),
            (None, Some((cur, old))) => {
                let rows = enum_rows(cur, old, e.status);
                let n = rows.len();
                (rows, 0, n)
            }
            _ => (Vec::new(), 0, 0),
        };
        // a table whose column type is a changed enum is affected by the change
        let (status, change_summary) = if !aff.is_empty() && e.status == Status::Unchanged {
            (Status::Modified, Some(aff.iter().map(|(c, en, d)| format!("{c} uses changed enum {} ({d})", display_id(en))).collect::<Vec<_>>().join("; ")))
        } else {
            (e.status, diff.and_then(|d| d.table(&e.id)).and_then(change_summary))
        };
        let group = group_of(e);
        let color = ov
            .and_then(|o| o.color.clone())
            .or_else(|| group.as_ref().and_then(|g| g.1.clone()))
            .or_else(|| group.as_ref().map(|g| color_for(&g.0)))
            .or_else(|| if multi_schema { Some(color_for(&e.schema)) } else { None });
        let comment = e.table.and_then(|t| t.comment.clone()).or_else(|| e.view.and_then(|v| v.comment.clone()));
        let label = display_id(&e.id).to_string();
        let mut node = Node {
            id: e.id.clone(),
            schema: e.schema.clone(),
            name: e.name.clone(),
            label,
            kind: e.kind,
            status,
            rows,
            hidden_columns: hidden,
            total_columns: total,
            comment,
            color,
            group: group.map(|g| g.0),
            partitions: partition_count.get(&e.id).copied().unwrap_or(0),
            external_refs: 0,
            focused: focus_seeds.contains(&e.id),
            note: ov.and_then(|o| o.note.clone()),
            change_summary,
            width: 0.0,
            height: 0.0,
        };
        size_node(&mut node);
        nodes.push(node);
    }

    let visible_ids: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let mut edges: Vec<Edge> = Vec::new();
    let mut external: HashMap<String, usize> = HashMap::new();
    for r in relations {
        let a = visible_ids.contains(r.edge.from.as_str());
        let b = visible_ids.contains(r.edge.to.as_str());
        if a && b {
            edges.push(r.edge);
        } else if a {
            *external.entry(r.edge.from.clone()).or_default() += 1;
        } else if b {
            *external.entry(r.edge.to.clone()).or_default() += 1;
        }
    }
    for n in &mut nodes {
        n.external_refs = external.get(&n.id).copied().unwrap_or(0);
    }
    stats.nodes_visible = nodes.len();
    stats.enums_visible = nodes.iter().filter(|n| n.kind == NodeKind::Enum).count();
    stats.edges_visible = edges.len();
    stats.hidden_by_filter = hidden_by_filter;
    stats.hidden = hidden;
    Graph { nodes, edges, stats }
}

fn name_prefix(name: &str) -> Option<String> {
    let i = name.find('_')?;
    if i == 0 {
        return None;
    }
    Some(name[..i].to_string())
}

pub fn plural_candidates(stem: &str) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(s) = stem.strip_suffix('y') {
        if !s.ends_with(['a', 'e', 'i', 'o', 'u']) {
            v.push(format!("{s}ies"));
        }
    }
    if stem.ends_with('s') || stem.ends_with('x') || stem.ends_with("ch") || stem.ends_with("sh") {
        v.push(format!("{stem}es"));
    }
    if let Some(s) = stem.strip_suffix("person") {
        v.push(format!("{s}people"));
    }
    v.push(format!("{stem}s"));
    v.push(stem.to_string());
    v
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[allow(clippy::too_many_arguments)]
fn build_rows(
    e: &Entity,
    t: &Table,
    diff: Option<&SchemaDiff>,
    cfg: &ViewConfig,
    ov: Option<&TableOverride>,
    mode: ColumnMode,
    has_diff: bool,
    refs: Option<&HashSet<&str>>,
    affected: &[(String, String, String)],
) -> (Vec<Row>, usize, usize) {
    let td = diff.and_then(|d| d.table(&e.id));
    // merged columns: current order, removed columns re-inserted after their old predecessor
    let mut cols: Vec<(&Column, Status, Option<&Column>)> = t
        .columns
        .iter()
        .map(|c| {
            let st = if e.status == Status::Added || e.status == Status::Removed {
                e.status
            } else {
                td.and_then(|td| td.column(&c.name)).map_or(Status::Unchanged, |cd| cd.status)
            };
            let old = e.old.and_then(|o| o.column(&c.name));
            (c, st, old)
        })
        .collect();
    if e.status == Status::Modified {
        if let (Some(old), Some(td)) = (e.old, td) {
            for (oi, oc) in old.columns.iter().enumerate() {
                if td.column(&oc.name).is_some_and(|c| c.status == Status::Removed) {
                    let pos = old.columns[..oi]
                        .iter()
                        .rev()
                        .find_map(|p| cols.iter().position(|(c, _, _)| c.name == p.name))
                        .map_or(0, |p| p + 1);
                    cols.insert(pos, (oc, Status::Removed, Some(oc)));
                }
            }
        }
    }
    let total = cols.len();
    let lookup = |name: &str| -> &Table {
        if t.column(name).is_some() {
            t
        } else {
            e.old.unwrap_or(t)
        }
    };
    let fmt_type = |ty: &str| -> String {
        let s = if cfg.short_types { short_type(ty) } else { ty.to_string() };
        truncate(&s, MAX_TYPE)
    };
    let mut rows = Vec::new();
    for (c, st, old) in &cols {
        let owner = lookup(&c.name);
        let pk = owner.is_pk(&c.name);
        let fk = owner.is_fk(&c.name);
        let unique = owner.is_unique(&c.name);
        let enum_change = affected.iter().find(|(n, _, _)| n == &c.name);
        let changed = (st.is_changed() && e.status == Status::Modified) || enum_change.is_some();
        let forced = ov.is_some_and(|o| o.show_columns.iter().any(|p| column_matches(p, &e.id, &c.name)));
        let mut show = match mode {
            ColumnMode::All | ColumnMode::Auto => true,
            ColumnMode::Keys => pk || fk || unique || changed,
            ColumnMode::Relations => pk || fk || changed,
            ColumnMode::Referenced => changed || refs.is_some_and(|r| r.contains(c.name.as_str())),
            ColumnMode::Changed => {
                if has_diff {
                    changed || (e.status != Status::Modified && (pk || fk))
                } else {
                    pk || fk || unique
                }
            }
            ColumnMode::None => false,
        };
        if show && !forced {
            let hidden = cfg.hide_columns.iter().any(|p| column_matches(p, &e.id, &c.name))
                || ov.is_some_and(|o| o.hide_columns.iter().any(|p| column_matches(p, &e.id, &c.name)));
            if hidden {
                show = false;
            }
        }
        if forced && mode != ColumnMode::None {
            show = true;
        }
        if !show {
            continue;
        }
        let mut tip = Vec::new();
        tip.push(format!("{} {}{}", c.name, c.data_type, if c.nullable { "" } else { " NOT NULL" }));
        if let Some((_, en, d)) = enum_change {
            tip.push(format!("enum {} changed: {d}", display_id(en)));
        }
        if let Some(d) = &c.default {
            tip.push(format!("default: {d}"));
        }
        if let Some(i) = &c.identity {
            tip.push(format!("identity: {i}"));
        }
        if let Some(g) = &c.generated {
            tip.push(format!("generated: {g}"));
        }
        if let Some(cm) = &c.comment {
            tip.push(cm.clone());
        }
        let mut old_type = None;
        if *st == Status::Modified {
            if let Some(cd) = td.and_then(|td| td.column(&c.name)) {
                for ch in &cd.changes {
                    tip.push(format!("{}: {} → {}", ch.field, ch.old.as_deref().unwrap_or("∅"), ch.new.as_deref().unwrap_or("∅")));
                    if ch.field == "type" {
                        old_type = ch.old.as_deref().map(&fmt_type);
                    }
                }
            }
            let _ = old;
        }
        let default = if cfg.show_defaults { c.default.as_deref().map(|d| truncate(&short_default(d), 24)) } else { None };
        rows.push(Row {
            kind: RowKind::Column,
            name: truncate(&c.name, MAX_NAME),
            data_type: if cfg.show_types { fmt_type(&c.data_type) } else { String::new() },
            old_type: if cfg.show_types { old_type } else { None },
            pk,
            fk,
            unique,
            nullable: c.nullable && cfg.show_nullable,
            default,
            status: if enum_change.is_some() { Status::Modified } else if e.status == Status::Modified { *st } else { Status::Unchanged },
            tooltip: tip.join("\n"),
        });
    }
    let mut hidden = total - rows.len();
    if cfg.max_columns > 0 && rows.len() > cfg.max_columns {
        hidden += rows.len() - cfg.max_columns;
        rows.truncate(cfg.max_columns);
    }
    if hidden > 0 && mode != ColumnMode::None {
        rows.push(Row {
            kind: RowKind::More,
            name: format!("… {hidden} more column{}", if hidden == 1 { "" } else { "s" }),
            data_type: String::new(),
            old_type: None,
            pk: false,
            fk: false,
            unique: false,
            nullable: true,
            default: None,
            status: Status::Unchanged,
            tooltip: String::new(),
        });
    }

    // index & constraint rows
    let mut extra: Vec<Row> = Vec::new();
    let show_all_idx = cfg.indexes == IndexMode::All;
    let show_changed_idx = cfg.indexes != IndexMode::None && e.status == Status::Modified;
    // index / constraint names are long: keep both columns short enough to
    // share a row (the tooltip has the full text)
    let mk = |kind: RowKind, name: &str, def: String, status: Status, unique: bool, tip: String| Row {
        kind,
        name: truncate(name, 34),
        data_type: truncate(&def, 24),
        old_type: None,
        pk: false,
        fk: false,
        unique,
        nullable: true,
        default: None,
        status,
        tooltip: tip,
    };
    if show_all_idx && mode != ColumnMode::None {
        for i in &t.indexes {
            let st = td.and_then(|td| td.indexes.iter().find(|x| x.name == i.name)).map_or(Status::Unchanged, |x| x.status);
            let st = if e.status == Status::Modified { st } else { Status::Unchanged };
            extra.push(mk(RowKind::Index, &i.name, format!("({})", i.columns.join(", ")), st, i.unique, i.definition.clone()));
        }
    }
    if show_changed_idx {
        if let Some(td) = td {
            for x in &td.indexes {
                if show_all_idx && x.status != Status::Removed {
                    continue;
                }
                let def = x.new.as_deref().or(x.old.as_deref()).unwrap_or_default();
                let cols = def.find('(').map(|p| def[p..].to_string()).unwrap_or_default();
                let unique = def.contains("UNIQUE");
                let tip = match (&x.old, &x.new) {
                    (Some(o), Some(n)) => format!("{o}\n→\n{n}"),
                    _ => def.to_string(),
                };
                let name = x.name.clone();
                extra.push(mk(RowKind::Index, &name, cols, x.status, unique, tip));
            }
            for x in &td.constraints {
                let def = x.new.as_deref().or(x.old.as_deref()).unwrap_or_default();
                extra.push(mk(RowKind::Constraint, &x.name, def.to_string(), x.status, false, def.to_string()));
            }
            // foreign keys are drawn as edges, but the edge may lead to a hidden
            // table (or the change may be a rename): show them as rows too
            for x in &td.foreign_keys {
                let def = x.new.as_deref().or(x.old.as_deref()).unwrap_or_default();
                let renamed = x.name.contains(" → ");
                let label = fk_short(def);
                let what = if renamed { "renamed".to_string() } else { x.name.clone() };
                let tip = if renamed { format!("foreign key renamed: {}\n{def}", x.name) } else { format!("{}: {def}", x.name) };
                extra.push(mk(RowKind::ForeignKey, &label, what, x.status, false, tip));
            }
            for pr in &td.properties {
                let def = format!("{} → {}", pr.old.as_deref().unwrap_or("∅"), pr.new.as_deref().unwrap_or("∅"));
                extra.push(mk(RowKind::Constraint, &pr.field, def.clone(), Status::Modified, false, format!("{} changed: {def}", pr.field)));
            }
        }
    }
    if !extra.is_empty() {
        rows.push(mk(RowKind::Section, "indexes & constraints", String::new(), Status::Unchanged, false, String::new()));
        rows.extend(extra);
    }
    (rows, hidden, total)
}

/// `(account_id) -> accounts(id) ON DELETE …` → `account_id → accounts`
fn fk_short(sig: &str) -> String {
    let cols = sig.find('(').and_then(|a| sig[a..].find(')').map(|b| &sig[a + 1..a + b])).unwrap_or("");
    let table = sig.find("-> ").map(|i| &sig[i + 3..]).map(|t| t.split('(').next().unwrap_or(t)).unwrap_or("");
    format!("{cols} → {table}")
}

/// One line saying what a diff changed in a table, for hover text.
fn change_summary(td: &crate::diff::TableDiff) -> Option<String> {
    if td.status != Status::Modified {
        return None;
    }
    let mut parts = Vec::new();
    let count = |items: &[Status], label: &str, out: &mut Vec<String>| {
        let (mut a, mut r, mut m) = (0, 0, 0);
        for s in items {
            match s {
                Status::Added => a += 1,
                Status::Removed => r += 1,
                _ => m += 1,
            }
        }
        let plural = |n: usize| if n == 1 { label.to_string() } else { format!("{label}s") };
        if a > 0 {
            out.push(format!("{a} {} added", plural(a)));
        }
        if r > 0 {
            out.push(format!("{r} {} removed", plural(r)));
        }
        if m > 0 {
            out.push(format!("{m} {} changed", plural(m)));
        }
    };
    count(&td.columns.iter().map(|c| c.status).collect::<Vec<_>>(), "column", &mut parts);
    let renamed = td.foreign_keys.iter().filter(|f| f.name.contains(" → ")).count();
    if renamed > 0 {
        parts.push(format!("{renamed} foreign key{} renamed", if renamed == 1 { "" } else { "s" }));
    }
    count(&td.foreign_keys.iter().filter(|f| !f.name.contains(" → ")).map(|f| f.status).collect::<Vec<_>>(), "foreign key", &mut parts);
    count(&td.indexes.iter().map(|i| i.status).collect::<Vec<_>>(), "index", &mut parts);
    count(&td.constraints.iter().map(|c| c.status).collect::<Vec<_>>(), "constraint", &mut parts);
    for p in &td.properties {
        parts.push(format!("{} changed", p.field));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn short_default(d: &str) -> String {
    if d.starts_with("nextval(") {
        return "serial".into();
    }
    // `'foo'::character varying` → `'foo'`
    match d.find("::") {
        Some(i) if d.starts_with('\'') => d[..i].to_string(),
        _ => d.to_string(),
    }
}

pub fn size_node(n: &mut Node) {
    let mut name_w: f64 = 0.0;
    let mut type_w: f64 = 0.0;
    let mut h = HEADER_H;
    for r in &n.rows {
        let nw = r.name.chars().count() as f64 * CHAR_W;
        match r.kind {
            RowKind::Section => {
                h += SECTION_H;
                continue;
            }
            RowKind::More => {
                name_w = name_w.max(nw * 0.9 - BADGE_W);
                h += ROW_H;
                continue;
            }
            _ => {}
        }
        name_w = name_w.max(nw);
        let mut tw = r.data_type.chars().count() as f64 * CHAR_W;
        if let Some(o) = &r.old_type {
            tw += (o.chars().count() as f64 + 3.0) * CHAR_W;
        }
        if let Some(d) = &r.default {
            tw += (d.chars().count() as f64 + 3.0) * CHAR_W;
        }
        type_w = type_w.max(tw);
        h += ROW_H;
    }
    if !n.rows.is_empty() {
        h += BOTTOM_PAD;
    }
    let rows_w = PAD_X + BADGE_W + name_w + GAP + type_w + NULL_W + PAD_X;
    let mut badge = 0.0;
    if n.status.is_changed() {
        badge += 70.0;
    }
    if n.kind != NodeKind::Table {
        badge += 50.0;
    }
    if n.partitions > 0 {
        badge += 40.0;
    }
    let header_w = PAD_X * 2.0 + n.label.chars().count() as f64 * HEADER_CHAR_W + badge + 8.0;
    n.width = rows_w.max(header_w).clamp(MIN_W, MAX_W).ceil();
    n.height = h.ceil();
}

/// Detached helper used by the session to describe hidden neighbours.
pub fn neighbours(g: &Graph) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in &g.edges {
        m.entry(e.from.clone()).or_default().push(e.to.clone());
        m.entry(e.to.clone()).or_default().push(e.from.clone());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn cols(g: &Graph, id: &str) -> Vec<String> {
        let n = g.nodes.iter().find(|n| n.id == id).unwrap();
        n.rows.iter().filter(|r| r.kind == RowKind::Column).map(|r| r.name.clone()).collect()
    }

    #[test]
    fn changed_enums_are_drawn_with_their_columns() {
        let base = parse("CREATE TYPE public.task_status AS ENUM ('todo', 'done');
            CREATE TABLE tasks (id bigint PRIMARY KEY, status public.task_status, title text);
            CREATE TABLE tags (id bigint PRIMARY KEY, name text);");
        let cur = parse("CREATE TYPE public.task_status AS ENUM ('todo', 'in_progress', 'done');
            CREATE TABLE tasks (id bigint PRIMARY KEY, status public.task_status, title text);
            CREATE TABLE tags (id bigint PRIMARY KEY, name text);");
        let d = crate::diff::diff(&base, &cur);
        let g = build(&cur, Some(&base), Some(&d), &ViewConfig::default());
        let en = g.nodes.iter().find(|n| n.kind == NodeKind::Enum).expect("enum node");
        assert_eq!(en.id, "public.task_status");
        assert_eq!(en.status, Status::Modified);
        let added: Vec<&str> = en.rows.iter().filter(|r| r.status == Status::Added).map(|r| r.name.as_str()).collect();
        assert_eq!(added, vec!["in_progress"]);
        let tasks = g.nodes.iter().find(|n| n.id == "public.tasks").unwrap();
        assert_eq!(tasks.status, Status::Modified);
        assert!(tasks.change_summary.as_deref().unwrap().contains("status uses changed enum task_status (+'in_progress')"));
        let status_row = tasks.rows.iter().find(|r| r.name == "status").unwrap();
        assert_eq!(status_row.status, Status::Modified);
        assert!(g.edges.iter().any(|e| e.kind == EdgeKind::EnumUse && e.from == "public.tasks" && e.to == "public.task_status" && e.status == Status::Modified));
        // changes only: the affected table shows even without neighbours
        let g2 = build(&cur, Some(&base), Some(&d), &ViewConfig { changes_only: true, changes_context: 0, ..Default::default() });
        let mut ids: Vec<&str> = g2.nodes.iter().map(|n| n.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["public.task_status", "public.tasks"]);
        // without a diff, enums stay hidden by default
        assert!(build(&cur, None, None, &ViewConfig::default()).nodes.iter().all(|n| n.kind != NodeKind::Enum));
    }

    #[test]
    fn unchanged_tables_show_only_referenced_columns() {
        let base_sql = "CREATE TABLE users (id bigint PRIMARY KEY, email text, name text);
            CREATE TABLE teams (id bigint PRIMARY KEY, title text);
            CREATE TABLE posts (id bigint PRIMARY KEY, user_id bigint REFERENCES users(id), body text);";
        let new_sql = "CREATE TABLE users (id bigint PRIMARY KEY, email text, name text);
            CREATE TABLE teams (id bigint PRIMARY KEY, title text);
            CREATE TABLE posts (id bigint PRIMARY KEY, user_id bigint REFERENCES users(id),
              team_id bigint REFERENCES teams(id), body text, title text);";
        let (base, cur) = (parse(base_sql), parse(new_sql));
        let d = crate::diff::diff(&base, &cur);
        // the default for unchanged tables in a diff is `referenced`
        let mut cfg = ViewConfig::default();
        let g = build(&cur, Some(&base), Some(&d), &cfg);
        // changed table keeps every column
        assert_eq!(cols(&g, "public.posts"), vec!["id", "user_id", "team_id", "body", "title"]);
        // unchanged neighbours only show what the drawn relations use
        assert_eq!(cols(&g, "public.users"), vec!["id"]);
        assert_eq!(cols(&g, "public.teams"), vec!["id"]);
        let users = g.nodes.iter().find(|n| n.id == "public.users").unwrap();
        assert_eq!(users.hidden_columns, 2);

        // opting out shows the normal columns again
        let all = ViewConfig { unchanged_columns: None, ..Default::default() };
        assert_eq!(cols(&build(&cur, Some(&base), Some(&d), &all), "public.users"), vec!["id", "email", "name"]);

        // per-table overrides still win
        cfg.tables.insert("users".into(), TableOverride { columns: Some(ColumnMode::All), ..Default::default() });
        let g = build(&cur, Some(&base), Some(&d), &cfg);
        assert_eq!(cols(&g, "public.users"), vec!["id", "email", "name"]);

        // without a diff the setting has no effect
        let g = build(&cur, None, None, &ViewConfig { unchanged_columns: Some(ColumnMode::Referenced), ..Default::default() });
        assert_eq!(cols(&g, "public.users").len(), 3);

        // per-pattern neighbour depths, and why the rest is hidden
    let cfg2 = ViewConfig {
        focus: vec!["posts".into(), "teams".into()],
        focus_depth: 0,
        focus_depths: [("posts".to_string(), 1)].into_iter().collect(),
        ..Default::default()
    };
    let g2 = build(&cur, None, None, &cfg2);
    let mut ids: Vec<&str> = g2.nodes.iter().map(|n| n.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["public.posts", "public.teams", "public.users"]);
    let g3 = build(&cur, None, None, &ViewConfig { focus: vec!["teams".into()], focus_depth: 0, ..Default::default() });
    assert_eq!(g3.nodes.len(), 1);
    assert_eq!(g3.stats.hidden.outside_focus, 2);
    assert_eq!(g3.stats.focus_matches.get("teams"), Some(&1));
    // the per-pattern depth overrides the global one: teams + its neighbour posts
    let g4 = build(&cur, None, None, &ViewConfig { focus: vec!["teams".into()], focus_depth: 0, focus_depths: [("teams".to_string(), 1)].into_iter().collect(), ..Default::default() });
    let mut ids4: Vec<&str> = g4.nodes.iter().map(|n| n.id.as_str()).collect();
    ids4.sort();
    assert_eq!(ids4, vec!["public.posts", "public.teams"]);

    // `referenced` as the global mode: hidden relations don't count
        let cfg = ViewConfig { columns: ColumnMode::Referenced, exclude: vec!["teams".into()], ..Default::default() };
        let g = build(&cur, None, None, &cfg);
        assert_eq!(cols(&g, "public.posts"), vec!["user_id"]);
    }
}
