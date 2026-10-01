//! Schema designs: a list of edit operations applied on top of a loaded
//! schema. A design renders as a normal diff (base = loaded schema,
//! current = schema with the operations applied) and exports to a
//! Markdown spec, PostgreSQL DDL or JSON for agents to implement.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diff::{diff, Status};
use crate::model::*;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ColumnSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: String,
    #[serde(default = "yes")]
    pub nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl ColumnSpec {
    fn to_column(&self) -> Column {
        Column {
            name: self.name.clone(),
            data_type: self.data_type.trim().to_string(),
            nullable: self.nullable,
            default: self.default.clone().filter(|d| !d.trim().is_empty()),
            comment: self.comment.clone().filter(|c| !c.trim().is_empty()),
            ..Default::default()
        }
    }
}

/// One edit. Table references accept `name` or `schema.name`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    CreateTable {
        table: String,
        columns: Vec<ColumnSpec>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        primary_key: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },
    DropTable {
        table: String,
    },
    RenameTable {
        table: String,
        to: String,
    },
    AddColumn {
        table: String,
        column: ColumnSpec,
    },
    DropColumn {
        table: String,
        column: String,
    },
    RenameColumn {
        table: String,
        column: String,
        to: String,
    },
    AlterColumn {
        table: String,
        column: String,
        #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
        data_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nullable: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        drop_default: bool,
    },
    SetPrimaryKey {
        table: String,
        columns: Vec<String>,
    },
    AddForeignKey {
        table: String,
        columns: Vec<String>,
        references: String,
        #[serde(default)]
        ref_columns: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_delete: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    DropForeignKey {
        table: String,
        name: String,
    },
    AddIndex {
        table: String,
        columns: Vec<String>,
        #[serde(default)]
        unique: bool,
        #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
        predicate: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    DropIndex {
        table: String,
        name: String,
    },
    SetComment {
        table: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        column: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },
}

impl Op {
    pub fn table(&self) -> &str {
        match self {
            Op::CreateTable { table, .. }
            | Op::DropTable { table }
            | Op::RenameTable { table, .. }
            | Op::AddColumn { table, .. }
            | Op::DropColumn { table, .. }
            | Op::RenameColumn { table, .. }
            | Op::AlterColumn { table, .. }
            | Op::SetPrimaryKey { table, .. }
            | Op::AddForeignKey { table, .. }
            | Op::DropForeignKey { table, .. }
            | Op::AddIndex { table, .. }
            | Op::DropIndex { table, .. }
            | Op::SetComment { table, .. } => table,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DesignSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Git ref the design was started from (`WORKTREE`, a branch, a sha …).
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Design {
    pub version: u32,
    pub name: String,
    pub description: String,
    pub source: DesignSource,
    pub ops: Vec<Op>,
    /// Free-form notes per table id (intent, constraints, data migration hints).
    pub notes: BTreeMap<String, String>,
    /// Diagram positions while designing.
    pub positions: BTreeMap<String, [f64; 2]>,
    /// Snapshot of every existing table the design touches, taken from the
    /// schema the design was made against. Makes the design self-contained
    /// (exports and `check` don't need the original schema).
    pub base_tables: Vec<Table>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ApplyError {
    pub op: usize,
    pub message: String,
}

pub struct Applied {
    pub schema: Schema,
    pub errors: Vec<ApplyError>,
    /// Final table id → id in the base schema (`None` for created tables).
    pub origin: BTreeMap<String, Option<String>>,
    /// Base ids of dropped tables.
    pub dropped: Vec<String>,
}

fn qualify_ref(r: &str) -> String {
    let r = r.trim().trim_matches('"');
    if r.contains('.') {
        r.to_string()
    } else {
        qualify("public", r)
    }
}

fn find(s: &Schema, r: &str) -> Option<usize> {
    let q = qualify_ref(r);
    if let Some(i) = s.tables.iter().position(|t| t.id() == q) {
        return Some(i);
    }
    if !r.contains('.') {
        let hits: Vec<usize> = s.tables.iter().enumerate().filter(|(_, t)| t.name == r).map(|(i, _)| i).collect();
        if hits.len() == 1 {
            return Some(hits[0]);
        }
    }
    None
}

fn resolve_ref(s: &Schema, r: &str) -> String {
    find(s, r).map(|i| s.tables[i].id()).unwrap_or_else(|| qualify_ref(r))
}

pub fn default_fk_name(table: &str, columns: &[String]) -> String {
    format!("fk_{}_{}", split_id(&qualify_ref(table)).1, columns.join("_"))
}

pub fn default_index_name(table: &str, columns: &[String]) -> String {
    let cols: Vec<String> = columns.iter().map(|c| c.chars().map(|ch| if ch.is_alphanumeric() { ch } else { '_' }).collect()).collect();
    format!("index_{}_on_{}", split_id(&qualify_ref(table)).1, cols.join("_and_"))
}

fn index_definition(table_id: &str, name: &str, columns: &[String], unique: bool, predicate: Option<&str>) -> String {
    format!(
        "CREATE {}INDEX {} ON {} USING btree ({}){}",
        if unique { "UNIQUE " } else { "" },
        quote_ident(name),
        quote_qualified(table_id),
        columns.join(", "),
        predicate.map(|p| format!(" WHERE ({p})")).unwrap_or_default()
    )
}

/// Apply operations in order. Invalid operations are skipped and reported.
pub fn apply(base: &Schema, ops: &[Op]) -> Applied {
    let mut s = base.clone();
    let mut errors = Vec::new();
    let mut origin: BTreeMap<String, Option<String>> = s.tables.iter().map(|t| (t.id(), Some(t.id()))).collect();
    let mut dropped = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        let mut err = |m: String| errors.push(ApplyError { op: k, message: m });
        if let Op::CreateTable { table, columns, primary_key, comment } = op {
            let id = qualify_ref(table);
            if s.table(&id).is_some() {
                err(format!("table {} already exists", display_id(&id)));
                continue;
            }
            let (schema, name) = split_id(&id);
            let mut t = Table { schema: schema.to_string(), name: name.to_string(), comment: comment.clone(), ..Default::default() };
            for c in columns {
                if t.column(&c.name).is_some() {
                    err(format!("duplicate column {}", c.name));
                    continue;
                }
                t.columns.push(c.to_column());
            }
            if !primary_key.is_empty() {
                for c in primary_key {
                    if let Some(col) = t.column_mut(c) {
                        col.nullable = false;
                    } else {
                        err(format!("primary key column {c} is not defined"));
                    }
                }
                t.primary_key = Some(PrimaryKey { name: Some(format!("{name}_pkey")), columns: primary_key.clone() });
            }
            if !s.schemas.contains(&t.schema) {
                s.schemas.push(t.schema.clone());
            }
            origin.insert(id, None);
            s.tables.push(t);
            continue;
        }
        let Some(ti) = find(&s, op.table()) else {
            err(format!("table {} does not exist", op.table()));
            continue;
        };
        let tid = s.tables[ti].id();
        match op {
            Op::CreateTable { .. } => unreachable!(),
            Op::DropTable { .. } => {
                let removed = s.tables.remove(ti);
                for t in &mut s.tables {
                    let before = t.foreign_keys.len();
                    t.foreign_keys.retain(|f| f.ref_table != tid);
                    if t.foreign_keys.len() != before {
                        err(format!("dropping {} also removes foreign keys from {}", display_id(&tid), display_id(&t.id())));
                    }
                }
                if let Some(Some(b)) = origin.remove(&removed.id()) {
                    dropped.push(b);
                }
            }
            Op::RenameTable { to, .. } => {
                let to = to.trim().to_string();
                let new_id = if to.contains('.') { to.clone() } else { qualify(&s.tables[ti].schema, &to) };
                if s.table(&new_id).is_some() {
                    err(format!("table {} already exists", display_id(&new_id)));
                    continue;
                }
                let (sch, name) = split_id(&new_id);
                let (sch, name) = (sch.to_string(), name.to_string());
                let t = &mut s.tables[ti];
                if let Some(pk) = &mut t.primary_key {
                    if pk.name.as_deref() == Some(&format!("{}_pkey", t.name)) {
                        pk.name = Some(format!("{name}_pkey"));
                    }
                }
                t.schema = sch;
                t.name = name;
                for t in &mut s.tables {
                    for f in &mut t.foreign_keys {
                        if f.ref_table == tid {
                            f.ref_table = new_id.clone();
                        }
                    }
                }
                let o = origin.remove(&tid).flatten();
                origin.insert(new_id, o);
            }
            Op::AddColumn { column, .. } => {
                let t = &mut s.tables[ti];
                if t.column(&column.name).is_some() {
                    err(format!("column {}.{} already exists", display_id(&tid), column.name));
                } else {
                    t.columns.push(column.to_column());
                }
            }
            Op::DropColumn { column, .. } => {
                let t = &mut s.tables[ti];
                if t.column(column).is_none() {
                    err(format!("column {}.{column} does not exist", display_id(&tid)));
                    continue;
                }
                t.columns.retain(|c| &c.name != column);
                t.foreign_keys.retain(|f| !f.columns.contains(column));
                t.indexes.retain(|i| !i.columns.contains(column));
                t.uniques.retain(|u| !u.columns.contains(column));
                if t.primary_key.as_ref().is_some_and(|p| p.columns.contains(column)) {
                    t.primary_key = None;
                }
                for other in &mut s.tables {
                    let before = other.foreign_keys.len();
                    other.foreign_keys.retain(|f| !(f.ref_table == tid && f.ref_columns.contains(column)));
                    if other.foreign_keys.len() != before {
                        err(format!("dropping {}.{column} also removes foreign keys from {}", display_id(&tid), display_id(&other.id())));
                    }
                }
            }
            Op::RenameColumn { column, to, .. } => {
                let t = &mut s.tables[ti];
                if t.column(to).is_some() {
                    err(format!("column {}.{to} already exists", display_id(&tid)));
                    continue;
                }
                let Some(c) = t.column_mut(column) else {
                    err(format!("column {}.{column} does not exist", display_id(&tid)));
                    continue;
                };
                c.name = to.clone();
                let ren = |v: &mut Vec<String>| v.iter_mut().filter(|x| *x == column).for_each(|x| *x = to.clone());
                if let Some(pk) = &mut t.primary_key {
                    ren(&mut pk.columns);
                }
                t.foreign_keys.iter_mut().for_each(|f| ren(&mut f.columns));
                t.indexes.iter_mut().for_each(|i| ren(&mut i.columns));
                t.uniques.iter_mut().for_each(|u| ren(&mut u.columns));
                for other in &mut s.tables {
                    for f in &mut other.foreign_keys {
                        if f.ref_table == tid {
                            ren(&mut f.ref_columns);
                        }
                    }
                }
            }
            Op::AlterColumn { column, data_type, nullable, default, drop_default, .. } => {
                let Some(c) = s.tables[ti].column_mut(column) else {
                    err(format!("column {}.{column} does not exist", display_id(&tid)));
                    continue;
                };
                if let Some(t) = data_type.as_ref().filter(|t| !t.trim().is_empty()) {
                    c.data_type = t.trim().to_string();
                }
                if let Some(n) = nullable {
                    c.nullable = *n;
                }
                if *drop_default {
                    c.default = None;
                } else if let Some(d) = default {
                    c.default = Some(d.clone());
                }
            }
            Op::SetPrimaryKey { columns, .. } => {
                let t = &mut s.tables[ti];
                if let Some(missing) = columns.iter().find(|c| t.column(c).is_none()) {
                    err(format!("column {}.{missing} does not exist", display_id(&tid)));
                    continue;
                }
                if columns.is_empty() {
                    t.primary_key = None;
                } else {
                    for c in columns {
                        t.column_mut(c).unwrap().nullable = false;
                    }
                    let name = t.primary_key.as_ref().and_then(|p| p.name.clone()).unwrap_or_else(|| format!("{}_pkey", t.name));
                    t.primary_key = Some(PrimaryKey { name: Some(name), columns: columns.clone() });
                }
            }
            Op::AddForeignKey { columns, references, ref_columns, on_delete, name, .. } => {
                let ref_id = resolve_ref(&s, references);
                let ref_cols = if ref_columns.is_empty() {
                    s.table(&ref_id).and_then(|r| r.primary_key.as_ref()).map(|p| p.columns.clone()).unwrap_or_else(|| vec!["id".into()])
                } else {
                    ref_columns.clone()
                };
                if s.table(&ref_id).is_none() {
                    err(format!("referenced table {} does not exist", display_id(&ref_id)));
                }
                let t = &mut s.tables[ti];
                if let Some(missing) = columns.iter().find(|c| t.column(c).is_none()) {
                    err(format!("column {}.{missing} does not exist", display_id(&tid)));
                    continue;
                }
                t.foreign_keys.push(ForeignKey {
                    name: Some(name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| default_fk_name(&tid, columns))),
                    columns: columns.clone(),
                    ref_table: ref_id,
                    ref_columns: ref_cols,
                    on_delete: on_delete.clone().filter(|d| !d.is_empty()).map(|d| d.to_uppercase()),
                    on_update: None,
                    deferrable: false,
                });
            }
            Op::DropForeignKey { name, .. } => {
                let t = &mut s.tables[ti];
                let before = t.foreign_keys.len();
                t.foreign_keys.retain(|f| f.name.as_deref() != Some(name) && f.key() != *name);
                if t.foreign_keys.len() == before {
                    err(format!("foreign key {name} does not exist on {}", display_id(&tid)));
                }
            }
            Op::AddIndex { columns, unique, predicate, name, .. } => {
                let t = &mut s.tables[ti];
                if let Some(missing) = columns.iter().find(|c| t.column(c).is_none()) {
                    err(format!("column {}.{missing} does not exist", display_id(&tid)));
                    continue;
                }
                let name = name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| default_index_name(&tid, columns));
                if t.indexes.iter().any(|i| i.name == name) {
                    err(format!("index {name} already exists"));
                    continue;
                }
                let predicate = predicate.clone().filter(|p| !p.trim().is_empty());
                t.indexes.push(Index {
                    definition: index_definition(&tid, &name, columns, *unique, predicate.as_deref()),
                    name,
                    unique: *unique,
                    method: "btree".into(),
                    columns: columns.clone(),
                    include: vec![],
                    predicate,
                });
            }
            Op::DropIndex { name, .. } => {
                let t = &mut s.tables[ti];
                let before = t.indexes.len();
                t.indexes.retain(|i| &i.name != name);
                if t.indexes.len() == before {
                    err(format!("index {name} does not exist on {}", display_id(&tid)));
                }
            }
            Op::SetComment { column, comment, .. } => {
                let comment = comment.clone().filter(|c| !c.trim().is_empty());
                let t = &mut s.tables[ti];
                match column {
                    Some(c) => match t.column_mut(c) {
                        Some(col) => col.comment = comment,
                        None => err(format!("column {}.{c} does not exist", display_id(&tid))),
                    },
                    None => t.comment = comment,
                }
            }
        }
    }
    Applied { schema: s, errors, origin, dropped }
}

impl Design {
    /// Schema made of the base snapshot (what exports and checks start from).
    pub fn base_schema(&self) -> Schema {
        let mut s = Schema { tables: self.base_tables.clone(), ..Default::default() };
        for t in &s.tables {
            if !s.schemas.contains(&t.schema) {
                s.schemas.push(t.schema.clone());
            }
        }
        s
    }

    /// Re-capture `base_tables` from the schema the design is applied to:
    /// every table touched by an op, plus tables its ops reference.
    pub fn capture_base(&mut self, schema: &Schema) {
        let mut ids: Vec<String> = Vec::new();
        let mut renamed: BTreeMap<String, String> = BTreeMap::new();
        for op in &self.ops {
            let r = op.table();
            let id = renamed.get(&qualify_ref(r)).cloned().unwrap_or_else(|| resolve_ref(schema, r));
            if schema.table(&id).is_some() && !ids.contains(&id) {
                ids.push(id.clone());
            }
            if let Op::AddForeignKey { references, .. } = op {
                let rid = resolve_ref(schema, references);
                if schema.table(&rid).is_some() && !ids.contains(&rid) {
                    ids.push(rid);
                }
            }
            if let Op::RenameTable { to, .. } = op {
                let sch = split_id(&id).0.to_string();
                renamed.insert(if to.contains('.') { to.clone() } else { qualify(&sch, to) }, id.clone());
            }
        }
        // tables referencing dropped / renamed tables lose or change FKs too
        for op in &self.ops {
            if matches!(op, Op::DropTable { .. } | Op::RenameTable { .. } | Op::DropColumn { .. } | Op::RenameColumn { .. }) {
                let id = resolve_ref(schema, op.table());
                for t in &schema.tables {
                    if t.foreign_keys.iter().any(|f| f.ref_table == id) && !ids.contains(&t.id()) {
                        ids.push(t.id());
                    }
                }
            }
        }
        self.base_tables = ids.iter().filter_map(|id| schema.table(id).cloned()).collect();
    }

    pub fn slug(&self) -> String {
        slugify(&self.name)
    }
}

pub fn slugify(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "design".into()
    } else {
        s.chars().take(64).collect()
    }
}

// ---- SQL --------------------------------------------------------------------

const RESERVED: &[&str] = &[
    "all", "and", "any", "array", "as", "asc", "both", "case", "cast", "check", "collate", "column", "constraint", "create", "default",
    "desc", "distinct", "do", "else", "end", "except", "false", "for", "foreign", "from", "grant", "group", "having", "in", "into", "is",
    "join", "leading", "limit", "not", "null", "offset", "on", "only", "or", "order", "primary", "references", "select", "table", "then",
    "to", "true", "union", "unique", "user", "using", "when", "where", "window", "with", "position", "type", "key", "value",
];

pub fn quote_ident(s: &str) -> String {
    let simple = !s.is_empty()
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if simple && !RESERVED.contains(&s) {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
}

fn quote_qualified(id: &str) -> String {
    let (s, n) = split_id(id);
    format!("{}.{}", quote_ident(s), quote_ident(n))
}

fn sql_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn column_sql(c: &ColumnSpec) -> String {
    let mut s = format!("{} {}", quote_ident(&c.name), c.data_type.trim());
    if let Some(d) = c.default.as_ref().filter(|d| !d.trim().is_empty()) {
        s.push_str(&format!(" DEFAULT {d}"));
    }
    if !c.nullable {
        s.push_str(" NOT NULL");
    }
    s
}

fn cols(v: &[String]) -> String {
    v.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
}

/// PostgreSQL DDL for the design, in operation order.
pub fn to_sql(design: &Design) -> String {
    let mut s = Schema { tables: design.base_tables.clone(), ..Default::default() };
    let mut out = Vec::new();
    for op in &design.ops {
        let t = resolve_ref(&s, op.table());
        let q = quote_qualified(&t);
        let stmt = match op {
            Op::CreateTable { columns, primary_key, comment, .. } => {
                let t = qualify_ref(op.table());
                let q = quote_qualified(&t);
                let mut lines: Vec<String> = columns.iter().map(|c| format!("    {}", column_sql(c))).collect();
                if !primary_key.is_empty() {
                    lines.push(format!("    PRIMARY KEY ({})", cols(primary_key)));
                }
                let mut st = format!("CREATE TABLE {q} (\n{}\n);", lines.join(",\n"));
                if let Some(c) = comment.as_ref().filter(|c| !c.is_empty()) {
                    st.push_str(&format!("\nCOMMENT ON TABLE {q} IS {};", sql_str(c)));
                }
                for c in columns {
                    if let Some(cm) = c.comment.as_ref().filter(|c| !c.is_empty()) {
                        st.push_str(&format!("\nCOMMENT ON COLUMN {q}.{} IS {};", quote_ident(&c.name), sql_str(cm)));
                    }
                }
                st
            }
            Op::DropTable { .. } => format!("DROP TABLE {q};"),
            Op::RenameTable { to, .. } => format!("ALTER TABLE {q} RENAME TO {};", quote_ident(split_id(&qualify_ref(to)).1)),
            Op::AddColumn { column, .. } => format!("ALTER TABLE {q} ADD COLUMN {};", column_sql(column)),
            Op::DropColumn { column, .. } => format!("ALTER TABLE {q} DROP COLUMN {};", quote_ident(column)),
            Op::RenameColumn { column, to, .. } => format!("ALTER TABLE {q} RENAME COLUMN {} TO {};", quote_ident(column), quote_ident(to)),
            Op::AlterColumn { column, data_type, nullable, default, drop_default, .. } => {
                let c = quote_ident(column);
                let mut parts = Vec::new();
                if let Some(ty) = data_type.as_ref().filter(|t| !t.trim().is_empty()) {
                    parts.push(format!("ALTER COLUMN {c} TYPE {}", ty.trim()));
                }
                if *drop_default {
                    parts.push(format!("ALTER COLUMN {c} DROP DEFAULT"));
                } else if let Some(d) = default {
                    parts.push(format!("ALTER COLUMN {c} SET DEFAULT {d}"));
                }
                match nullable {
                    Some(false) => parts.push(format!("ALTER COLUMN {c} SET NOT NULL")),
                    Some(true) => parts.push(format!("ALTER COLUMN {c} DROP NOT NULL")),
                    None => {}
                }
                if parts.is_empty() {
                    String::new()
                } else {
                    format!("ALTER TABLE {q}\n    {};", parts.join(",\n    "))
                }
            }
            Op::SetPrimaryKey { columns, .. } => {
                let existing = s.table(&t).and_then(|t| t.primary_key.as_ref()).and_then(|p| p.name.clone());
                let mut st = String::new();
                if let Some(n) = existing {
                    st.push_str(&format!("ALTER TABLE {q} DROP CONSTRAINT {};\n", quote_ident(&n)));
                }
                if !columns.is_empty() {
                    st.push_str(&format!("ALTER TABLE {q} ADD PRIMARY KEY ({});", cols(columns)));
                }
                st.trim_end().to_string()
            }
            Op::AddForeignKey { columns, references, ref_columns, on_delete, name, .. } => {
                let ref_id = resolve_ref(&s, references);
                let ref_cols = if ref_columns.is_empty() {
                    s.table(&ref_id).and_then(|r| r.primary_key.as_ref()).map(|p| p.columns.clone()).unwrap_or_else(|| vec!["id".into()])
                } else {
                    ref_columns.clone()
                };
                let name = name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| default_fk_name(&t, columns));
                format!(
                    "ALTER TABLE {q}\n    ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}({}){};",
                    quote_ident(&name),
                    cols(columns),
                    quote_qualified(&ref_id),
                    cols(&ref_cols),
                    on_delete.as_ref().filter(|d| !d.is_empty()).map(|d| format!(" ON DELETE {}", d.to_uppercase())).unwrap_or_default()
                )
            }
            Op::DropForeignKey { name, .. } => format!("ALTER TABLE {q} DROP CONSTRAINT {};", quote_ident(name)),
            Op::AddIndex { columns, unique, predicate, name, .. } => {
                let name = name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| default_index_name(&t, columns));
                format!(
                    "CREATE {}INDEX {} ON {q} ({}){};",
                    if *unique { "UNIQUE " } else { "" },
                    quote_ident(&name),
                    cols(columns),
                    predicate.as_ref().filter(|p| !p.trim().is_empty()).map(|p| format!(" WHERE ({p})")).unwrap_or_default()
                )
            }
            Op::DropIndex { name, .. } => format!("DROP INDEX {}.{};", quote_ident(split_id(&t).0), quote_ident(name)),
            Op::SetComment { column, comment, .. } => {
                let target = match column {
                    Some(c) => format!("COLUMN {q}.{}", quote_ident(c)),
                    None => format!("TABLE {q}"),
                };
                format!("COMMENT ON {target} IS {};", comment.as_ref().filter(|c| !c.is_empty()).map(|c| sql_str(c)).unwrap_or_else(|| "NULL".into()))
            }
        };
        if !stmt.is_empty() {
            out.push(stmt);
        }
        // keep the scratch schema in sync so later ops resolve names / PKs
        s = apply(&s, std::slice::from_ref(op)).schema;
    }
    if out.is_empty() {
        return "-- no changes\n".into();
    }
    out.join("\n\n") + "\n"
}

