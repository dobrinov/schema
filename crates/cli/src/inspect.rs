//! `schema inspect`: compact, LLM-friendly schema descriptions.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use schema_core::glob::{glob_match, table_matches};
use schema_core::model::{display_id, Schema, Table};
use serde_json::{json, Value};

pub fn resolve_table<'a>(s: &'a Schema, pat: &str) -> Vec<&'a Table> {
    if let Some(t) = s.table(pat).or_else(|| s.table(&format!("public.{pat}"))) {
        return vec![t];
    }
    s.tables.iter().filter(|t| table_matches(pat, &t.id())).collect()
}

fn referenced_by(s: &Schema, id: &str) -> Vec<(String, String, String)> {
    let mut v = Vec::new();
    for t in &s.tables {
        for f in &t.foreign_keys {
            if f.ref_table == id {
                v.push((t.id(), f.columns.join(", "), f.ref_columns.join(", ")));
            }
        }
    }
    v
}

pub fn summary_text(s: &Schema, name: &str) -> String {
    let mut o = format!(
        "{name}: {} tables, {} views, {} enums, {} functions, schemas: {}\n\n",
        s.tables.len(),
        s.views.len(),
        s.enums.len(),
        s.functions.len(),
        s.schemas.join(", ")
    );
    let mut incoming: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in &s.tables {
        for f in &t.foreign_keys {
            incoming.entry(f.ref_table.clone()).or_default().insert(display_id(&t.id()).to_string());
        }
    }
    for t in &s.tables {
        let id = t.id();
        let out: BTreeSet<String> = t.foreign_keys.iter().map(|f| display_id(&f.ref_table).to_string()).collect();
        o.push_str(&format!("{} ({} cols)", display_id(&id), t.columns.len()));
        if let Some(p) = &t.partition_of {
            o.push_str(&format!(" partition of {}", display_id(p)));
        }
        if !out.is_empty() {
            o.push_str(&format!("  → {}", out.into_iter().collect::<Vec<_>>().join(", ")));
        }
        if let Some(inc) = incoming.get(&id) {
            o.push_str(&format!("  ← {}", inc.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        o.push('\n');
    }
    if !s.views.is_empty() {
        o.push_str("\nviews:\n");
        for v in &s.views {
            o.push_str(&format!(
                "{}{} reads {}\n",
                display_id(&v.id()),
                if v.materialized { " (materialized)" } else { "" },
                v.depends_on.iter().map(|d| display_id(d)).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    if !s.enums.is_empty() {
        o.push_str("\nenums:\n");
        for e in &s.enums {
            o.push_str(&format!("{}: {}\n", display_id(&e.id()), e.values.join(" | ")));
        }
    }
    o
}

pub fn table_text(s: &Schema, t: &Table) -> String {
    let id = t.id();
    let mut o = format!("TABLE {}", display_id(&id));
    if let Some(c) = &t.comment {
        o.push_str(&format!(" — {c}"));
    }
    o.push('\n');
    if let Some(p) = &t.partition_by {
        o.push_str(&format!("  partitioned by {p}\n"));
    }
    if let Some(p) = &t.partition_of {
        o.push_str(&format!("  partition of {}\n", display_id(p)));
    }
    let w = t.columns.iter().map(|c| c.name.len()).max().unwrap_or(4);
    let tw = t.columns.iter().map(|c| c.data_type.len()).max().unwrap_or(4).min(40);
    o.push_str("  columns:\n");
    for c in &t.columns {
        let mut flags = Vec::new();
        if t.is_pk(&c.name) {
            flags.push("PK".to_string());
        }
        for f in t.foreign_keys.iter().filter(|f| f.columns.contains(&c.name)) {
            flags.push(format!("FK → {}({})", display_id(&f.ref_table), f.ref_columns.join(", ")));
        }
        if t.is_unique(&c.name) && !t.is_pk(&c.name) {
            flags.push("UNIQUE".into());
        }
        if let Some(d) = &c.default {
            flags.push(format!("default {d}"));
        }
        if let Some(i) = &c.identity {
            flags.push(format!("identity {i}"));
        }
        if let Some(g) = &c.generated {
            flags.push(format!("generated ({g})"));
        }
        if let Some(cm) = &c.comment {
            flags.push(format!("-- {cm}"));
        }
        o.push_str(&format!(
            "    {:w$}  {:tw$}  {:8}  {}\n",
            c.name,
            c.data_type,
            if c.nullable { "NULL" } else { "NOT NULL" },
            flags.join("  "),
        ));
    }
    if !t.indexes.is_empty() {
        o.push_str("  indexes:\n");
        for i in &t.indexes {
            o.push_str(&format!(
                "    {}{} ({}){}\n",
                if i.unique { "UNIQUE " } else { "" },
                i.name,
                i.columns.join(", "),
                i.predicate.as_deref().map(|p| format!(" WHERE {p}")).unwrap_or_default()
            ));
        }
    }
    for c in &t.checks {
        o.push_str(&format!("  check {}: {}\n", c.name.as_deref().unwrap_or(""), c.expression));
    }
    for u in &t.uniques {
        o.push_str(&format!("  unique {}: ({})\n", u.name.as_deref().unwrap_or(""), u.columns.join(", ")));
    }
    let refs = referenced_by(s, &id);
    if !refs.is_empty() {
        o.push_str("  referenced by:\n");
        for (t, c, rc) in refs {
            o.push_str(&format!("    {}.{} → ({rc})\n", display_id(&t), c));
        }
    }
    let trig: Vec<_> = s.triggers.iter().filter(|x| x.table == id).collect();
    for tr in trig {
        o.push_str(&format!("  trigger: {}\n", tr.definition));
    }
    let views: Vec<_> = s.views.iter().filter(|v| v.depends_on.contains(&id)).map(|v| display_id(&v.id()).to_string()).collect();
    if !views.is_empty() {
        o.push_str(&format!("  used by views: {}\n", views.join(", ")));
    }
    o
}

pub fn neighbourhood(s: &Schema, seeds: &[String], depth: u32) -> Vec<(String, u32)> {
    let mut adj: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in &s.tables {
        for f in &t.foreign_keys {
            adj.entry(t.id()).or_default().insert(f.ref_table.clone());
            adj.entry(f.ref_table.clone()).or_default().insert(t.id());
        }
    }
    let mut seen: BTreeMap<String, u32> = seeds.iter().map(|s| (s.clone(), 0)).collect();
    let mut q: VecDeque<(String, u32)> = seeds.iter().map(|s| (s.clone(), 0)).collect();
    while let Some((id, d)) = q.pop_front() {
        if d >= depth {
            continue;
        }
        for n in adj.get(&id).into_iter().flatten() {
            if !seen.contains_key(n) {
                seen.insert(n.clone(), d + 1);
                q.push_back((n.clone(), d + 1));
            }
        }
    }
    let mut v: Vec<(String, u32)> = seen.into_iter().collect();
    v.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    v
}

pub fn search(s: &Schema, q: &str) -> Vec<(String, Option<String>, String)> {
    let pat = if q.contains('*') || q.contains('?') { q.to_string() } else { format!("*{q}*") };
    let mut out = Vec::new();
    for t in &s.tables {
        let id = t.id();
        if glob_match(&pat, &t.name) {
            out.push((id.clone(), None, format!("{} columns", t.columns.len())));
        }
        for c in &t.columns {
            if glob_match(&pat, &c.name) {
                out.push((id.clone(), Some(c.name.clone()), c.data_type.clone()));
            }
        }
    }
    out
}

pub fn compact_json(s: &Schema) -> Value {
    json!({
        "schemas": s.schemas,
        "tables": s.tables.iter().map(|t| json!({
            "id": t.id(),
            "comment": t.comment,
            "partition_of": t.partition_of,
            "primary_key": t.primary_key.as_ref().map(|p| &p.columns),
            "columns": t.columns.iter().map(|c| json!({"name": c.name, "type": c.data_type, "nullable": c.nullable, "default": c.default})).collect::<Vec<_>>(),
            "foreign_keys": t.foreign_keys.iter().map(|f| json!({"columns": f.columns, "references": f.ref_table, "ref_columns": f.ref_columns, "on_delete": f.on_delete})).collect::<Vec<_>>(),
            "indexes": t.indexes.iter().map(|i| json!({"name": i.name, "unique": i.unique, "columns": i.columns, "where": i.predicate})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "views": s.views.iter().map(|v| json!({"id": v.id(), "materialized": v.materialized, "depends_on": v.depends_on})).collect::<Vec<_>>(),
        "enums": s.enums.iter().map(|e| json!({"id": e.id(), "values": e.values})).collect::<Vec<_>>(),
        "warnings": s.warnings,
    })
}
