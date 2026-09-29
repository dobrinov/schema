//! Parsed representation of a Postgres schema.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Schema {
    pub schemas: Vec<String>,
    pub extensions: Vec<Extension>,
    pub enums: Vec<EnumType>,
    pub tables: Vec<Table>,
    pub views: Vec<View>,
    pub sequences: Vec<Sequence>,
    pub functions: Vec<Function>,
    pub triggers: Vec<Trigger>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Table {
    pub schema: String,
    pub name: String,
    pub columns: Vec<Column>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_key: Option<PrimaryKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foreign_keys: Vec<ForeignKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uniques: Vec<UniqueConstraint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckConstraint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<Index>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_of: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherits: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unlogged: bool,
}

impl Table {
    pub fn id(&self) -> String {
        qualify(&self.schema, &self.name)
    }
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }
    pub fn column_mut(&mut self, name: &str) -> Option<&mut Column> {
        self.columns.iter_mut().find(|c| c.name == name)
    }
    pub fn is_pk(&self, col: &str) -> bool {
        self.primary_key.as_ref().is_some_and(|p| p.columns.iter().any(|c| c == col))
    }
    pub fn is_fk(&self, col: &str) -> bool {
        self.foreign_keys.iter().any(|f| f.columns.iter().any(|c| c == col))
    }
    /// Column is covered by a single-column unique constraint / unique index.
    pub fn is_unique(&self, col: &str) -> bool {
        let single = |cols: &[String]| cols.len() == 1 && cols[0] == col;
        self.uniques.iter().any(|u| single(&u.columns))
            || self.indexes.iter().any(|i| i.unique && i.predicate.is_none() && single(&i.columns))
            || self.primary_key.as_ref().is_some_and(|p| single(&p.columns))
    }
    /// Column is the leading column of any index.
    pub fn is_indexed(&self, col: &str) -> bool {
        self.indexes.iter().any(|i| i.columns.first().is_some_and(|c| c == col))
            || self.uniques.iter().any(|u| u.columns.first().is_some_and(|c| c == col))
            || self.primary_key.as_ref().is_some_and(|p| p.columns.first().is_some_and(|c| c == col))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PrimaryKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ForeignKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub columns: Vec<String>,
    /// Qualified id (`schema.table`) of the referenced table.
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_delete: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_update: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deferrable: bool,
}

impl ForeignKey {
    /// A name-independent description used for matching and display.
    pub fn signature(&self) -> String {
        let mut s = format!(
            "({}) -> {}({})",
            self.columns.join(", "),
            display_id(&self.ref_table),
            self.ref_columns.join(", ")
        );
        if let Some(d) = &self.on_delete {
            s.push_str(&format!(" ON DELETE {d}"));
        }
        if let Some(u) = &self.on_update {
            s.push_str(&format!(" ON UPDATE {u}"));
        }
        if self.deferrable {
            s.push_str(" DEFERRABLE");
        }
        s
    }
    pub fn key(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.signature())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UniqueConstraint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CheckConstraint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub expression: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Index {
    pub name: String,
    pub unique: bool,
    pub method: String,
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    pub definition: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct View {
    pub schema: String,
    pub name: String,
    pub materialized: bool,
    pub definition: String,
    /// Qualified ids of tables / views referenced by the query.
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<Index>,
    #[serde(skip)]
    pub raw_refs: Vec<String>,
}

impl View {
    pub fn id(&self) -> String {
        qualify(&self.schema, &self.name)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EnumType {
    pub schema: String,
    pub name: String,
    pub values: Vec<String>,
}

impl EnumType {
    pub fn id(&self) -> String {
        qualify(&self.schema, &self.name)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Extension {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Sequence {
    pub schema: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Function {
    pub schema: String,
    pub name: String,
    pub arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub definition: String,
}

impl Function {
    pub fn id(&self) -> String {
        format!("{}({})", qualify(&self.schema, &self.name), self.arguments)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Trigger {
    pub name: String,
    pub table: String,
    pub definition: String,
}

impl Trigger {
    pub fn id(&self) -> String {
        format!("{} ON {}", self.name, self.table)
    }
}

pub fn qualify(schema: &str, name: &str) -> String {
    format!("{schema}.{name}")
}

/// Hide the default `public.` prefix for display purposes.
pub fn display_id(id: &str) -> &str {
    id.strip_prefix("public.").unwrap_or(id)
}

/// Split `schema.name` ids. Names never contain unescaped dots in our ids,
/// but quoted identifiers could, so split on the first dot only.
pub fn split_id(id: &str) -> (&str, &str) {
    match id.find('.') {
        Some(i) => (&id[..i], &id[i + 1..]),
        None => ("public", id),
    }
}

impl Schema {
    pub fn table(&self, id: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.id() == id)
    }
    pub fn view(&self, id: &str) -> Option<&View> {
        self.views.iter().find(|v| v.id() == id)
    }
}

/// Shorter, more readable spellings of common Postgres types.
pub fn short_type(t: &str) -> String {
    let mut s = t.trim().to_string();
    for p in ["public.", "pg_catalog."] {
        s = s.replace(p, "");
    }
    let rules: [(&str, &str); 10] = [
        ("character varying", "varchar"),
        ("timestamp with time zone", "timestamptz"),
        ("timestamp without time zone", "timestamp"),
        ("time with time zone", "timetz"),
        ("time without time zone", "time"),
        ("double precision", "float8"),
        ("bit varying", "varbit"),
        ("character", "char"),
        ("boolean", "bool"),
        ("integer", "int"),
    ];
    for (from, to) in rules {
        s = s.replace(from, to);
    }
    // `timestamp(6) without time zone` → `timestamp(6)`
    s = s.replace(" without time zone", "");
    if let Some(i) = s.find(" with time zone") {
        let head = &s[..i];
        if let Some(rest) = head.strip_prefix("timestamp") {
            s = format!("timestamptz{rest}");
        } else if let Some(rest) = head.strip_prefix("time") {
            s = format!("timetz{rest}");
        }
    }
    s
}