// ---- Markdown spec ------------------------------------------------------------

fn describe(op: &Op) -> String {
    let c = |x: &ColumnSpec| {
        let mut s = format!("`{}` {}{}", x.name, x.data_type, if x.nullable { "" } else { " NOT NULL" });
        if let Some(d) = x.default.as_ref().filter(|d| !d.is_empty()) {
            s.push_str(&format!(" DEFAULT {d}"));
        }
        s
    };
    match op {
        Op::CreateTable { .. } => "create table".into(),
        Op::DropTable { .. } => "drop the table".into(),
        Op::RenameTable { to, .. } => format!("rename table to `{to}`"),
        Op::AddColumn { column, .. } => format!("add column {}", c(column)),
        Op::DropColumn { column, .. } => format!("drop column `{column}`"),
        Op::RenameColumn { column, to, .. } => format!("rename column `{column}` → `{to}`"),
        Op::AlterColumn { column, data_type, nullable, default, drop_default, .. } => {
            let mut parts = Vec::new();
            if let Some(t) = data_type {
                parts.push(format!("type → {t}"));
            }
            match nullable {
                Some(false) => parts.push("NOT NULL".into()),
                Some(true) => parts.push("nullable".into()),
                None => {}
            }
            if *drop_default {
                parts.push("drop default".into());
            } else if let Some(d) = default {
                parts.push(format!("default → {d}"));
            }
            format!("change column `{column}`: {}", parts.join(", "))
        }
        Op::SetPrimaryKey { columns, .. } => format!("set primary key ({})", columns.join(", ")),
        Op::AddForeignKey { columns, references, ref_columns, on_delete, .. } => format!(
            "add foreign key ({}) → `{}`{}{}",
            columns.join(", "),
            display_id(&qualify_ref(references)),
            if ref_columns.is_empty() { String::new() } else { format!("({})", ref_columns.join(", ")) },
            on_delete.as_ref().filter(|d| !d.is_empty()).map(|d| format!(" ON DELETE {}", d.to_uppercase())).unwrap_or_default()
        ),
        Op::DropForeignKey { name, .. } => format!("drop foreign key `{name}`"),
        Op::AddIndex { columns, unique, predicate, .. } => format!(
            "add {}index on ({}){}",
            if *unique { "unique " } else { "" },
            columns.join(", "),
            predicate.as_ref().filter(|p| !p.is_empty()).map(|p| format!(" WHERE {p}")).unwrap_or_default()
        ),
        Op::DropIndex { name, .. } => format!("drop index `{name}`"),
        Op::SetComment { column, comment, .. } => match column {
            Some(col) => format!("set comment on `{col}`: {}", comment.as_deref().unwrap_or("(none)")),
            None => format!("set table comment: {}", comment.as_deref().unwrap_or("(none)")),
        },
    }
}

