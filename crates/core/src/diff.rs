//! Structural diff between two parsed schemas.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lexer::normalize_ws;
use crate::model::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Unchanged,
    Added,
    Removed,
    Modified,
}

impl Status {
    pub fn is_changed(self) -> bool {
        self != Status::Unchanged
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Unchanged => "unchanged",
            Status::Added => "added",
            Status::Removed => "removed",
            Status::Modified => "modified",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldChange {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnDiff {
    pub name: String,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<FieldChange>,
}

/// Diff of a named item described by a definition string (index, FK, check …).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemDiff {
    pub name: String,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableDiff {
    pub id: String,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ColumnDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foreign_keys: Vec<ItemDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<ItemDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<ItemDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<FieldChange>,
}

impl TableDiff {
    pub fn column(&self, name: &str) -> Option<&ColumnDiff> {
        self.columns.iter().find(|c| c.name == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DiffSummary {
    pub tables_added: usize,
    pub tables_removed: usize,
    pub tables_modified: usize,
    pub columns_added: usize,
    pub columns_removed: usize,
    pub columns_modified: usize,
    pub foreign_keys_added: usize,
    pub foreign_keys_removed: usize,
    pub indexes_added: usize,
    pub indexes_removed: usize,
    pub indexes_modified: usize,
    pub other_changes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SchemaDiff {
    pub tables: Vec<TableDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<ItemDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enums: Vec<ItemDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<ItemDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<ItemDiff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<ItemDiff>,
    pub summary: DiffSummary,
}

impl SchemaDiff {
    pub fn table(&self, id: &str) -> Option<&TableDiff> {
        self.tables.iter().find(|t| t.id == id)
    }
    pub fn table_status(&self, id: &str) -> Status {
        self.table(id).map_or(Status::Unchanged, |t| t.status)
    }
    pub fn view_status(&self, id: &str) -> Status {
        self.views.iter().find(|v| v.name == id).map_or(Status::Unchanged, |v| v.status)
    }
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
            && self.views.is_empty()
            && self.enums.is_empty()
            && self.functions.is_empty()
            && self.triggers.is_empty()
            && self.extensions.is_empty()
    }
}

fn opt_change(field: &str, old: &Option<String>, new: &Option<String>, out: &mut Vec<FieldChange>) {
    if old != new {
        out.push(FieldChange { field: field.into(), old: old.clone(), new: new.clone() });
    }
}

fn diff_column(old: &Column, new: &Column) -> Vec<FieldChange> {
    let mut ch = Vec::new();
    if normalize_ws(&old.data_type) != normalize_ws(&new.data_type) {
        ch.push(FieldChange { field: "type".into(), old: Some(old.data_type.clone()), new: Some(new.data_type.clone()) });
    }
    if old.nullable != new.nullable {
        let s = |n: bool| Some(if n { "NULL" } else { "NOT NULL" }.to_string());
        ch.push(FieldChange { field: "nullable".into(), old: s(old.nullable), new: s(new.nullable) });
    }
    opt_change("default", &old.default, &new.default, &mut ch);
    opt_change("identity", &old.identity, &new.identity, &mut ch);
    opt_change("generated", &old.generated, &new.generated, &mut ch);
    opt_change("collation", &old.collation, &new.collation, &mut ch);
    opt_change("comment", &old.comment, &new.comment, &mut ch);
    ch
}

/// Diff two maps of name → definition.
fn diff_items(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> Vec<ItemDiff> {
    let mut out = Vec::new();
    for (k, o) in old {
        match new.get(k) {
            None => out.push(ItemDiff { name: k.clone(), status: Status::Removed, old: Some(o.clone()), new: None }),
            Some(n) if normalize_ws(n) != normalize_ws(o) => {
                out.push(ItemDiff { name: k.clone(), status: Status::Modified, old: Some(o.clone()), new: Some(n.clone()) })
            }
            _ => {}
        }
    }
    for (k, n) in new {
        if !old.contains_key(k) {
            out.push(ItemDiff { name: k.clone(), status: Status::Added, old: None, new: Some(n.clone()) });
        }
    }
    // Items with identical definitions but different names are renames.
    let mut drop = vec![false; out.len()];
    let mut renames = Vec::new();
    for r in 0..out.len() {
        if out[r].status != Status::Removed {
            continue;
        }
        let def = normalize_ws(out[r].old.as_deref().unwrap_or_default());
        if let Some(a) = (0..out.len()).find(|&a| {
            !drop[a] && out[a].status == Status::Added && normalize_ws(out[a].new.as_deref().unwrap_or_default()) == def
        }) {
            drop[a] = true;
            drop[r] = true;
            renames.push(ItemDiff {
                name: format!("{} → {}", out[r].name, out[a].name),
                status: Status::Modified,
                old: out[r].old.clone(),
                new: out[a].new.clone(),
            });
        }
    }
    let mut kept: Vec<ItemDiff> = out.into_iter().zip(drop).filter(|(_, d)| !d).map(|(i, _)| i).collect();
    kept.extend(renames);
    kept
}

fn fk_map(t: &Table) -> BTreeMap<String, String> {
    t.foreign_keys.iter().map(|f| (f.key(), f.signature())).collect()
}

fn index_map(t: &Table) -> BTreeMap<String, String> {
    // Names are part of the definition; strip them so renames are detected as such.
    t.indexes
        .iter()
        .map(|i| (i.name.clone(), i.definition.replacen(&format!(" {} ON", i.name), " ON", 1)))
        .collect()
}

fn constraint_map(t: &Table) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if let Some(pk) = &t.primary_key {
        m.insert(pk.name.clone().unwrap_or_else(|| "primary key".into()), format!("PRIMARY KEY ({})", pk.columns.join(", ")));
    }
    for u in &t.uniques {
        let def = format!("UNIQUE ({})", u.columns.join(", "));
        m.insert(u.name.clone().unwrap_or_else(|| def.clone()), def);
    }
    for c in &t.checks {
        let def = format!("CHECK ({})", c.expression);
        m.insert(c.name.clone().unwrap_or_else(|| def.clone()), def);
    }
    m
}

pub fn diff_table(old: &Table, new: &Table) -> TableDiff {
    let mut columns = Vec::new();
    for oc in &old.columns {
        match new.column(&oc.name) {
            None => columns.push(ColumnDiff { name: oc.name.clone(), status: Status::Removed, changes: vec![] }),
            Some(nc) => {
                let changes = diff_column(oc, nc);
                if !changes.is_empty() {
                    columns.push(ColumnDiff { name: oc.name.clone(), status: Status::Modified, changes });
                }
            }
        }
    }
    for nc in &new.columns {
        if old.column(&nc.name).is_none() {
            columns.push(ColumnDiff { name: nc.name.clone(), status: Status::Added, changes: vec![] });
        }
    }
    let foreign_keys = diff_items(&fk_map(old), &fk_map(new));
    let indexes = diff_items(&index_map(old), &index_map(new));
    let constraints = diff_items(&constraint_map(old), &constraint_map(new));
    let mut properties = Vec::new();
    opt_change("comment", &old.comment, &new.comment, &mut properties);
    opt_change("partition_by", &old.partition_by, &new.partition_by, &mut properties);
    opt_change("partition_of", &old.partition_of, &new.partition_of, &mut properties);
    if old.inherits != new.inherits {
        properties.push(FieldChange {
            field: "inherits".into(),
            old: Some(old.inherits.join(", ")),
            new: Some(new.inherits.join(", ")),
        });
    }
    let changed = !columns.is_empty() || !foreign_keys.is_empty() || !indexes.is_empty() || !constraints.is_empty() || !properties.is_empty();
    TableDiff {
        id: new.id(),
        status: if changed { Status::Modified } else { Status::Unchanged },
        columns,
        foreign_keys,
        indexes,
        constraints,
        properties,
    }
}

fn whole_table(t: &Table, status: Status) -> TableDiff {
    let cols = t.columns.iter().map(|c| ColumnDiff { name: c.name.clone(), status, changes: vec![] }).collect();
    let items = |m: BTreeMap<String, String>| -> Vec<ItemDiff> {
        m.into_iter()
            .map(|(k, v)| {
                let (old, new) = if status == Status::Added { (None, Some(v)) } else { (Some(v), None) };
                ItemDiff { name: k, status, old, new }
            })
            .collect()
    };
    TableDiff {
        id: t.id(),
        status,
        columns: cols,
        foreign_keys: items(fk_map(t)),
        indexes: items(index_map(t)),
        constraints: items(constraint_map(t)),
        properties: vec![],
    }
}

pub fn diff(old: &Schema, new: &Schema) -> SchemaDiff {
    let mut d = SchemaDiff::default();
    let old_t: BTreeMap<String, &Table> = old.tables.iter().map(|t| (t.id(), t)).collect();
    let new_t: BTreeMap<String, &Table> = new.tables.iter().map(|t| (t.id(), t)).collect();
    for (id, ot) in &old_t {
        match new_t.get(id) {
            None => d.tables.push(whole_table(ot, Status::Removed)),
            Some(nt) => {
                let td = diff_table(ot, nt);
                if td.status.is_changed() {
                    d.tables.push(td);
                }
            }
        }
    }
    for (id, nt) in &new_t {
        if !old_t.contains_key(id) {
            d.tables.push(whole_table(nt, Status::Added));
        }
    }
    d.tables.sort_by(|a, b| a.id.cmp(&b.id));

    let views = |s: &Schema| -> BTreeMap<String, String> {
        s.views
            .iter()
            .map(|v| (v.id(), format!("{}VIEW AS {}", if v.materialized { "MATERIALIZED " } else { "" }, v.definition)))
            .collect()
    };
    d.views = diff_items(&views(old), &views(new));
    let enums = |s: &Schema| -> BTreeMap<String, String> {
        s.enums.iter().map(|e| (e.id(), format!("ENUM ({})", e.values.iter().map(|v| format!("'{v}'")).collect::<Vec<_>>().join(", ")))).collect()
    };
    d.enums = diff_items(&enums(old), &enums(new));
    let funcs = |s: &Schema| -> BTreeMap<String, String> { s.functions.iter().map(|f| (f.id(), f.definition.clone())).collect() };
    d.functions = diff_items(&funcs(old), &funcs(new));
    let trig = |s: &Schema| -> BTreeMap<String, String> { s.triggers.iter().map(|t| (t.id(), t.definition.clone())).collect() };
    d.triggers = diff_items(&trig(old), &trig(new));
    let ext = |s: &Schema| -> BTreeMap<String, String> {
        s.extensions.iter().map(|e| (e.name.clone(), format!("EXTENSION {}", e.name))).collect()
    };
    d.extensions = diff_items(&ext(old), &ext(new));

    let s = &mut d.summary;
    for t in &d.tables {
        match t.status {
            Status::Added => {
                s.tables_added += 1;
                s.foreign_keys_added += t.foreign_keys.len();
                s.indexes_added += t.indexes.len();
            }
            Status::Removed => {
                s.tables_removed += 1;
                s.foreign_keys_removed += t.foreign_keys.len();
                s.indexes_removed += t.indexes.len();
            }
            _ => {
                s.tables_modified += 1;
                for c in &t.columns {
                    match c.status {
                        Status::Added => s.columns_added += 1,
                        Status::Removed => s.columns_removed += 1,
                        _ => s.columns_modified += 1,
                    }
                }
                for f in &t.foreign_keys {
                    match f.status {
                        Status::Added => s.foreign_keys_added += 1,
                        Status::Removed => s.foreign_keys_removed += 1,
                        _ => {
                            s.foreign_keys_added += 1;
                            s.foreign_keys_removed += 1
                        }
                    }
                }
                for i in &t.indexes {
                    match i.status {
                        Status::Added => s.indexes_added += 1,
                        Status::Removed => s.indexes_removed += 1,
                        _ => s.indexes_modified += 1,
                    }
                }
                s.other_changes += t.constraints.len() + t.properties.len();
            }
        }
    }
    s.other_changes += d.views.len() + d.enums.len() + d.functions.len() + d.triggers.len() + d.extensions.len();
    d
}

/// Human readable markdown summary, handy for LLM consumption and PR descriptions.
pub fn to_markdown(d: &SchemaDiff) -> String {
    let mut o = String::new();
    let s = &d.summary;
    if d.is_empty() {
        return "No schema changes.\n".into();
    }
    o.push_str(&format!(
        "**Tables:** +{} −{} ~{} · **Columns:** +{} −{} ~{} · **FKs:** +{} −{} · **Indexes:** +{} −{} ~{}\n\n",
        s.tables_added,
        s.tables_removed,
        s.tables_modified,
        s.columns_added,
        s.columns_removed,
        s.columns_modified,
        s.foreign_keys_added,
        s.foreign_keys_removed,
        s.indexes_added,
        s.indexes_removed,
        s.indexes_modified
    ));
    let sym = |st: Status| match st {
        Status::Added => "+",
        Status::Removed => "−",
        Status::Modified => "~",
        Status::Unchanged => " ",
    };
    for t in &d.tables {
        o.push_str(&format!("### {} `{}` ({})\n", sym(t.status), display_id(&t.id), t.status.as_str()));
        if t.status == Status::Modified {
            for c in &t.columns {
                let details = c
                    .changes
                    .iter()
                    .map(|f| format!("{}: {} → {}", f.field, f.old.as_deref().unwrap_or("∅"), f.new.as_deref().unwrap_or("∅")))
                    .collect::<Vec<_>>()
                    .join("; ");
                o.push_str(&format!("- {} column `{}`{}\n", sym(c.status), c.name, if details.is_empty() { String::new() } else { format!(" — {details}") }));
            }
        } else {
            let cols: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
            o.push_str(&format!("- columns: {}\n", cols.join(", ")));
        }
        for (label, items) in [("foreign key", &t.foreign_keys), ("index", &t.indexes), ("constraint", &t.constraints)] {
            for i in items {
                let def = i.new.as_deref().or(i.old.as_deref()).unwrap_or_default();
                o.push_str(&format!("- {} {label} `{}`: {}\n", sym(i.status), i.name, def));
            }
        }
        for p in &t.properties {
            o.push_str(&format!("- ~ {}: {} → {}\n", p.field, p.old.as_deref().unwrap_or("∅"), p.new.as_deref().unwrap_or("∅")));
        }
        o.push('\n');
    }
    for (label, items) in [
        ("Views", &d.views),
        ("Enums", &d.enums),
        ("Functions", &d.functions),
        ("Triggers", &d.triggers),
        ("Extensions", &d.extensions),
    ] {
        if items.is_empty() {
            continue;
        }
        o.push_str(&format!("### {label}\n"));
        for i in items {
            o.push_str(&format!("- {} `{}` ({})\n", sym(i.status), display_id(&i.name), i.status.as_str()));
        }
        o.push('\n');
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn detects_changes() {
        let a = parse(
            "CREATE TABLE public.users (id bigint NOT NULL, email text, age int);
             CREATE TABLE public.old (id int);
             ALTER TABLE ONLY public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);
             CREATE INDEX idx_email ON public.users USING btree (email);",
        );
        let b = parse(
            "CREATE TABLE public.users (id bigint NOT NULL, email text NOT NULL, name text);
             CREATE TABLE public.posts (id int, user_id bigint);
             ALTER TABLE ONLY public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);
             CREATE INDEX idx_email_renamed ON public.users USING btree (email);
             ALTER TABLE ONLY public.posts ADD CONSTRAINT fk_1 FOREIGN KEY (user_id) REFERENCES public.users(id);",
        );
        let d = diff(&a, &b);
        assert_eq!(d.summary.tables_added, 1);
        assert_eq!(d.summary.tables_removed, 1);
        assert_eq!(d.summary.tables_modified, 1);
        let u = d.table("public.users").unwrap();
        assert_eq!(u.column("email").unwrap().status, Status::Modified);
        assert_eq!(u.column("age").unwrap().status, Status::Removed);
        assert_eq!(u.column("name").unwrap().status, Status::Added);
        assert_eq!(u.indexes.len(), 1);
        assert_eq!(u.indexes[0].name, "idx_email → idx_email_renamed");
        assert!(to_markdown(&d).contains("`posts`"));
    }
}
