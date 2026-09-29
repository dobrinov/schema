//! View configuration. Every field has a sensible default so partial JSON
//! (from the UI, a `.schema.json` file or an LLM) is always valid.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColumnMode {
    /// All columns for small diagrams, keys only for large ones.
    #[default]
    Auto,
    All,
    /// Primary, foreign and unique key columns (plus changed columns in a diff).
    Keys,
    /// Only columns taking part in relations (PK + FK).
    Relations,
    /// Only columns used by the relations drawn in the diagram (the FK
    /// columns pointing out and the columns other visible tables reference).
    Referenced,
    /// Only changed columns (diff mode); falls back to keys otherwise.
    Changed,
    /// Header only.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IndexMode {
    None,
    /// Show added / removed / modified indexes only (diff mode).
    #[default]
    Changed,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FocusDirection {
    #[default]
    Both,
    /// Tables the focused tables reference (parents).
    Outgoing,
    /// Tables referencing the focused tables (children).
    Incoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    /// Sugiyama-style layered layout; parents before children.
    #[default]
    Layered,
    /// Force-directed (Fruchterman–Reingold) with overlap removal.
    Force,
    /// Alphabetical grid.
    Grid,
    /// Nodes on a circle.
    Circular,
    /// Concentric rings around focused / most connected tables.
    Radial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum Direction {
    #[default]
    LR,
    RL,
    TB,
    BT,
}

impl Direction {
    pub fn horizontal(self) -> bool {
        matches!(self, Direction::LR | Direction::RL)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    #[default]
    None,
    Schema,
    /// First `_`-separated segment of the table name (`billing_invoices` → `billing`).
    Prefix,
    /// Groups defined in `groups`.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GridSort {
    #[default]
    Name,
    Degree,
    Size,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LayoutConfig {
    pub algorithm: Algorithm,
    pub direction: Direction,
    /// Gap between nodes within a layer / grid row.
    pub node_spacing: f64,
    /// Gap between layers (layered) or ideal edge length (force).
    pub rank_spacing: f64,
    pub group_by: GroupBy,
    /// Lay out connected components separately and pack them.
    pub pack_components: bool,
    /// Max tables per layer in the layered layout (0 = auto).
    pub max_layer_width: usize,
    /// Force-directed iterations.
    pub iterations: usize,
    pub grid_sort: GridSort,
    /// With a focus and the layered algorithm, arrange the neighbourhood
    /// around the focused tables (referenced tables on one side, referencing
    /// tables on the other, ordered to avoid crossing relations).
    pub focus_layout: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        LayoutConfig {
            algorithm: Algorithm::Layered,
            direction: Direction::LR,
            node_spacing: 36.0,
            rank_spacing: 110.0,
            group_by: GroupBy::None,
            pack_components: true,
            max_layer_width: 0,
            iterations: 300,
            grid_sort: GridSort::Name,
            focus_layout: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EdgeStyle {
    #[default]
    Curved,
    Orthogonal,
    Straight,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EdgeAnchor {
    /// Attach edges to the FK / PK column rows.
    #[default]
    Column,
    /// Attach edges to table borders.
    Table,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EdgeConfig {
    pub style: EdgeStyle,
    pub anchor: EdgeAnchor,
    /// Show constraint names on edges.
    pub labels: bool,
    /// Crow's-foot cardinality markers.
    pub cardinality: bool,
    /// Infer relations from `<singular>_id` columns when no FK exists.
    pub inferred: bool,
    pub self_loops: bool,
    /// Dashed edges from views to the tables they read.
    pub view_dependencies: bool,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        EdgeConfig {
            style: EdgeStyle::Curved,
            anchor: EdgeAnchor::Column,
            labels: false,
            cardinality: true,
            inferred: false,
            self_loops: true,
            view_dependencies: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct TableOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<ColumnMode>,
    /// Column patterns to hide for this table.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hide_columns: Vec<String>,
    /// Column patterns always shown for this table (wins over hiding rules).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub show_columns: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct GroupDef {
    pub name: String,
    /// Table patterns belonging to this group.
    pub tables: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ViewConfig {
    /// Table patterns to include (empty = everything). Globs: `*`, `?`;
    /// patterns without a dot match table names in any schema.
    pub include: Vec<String>,
    /// Table patterns to exclude.
    pub exclude: Vec<String>,
    /// Restrict to these schemas (empty = all).
    pub schemas: Vec<String>,
    /// Show only these tables and their neighbourhood.
    pub focus: Vec<String>,
    pub focus_depth: u32,
    /// Neighbour depth per focus pattern (overrides `focus_depth`).
    pub focus_depths: BTreeMap<String, u32>,
    pub focus_direction: FocusDirection,
    /// In a diff, show only changed tables (plus `changes_context` hops).
    pub changes_only: bool,
    pub changes_context: u32,
    pub show_views: bool,
    /// Show partitions as separate tables instead of folding them into the parent.
    pub show_partitions: bool,
    /// Show tables without any visible relation.
    pub show_isolated: bool,
    pub columns: ColumnMode,
    /// In a diff, column mode for tables that did not change (the context
    /// around the changes). Defaults to `referenced`; `None` = same as `columns`.
    pub unchanged_columns: Option<ColumnMode>,
    /// Column patterns hidden everywhere (`created_at`, `*_at`, `users.encrypted_*`).
    pub hide_columns: Vec<String>,
    /// Truncate tables to this many column rows (0 = no limit).
    pub max_columns: usize,
    pub show_types: bool,
    pub short_types: bool,
    pub show_nullable: bool,
    pub show_defaults: bool,
    pub indexes: IndexMode,
    pub tables: BTreeMap<String, TableOverride>,
    pub layout: LayoutConfig,
    pub edges: EdgeConfig,
    pub groups: Vec<GroupDef>,
    /// Manually pinned node positions (top-left corner).
    pub positions: BTreeMap<String, [f64; 2]>,
    pub theme: Theme,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

pub const DEFAULT_EXCLUDES: &[&str] = &["schema_migrations", "ar_internal_metadata"];

impl Default for ViewConfig {
    fn default() -> Self {
        ViewConfig {
            include: vec![],
            exclude: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
            schemas: vec![],
            focus: vec![],
            focus_depth: 1,
            focus_depths: BTreeMap::new(),
            focus_direction: FocusDirection::Both,
            changes_only: false,
            changes_context: 1,
            show_views: false,
            show_partitions: false,
            show_isolated: true,
            columns: ColumnMode::Auto,
            unchanged_columns: Some(ColumnMode::Referenced),
            hide_columns: vec![],
            max_columns: 0,
            show_types: true,
            short_types: true,
            show_nullable: true,
            show_defaults: false,
            indexes: IndexMode::Changed,
            tables: BTreeMap::new(),
            layout: LayoutConfig::default(),
            edges: EdgeConfig::default(),
            groups: vec![],
            positions: BTreeMap::new(),
            theme: Theme::Light,
            title: None,
        }
    }
}

impl ViewConfig {
    pub fn from_json(s: &str) -> Result<ViewConfig, String> {
        if s.trim().is_empty() {
            return Ok(ViewConfig::default());
        }
        serde_json::from_str(s).map_err(|e| format!("invalid view config: {e}"))
    }

    /// Per-table override for a table id, matching patterns as well as ids.
    pub fn table_override(&self, id: &str) -> Option<&TableOverride> {
        if let Some(o) = self.tables.get(id) {
            return Some(o);
        }
        self.tables.iter().find(|(k, _)| crate::glob::table_matches(k, id)).map(|(_, v)| v)
    }
}

/// Deep-merge `patch` into `base` (JSON objects merge, everything else replaces).
pub fn merge_json(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                merge_json(b.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (b, p) => *b = p.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_uses_defaults() {
        let c = ViewConfig::from_json(r#"{"layout": {"algorithm": "force"}, "columns": "keys"}"#).unwrap();
        assert_eq!(c.layout.algorithm, Algorithm::Force);
        assert_eq!(c.layout.rank_spacing, 110.0);
        assert_eq!(c.columns, ColumnMode::Keys);
        assert_eq!(c.exclude, vec!["schema_migrations", "ar_internal_metadata"]);
    }
}