fn table_md(t: &Table) -> String {
    let mut o = String::from("| Column | Type | Null | Default | Notes |\n|---|---|---|---|---|\n");
    for c in &t.columns {
        let mut notes = Vec::new();
        if t.is_pk(&c.name) {
            notes.push("PK".to_string());
        }
        for f in t.foreign_keys.iter().filter(|f| f.columns.contains(&c.name)) {
            notes.push(format!("FK → {}({})", display_id(&f.ref_table), f.ref_columns.join(", ")));
        }
        if let Some(cm) = &c.comment {
            notes.push(cm.clone());
        }
        o.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            c.name,
            c.data_type,
            if c.nullable { "yes" } else { "NOT NULL" },
            c.default.as_deref().map(|d| format!("`{}`", d.replace('|', "\\|"))).unwrap_or_default(),
            notes.join("; ").replace('|', "\\|")
        ));
    }
    o
}

pub fn to_markdown(design: &Design, generator: &str) -> String {
    spec(design, generator, true)
}

/// A self-contained prompt for a coding agent: instructions plus the whole
/// spec inline, so it works without access to the design files.
pub fn to_agent_prompt(design: &Design, generator: &str) -> String {
    let slug = design.slug();
    let mut o = format!(
        "Implement the database schema design \"{name}\" below using this project's migration tooling.\n\n\
Rules:\n\
- Don't edit the schema dump (`db/structure.sql` or `db/schema.rb`) by hand; it must be regenerated by running the migrations.\n\
- Keep the table, column and index names exactly as specified. Constraint and index names in the SQL are suggestions; follow the project's naming conventions if they differ.\n\
- Handle existing data: backfill before adding NOT NULL, and update code that still uses renamed or dropped tables and columns.\n\
- Follow the notes and ⚠️ warnings in the spec.\n\n\
Steps:\n\
1. Write migration(s) that make every change listed under \"Changes\" (the SQL section shows the intended end result in PostgreSQL).\n\
2. Run the migrations so the schema dump is regenerated (for Rails: `bin/rails db:migrate` updates `db/structure.sql` or `db/schema.rb`).\n\
3. Verify the regenerated dump against the spec, table by table: every new or changed table, column (type and nullability), primary key, foreign key and index must match, and dropped items must be gone. If the `schema` CLI is available, `schema design check {slug}` does this check for you; repeat until every table passes.\n\
4. Summarise what you changed and anything you intentionally did differently.\n\n---\n\n",
        name = design.name
    );
    o.push_str(&spec(design, generator, false));
    o
}

fn spec(design: &Design, generator: &str, include_howto: bool) -> String {
    let base = design.base_schema();
    let applied = apply(&base, &design.ops);
    let slug = design.slug();
    let mut o = format!("# Schema design: {}\n\n", design.name);
    if !design.description.trim().is_empty() {
        o.push_str(design.description.trim());
        o.push_str("\n\n");
    }
    let src = &design.source;
    o.push_str(&format!(
        "- **Source:** `{}`{}{}\n",
        src.file.as_deref().unwrap_or("structure.sql"),
        src.git_ref
            .as_deref()
            .map(|r| match r {
                "WORKTREE" => " (working tree)".to_string(),
                "INDEX" => " (index)".to_string(),
                r => format!(" at `{r}`"),
            })
            .unwrap_or_default(),
        src.commit.as_deref().map(|c| format!(" (commit `{c}`)")).unwrap_or_default()
    ));
    let (created, modified, dropped) = counts(design, &base);
    o.push_str(&format!(
        "- **Changes:** {} operations · {created} new tables · {modified} modified tables · {dropped} dropped tables\n- Generated by {generator}\n\n",
        design.ops.len(),
    ));
    if !applied.errors.is_empty() {
        o.push_str("> **Warnings:**\n");
        for e in &applied.errors {
            o.push_str(&format!("> - operation {}: {}\n", e.op + 1, e.message));
        }
        o.push('\n');
    }
    if include_howto {
        o.push_str(&format!(
        "## Implementing this design\n\n\
1. Treat this document as the spec. Implement it with the project's migration tooling (for Rails: `bin/rails generate migration …`), not by editing the schema dump by hand.\n\
2. Keep the table, column and index names below. Constraint and index names in the SQL are suggestions; follow the project's conventions if they differ.\n\
3. Consider data: backfill before adding `NOT NULL`, and check that renames and drops don't break code that still uses the old names.\n\
4. Run the migrations, regenerate the schema dump (for Rails: `bin/rails db:migrate` updates `db/structure.sql` or `db/schema.rb`), then verify with `schema design check {slug}`.\n\n"
        ));
    }
    o.push_str("## Changes\n\n");

    // group ops by the final id of the table they touch
    let mut order: Vec<String> = Vec::new();
    let mut by_table: BTreeMap<String, Vec<&Op>> = BTreeMap::new();
    let mut alias: BTreeMap<String, String> = BTreeMap::new(); // any id → first id
    for op in &design.ops {
        let id = resolve_ref(&base, op.table());
        let key = alias.get(&id).cloned().unwrap_or_else(|| id.clone());
        if let Op::RenameTable { to, .. } = op {
            let new_id = if to.contains('.') { to.clone() } else { qualify(split_id(&id).0, to) };
            alias.insert(new_id, key.clone());
        }
        if !order.contains(&key) {
            order.push(key.clone());
        }
        by_table.entry(key).or_default().push(op);
    }
    let final_id = |first: &str| -> String {
        let mut cur = first.to_string();
        for (new, old) in &alias {
            if *old == first {
                cur = new.clone();
            }
        }
        cur
    };
    for key in &order {
        let ops = &by_table[key];
        let fid = final_id(key);
        let created = ops.iter().any(|o| matches!(o, Op::CreateTable { .. }));
        let dropped = ops.iter().any(|o| matches!(o, Op::DropTable { .. }));
        let note = design.notes.get(key).or_else(|| design.notes.get(&fid));
        if dropped && !created {
            o.push_str(&format!("### Drop table `{}`\n\n", display_id(key)));
            if let Some(n) = note {
                o.push_str(&format!("{n}\n\n"));
            }
            let refs: Vec<String> = base
                .tables
                .iter()
                .filter(|t| t.foreign_keys.iter().any(|f| f.ref_table == *key))
                .map(|t| format!("`{}`", display_id(&t.id())))
                .collect();
            if !refs.is_empty() {
                o.push_str(&format!("Referenced by {} — those foreign keys are removed too.\n\n", refs.join(", ")));
            }
            continue;
        }
        if dropped && created {
            continue;
        }
        let Some(t) = applied.schema.table(&fid) else { continue };
        if created {
            o.push_str(&format!("### New table `{}`\n\n", display_id(&fid)));
            if let Some(c) = &t.comment {
                o.push_str(&format!("_{c}_\n\n"));
            }
            if let Some(n) = note {
                o.push_str(&format!("{n}\n\n"));
            }
            o.push_str(&table_md(t));
            if let Some(pk) = &t.primary_key {
                o.push_str(&format!("\nPrimary key: ({})\n", pk.columns.join(", ")));
            }
            if !t.foreign_keys.is_empty() {
                o.push_str("\nForeign keys:\n");
                for f in &t.foreign_keys {
                    o.push_str(&format!(
                        "- ({}) → `{}`({}){}\n",
                        f.columns.join(", "),
                        display_id(&f.ref_table),
                        f.ref_columns.join(", "),
                        f.on_delete.as_deref().map(|d| format!(" ON DELETE {d}")).unwrap_or_default()
                    ));
                }
            }
            if !t.indexes.is_empty() {
                o.push_str("\nIndexes:\n");
                for i in &t.indexes {
                    o.push_str(&format!(
                        "- {}({}){}\n",
                        if i.unique { "UNIQUE " } else { "" },
                        i.columns.join(", "),
                        i.predicate.as_deref().map(|p| format!(" WHERE {p}")).unwrap_or_default()
                    ));
                }
            }
            o.push('\n');
        } else {
            let title = if fid != *key { format!("`{}` (renamed to `{}`)", display_id(key), display_id(&fid)) } else { format!("`{}`", display_id(key)) };
            o.push_str(&format!("### Modify {title}\n\n"));
            if let Some(n) = note {
                o.push_str(&format!("{n}\n\n"));
            }
            for op in ops {
                o.push_str(&format!("- {}\n", describe(op)));
            }
            for op in ops {
                let risk = match op {
                    Op::AlterColumn { column, nullable: Some(false), .. } => Some(format!("`{column}` becomes NOT NULL: backfill existing rows first")),
                    Op::AlterColumn { column, data_type: Some(_), .. } => Some(format!("`{column}` changes type: check existing data converts")),
                    Op::AddColumn { column, .. } if !column.nullable && column.default.as_ref().is_none_or(|d| d.trim().is_empty()) => {
                        Some(format!("`{}` is NOT NULL without a default: existing rows need a value", column.name))
                    }
                    Op::DropColumn { column, .. } => Some(format!("dropping `{column}` deletes its data")),
                    _ => None,
                };
                if let Some(r) = risk {
                    o.push_str(&format!("- ⚠️ {r}\n"));
                }
            }
            o.push('\n');
        }
    }
    o.push_str("## SQL (PostgreSQL)\n\n```sql\n");
    o.push_str(&to_sql(design));
    o.push_str("```\n\n## Operations (JSON)\n\n```json\n");
    o.push_str(&serde_json::to_string_pretty(&design.ops).unwrap_or_default());
    o.push_str("\n```\n");
    o
}

/// (created, modified, dropped) table counts, treating renames as modifications.
fn counts(design: &Design, base: &Schema) -> (usize, usize, usize) {
    let applied = apply(base, &design.ops);
    let created = applied.origin.values().filter(|o| o.is_none()).count();
    let dropped = applied.dropped.len();
    let modified = applied
        .origin
        .iter()
        .filter(|(fid, o)| o.as_ref().is_some_and(|o| o != *fid || d_changed(base, &applied.schema, o, fid)))
        .count();
    (created, modified, dropped)
}

// ---- check -------------------------------------------------------------------

/// Canonical spelling of a type, so `varchar(255)` matches
/// `character varying(255)` and Rails' `timestamp(6)` matches `timestamp`.
pub fn canonical_type(t: &str) -> String {
    let s = short_type(&t.trim().to_lowercase());
    let aliases = [
        ("timestamptz(6)", "timestamptz"),
        ("timestamp(6)", "timestamp"),
        ("int4", "int"),
        ("int8", "bigint"),
        ("int2", "smallint"),
        ("bigserial", "bigint"),
        ("serial8", "bigint"),
        ("smallserial", "smallint"),
        ("serial4", "int"),
        ("serial", "int"),
        ("real", "float4"),
        ("decimal", "numeric"),
    ];
    // compare the element type of arrays too (`int4[]`)
    let (head, tail) = match s.find('[') {
        Some(i) => (&s[..i], &s[i..]),
        None => (s.as_str(), ""),
    };
    let head = aliases.iter().find(|(a, _)| *a == head).map_or(head, |(_, b)| *b);
    format!("{head}{tail}").replace(' ', "")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Done,
    Missing,
    Differs,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckItem {
    pub table: String,
    pub status: CheckStatus,
    pub details: Vec<String>,
}

fn fk_sig(f: &ForeignKey) -> String {
    format!("({}) -> {}({})", f.columns.join(","), f.ref_table, f.ref_columns.join(","))
}

fn idx_sig(i: &Index) -> String {
    format!("{}({}){}", if i.unique { "UNIQUE " } else { "" }, i.columns.join(","), i.predicate.as_deref().map(|p| format!(" WHERE {}", p.replace(['(', ')', ' '], ""))).unwrap_or_default())
}

/// Compare what the design expects against an actual schema (typically the
/// regenerated `structure.sql` after running migrations).
pub fn check(design: &Design, actual: &Schema) -> Vec<CheckItem> {
    let base = design.base_schema();
    let applied = apply(&base, &design.ops);
    let expected = &applied.schema;
    let mut out = Vec::new();
    // tables that should be gone
    for b in &applied.dropped {
        let gone = actual.table(b).is_none();
        out.push(CheckItem {
            table: b.clone(),
            status: if gone { CheckStatus::Done } else { CheckStatus::Missing },
            details: if gone { vec!["dropped".into()] } else { vec!["table should be dropped".into()] },
        });
    }
    let mut touched: Vec<String> = Vec::new();
    for (fid, orig) in &applied.origin {
        let changed = match orig {
            None => true,
            Some(o) => o != fid || d_changed(&base, expected, o, fid),
        };
        if changed {
            touched.push(fid.clone());
        }
    }
    for fid in touched {
        let exp = expected.table(&fid).unwrap();
        let orig = applied.origin.get(&fid).cloned().flatten();
        let base_t = orig.as_ref().and_then(|o| base.table(o));
        let mut details = Vec::new();
        let Some(act) = actual.table(&fid) else {
            out.push(CheckItem { table: fid.clone(), status: CheckStatus::Missing, details: vec![if orig.is_some() && orig.as_deref() != Some(&fid) { format!("table should be renamed from {}", display_id(orig.as_deref().unwrap())) } else { "table does not exist".into() }] });
            continue;
        };
        if let (Some(o), true) = (&orig, orig.as_deref() != Some(fid.as_str())) {
            if actual.table(o).is_some() {
                details.push(format!("old table {} still exists", display_id(o)));
            }
        }
        for c in &exp.columns {
            match act.column(&c.name) {
                None => details.push(format!("missing column {}", c.name)),
                Some(a) => {
                    if canonical_type(&a.data_type) != canonical_type(&c.data_type) {
                        details.push(format!("column {}: type is {}, expected {}", c.name, a.data_type, c.data_type));
                    }
                    if a.nullable != c.nullable {
                        details.push(format!("column {}: {}, expected {}", c.name, if a.nullable { "nullable" } else { "NOT NULL" }, if c.nullable { "nullable" } else { "NOT NULL" }));
                    }
                }
            }
        }
        if let Some(b) = base_t {
            for c in &b.columns {
                if exp.column(&c.name).is_none() && act.column(&c.name).is_some() {
                    details.push(format!("column {} should be removed", c.name));
                }
            }
        }
        let pk = |t: &Table| t.primary_key.as_ref().map(|p| p.columns.clone()).unwrap_or_default();
        if pk(exp) != pk(act) {
            details.push(format!("primary key is ({}), expected ({})", pk(act).join(", "), pk(exp).join(", ")));
        }
        let act_fks: Vec<String> = act.foreign_keys.iter().map(fk_sig).collect();
        for f in &exp.foreign_keys {
            if !act_fks.contains(&fk_sig(f)) {
                details.push(format!("missing foreign key ({}) → {}", f.columns.join(", "), display_id(&f.ref_table)));
            }
        }
        let exp_fks: Vec<String> = exp.foreign_keys.iter().map(fk_sig).collect();
        if let Some(b) = base_t {
            for f in &b.foreign_keys {
                let s = fk_sig(f);
                if !exp_fks.contains(&s) && act_fks.contains(&s) {
                    details.push(format!("foreign key ({}) → {} should be removed", f.columns.join(", "), display_id(&f.ref_table)));
                }
            }
        }
        let act_idx: Vec<String> = act.indexes.iter().map(idx_sig).collect();
        for i in &exp.indexes {
            if !act_idx.contains(&idx_sig(i)) {
                details.push(format!("missing {}index on ({})", if i.unique { "unique " } else { "" }, i.columns.join(", ")));
            }
        }
        let exp_idx: Vec<String> = exp.indexes.iter().map(idx_sig).collect();
        if let Some(b) = base_t {
            for i in &b.indexes {
                let s = idx_sig(i);
                if !exp_idx.contains(&s) && act_idx.contains(&s) {
                    details.push(format!("index {} should be removed", i.name));
                }
            }
        }
        let status = if details.is_empty() {
            CheckStatus::Done
        } else if orig.is_none() && details.iter().all(|d| d.starts_with("missing")) && details.len() >= exp.columns.len() {
            CheckStatus::Missing
        } else {
            CheckStatus::Differs
        };
        out.push(CheckItem { table: fid, status, details });
    }
    out
}

fn d_changed(base: &Schema, expected: &Schema, orig: &str, fid: &str) -> bool {
    match (base.table(orig), expected.table(fid)) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

/// Label used in the UI for an op.
pub fn op_label(op: &Op) -> String {
    format!("{}: {}", display_id(&qualify_ref(op.table())), describe(op))
}

pub fn summary_status(design: &Design) -> Vec<(String, Status)> {
    let base = design.base_schema();
    let applied = apply(&base, &design.ops);
    diff(&base, &applied.schema).tables.into_iter().map(|t| (t.id, t.status)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn base() -> Schema {
        parse(
            "CREATE TABLE public.users (id bigint NOT NULL, email character varying(255), name text);
             ALTER TABLE ONLY public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);
             CREATE TABLE public.posts (id bigint NOT NULL, user_id bigint, body text);
             ALTER TABLE ONLY public.posts ADD CONSTRAINT posts_pkey PRIMARY KEY (id);
             ALTER TABLE ONLY public.posts ADD CONSTRAINT fk_rails_1 FOREIGN KEY (user_id) REFERENCES public.users(id);
             CREATE INDEX index_posts_on_user_id ON public.posts USING btree (user_id);",
        )
    }

    fn ops() -> Vec<Op> {
        serde_json::from_str(
            r#"[
            {"op":"create_table","table":"cards","columns":[
                {"name":"id","type":"bigserial","nullable":false},
                {"name":"user_id","type":"bigint","nullable":false},
                {"name":"last4","type":"varchar(4)"}],"primary_key":["id"],"comment":"Payment cards"},
            {"op":"add_foreign_key","table":"cards","columns":["user_id"],"references":"users","on_delete":"cascade"},
            {"op":"add_index","table":"cards","columns":["user_id","last4"],"unique":true},
            {"op":"add_column","table":"users","column":{"name":"locale","type":"varchar(10)","nullable":false,"default":"'en'"}},
            {"op":"rename_column","table":"users","column":"name","to":"full_name"},
            {"op":"alter_column","table":"posts","column":"body","nullable":false},
            {"op":"rename_table","table":"posts","to":"articles"},
            {"op":"drop_index","table":"articles","name":"index_posts_on_user_id"}
        ]"#,
        )
        .unwrap()
    }

    #[test]
    fn applies_ops() {
        let a = apply(&base(), &ops());
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        let s = a.schema;
        let cards = s.table("public.cards").unwrap();
        assert_eq!(cards.columns.len(), 3);
        assert_eq!(cards.foreign_keys[0].ref_table, "public.users");
        assert_eq!(cards.foreign_keys[0].ref_columns, vec!["id"]);
        assert_eq!(cards.foreign_keys[0].on_delete.as_deref(), Some("CASCADE"));
        assert!(cards.indexes[0].unique);
        let users = s.table("public.users").unwrap();
        assert!(users.column("full_name").is_some() && users.column("name").is_none());
        assert!(!users.column("locale").unwrap().nullable);
        let articles = s.table("public.articles").unwrap();
        assert!(s.table("public.posts").is_none());
        assert!(!articles.column("body").unwrap().nullable);
        assert!(articles.indexes.is_empty());
        assert_eq!(a.origin.get("public.articles"), Some(&Some("public.posts".to_string())));
        assert_eq!(a.origin.get("public.cards"), Some(&None));
    }

    #[test]
    fn reports_invalid_ops() {
        let bad: Vec<Op> = serde_json::from_str(r#"[{"op":"drop_column","table":"users","column":"nope"},{"op":"add_column","table":"ghosts","column":{"name":"x","type":"int"}}]"#).unwrap();
        let a = apply(&base(), &bad);
        assert_eq!(a.errors.len(), 2);
        assert_eq!(a.errors[1].op, 1);
    }

    #[test]
    fn exports_sql_and_markdown() {
        let mut d = Design { name: "Cards v1".into(), ops: ops(), ..Default::default() };
        d.capture_base(&base());
        assert_eq!(d.slug(), "cards-v1");
        assert_eq!(d.base_tables.len(), 2);
        let sql = to_sql(&d);
        assert!(sql.contains("CREATE TABLE public.cards (\n    id bigserial NOT NULL,"), "{sql}");
        assert!(sql.contains("PRIMARY KEY (id)"));
        assert!(sql.contains("ADD CONSTRAINT fk_cards_user_id FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;"), "{sql}");
        assert!(sql.contains("CREATE UNIQUE INDEX index_cards_on_user_id_and_last4 ON public.cards (user_id, last4);"));
        assert!(sql.contains("ALTER TABLE public.users RENAME COLUMN name TO full_name;"));
        assert!(sql.contains("ALTER TABLE public.posts RENAME TO articles;"));
        assert!(sql.contains("DROP INDEX public.index_posts_on_user_id;"), "{sql}");
        let md = to_markdown(&d, "test");
        assert!(md.contains("### New table `cards`"), "{md}");
        assert!(md.contains("### Modify `posts` (renamed to `articles`)"), "{md}");
        assert!(md.contains("`body` becomes NOT NULL"), "{md}");
        assert!(md.contains("1 new tables · 2 modified tables · 0 dropped tables"), "{md}");
        assert!(md.contains("schema design check cards-v1"));
        let prompt = to_agent_prompt(&d, "test");
        assert!(prompt.starts_with("Implement the database schema design \"Cards v1\""));
        assert!(prompt.contains("### New table `cards`") && prompt.contains("```sql") && prompt.contains("\"op\": \"create_table\""));
        assert!(!prompt.contains(".schema/designs") && !prompt.contains("## Implementing this design"), "{prompt}");
        // round trip
        let json = serde_json::to_string(&d).unwrap();
        let back: Design = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn checks_implementation() {
        let mut d = Design { name: "x".into(), ops: ops(), ..Default::default() };
        d.capture_base(&base());
        // nothing implemented yet
        let items = check(&d, &base());
        assert!(items.iter().any(|i| i.table == "public.cards" && i.status == CheckStatus::Missing));
        assert!(items.iter().all(|i| i.status != CheckStatus::Done));
        // fully implemented, with pg_dump spellings and Rails-style names
        let done = parse(
            "CREATE TABLE public.users (id bigint NOT NULL, email character varying(255), full_name text, locale character varying(10) DEFAULT 'en'::character varying NOT NULL);
             ALTER TABLE ONLY public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);
             CREATE TABLE public.articles (id bigint NOT NULL, user_id bigint, body text NOT NULL);
             ALTER TABLE ONLY public.articles ADD CONSTRAINT articles_pkey PRIMARY KEY (id);
             ALTER TABLE ONLY public.articles ADD CONSTRAINT fk_rails_1 FOREIGN KEY (user_id) REFERENCES public.users(id);
             CREATE TABLE public.cards (id bigint NOT NULL, user_id bigint NOT NULL, last4 character varying(4));
             ALTER TABLE ONLY public.cards ADD CONSTRAINT cards_pkey PRIMARY KEY (id);
             ALTER TABLE ONLY public.cards ADD CONSTRAINT fk_rails_9 FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;
             CREATE UNIQUE INDEX index_cards_on_user_id_and_last4 ON public.cards USING btree (user_id, last4);",
        );
        let items = check(&d, &done);
        assert!(items.iter().all(|i| i.status == CheckStatus::Done), "{items:#?}");
        // partially implemented
        let partial = parse("CREATE TABLE public.cards (id bigint NOT NULL, user_id integer);");
        let items = check(&d, &partial);
        let cards = items.iter().find(|i| i.table == "public.cards").unwrap();
        assert_eq!(cards.status, CheckStatus::Differs);
        assert!(cards.details.iter().any(|x| x.contains("missing column last4")));
        assert!(cards.details.iter().any(|x| x.contains("type is integer, expected bigint")));
    }

    #[test]
    fn canonical_types() {
        assert_eq!(canonical_type("character varying(255)"), canonical_type("varchar(255)"));
        assert_eq!(canonical_type("timestamp(6) without time zone"), canonical_type("timestamp"));
        assert_eq!(canonical_type("integer"), canonical_type("int4"));
        assert_eq!(canonical_type("bigserial"), "bigint");
        assert_ne!(canonical_type("integer"), canonical_type("bigint"));
    }
}
