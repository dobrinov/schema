//! Parser for Postgres DDL, primarily `pg_dump --schema-only` output
//! (Rails `structure.sql`), but tolerant of hand-written schema files.
//!
//! The parser never fails: unknown statements are skipped and malformed ones
//! produce warnings.
use std::collections::HashMap;

use crate::lexer::{normalize_ws, tokenize, Kind, Token};
use crate::model::*;

pub fn parse(src: &str) -> Schema {
    let tokens = tokenize(src);
    let mut p = State { schema: Schema::default(), default_schema: "public".into(), table_idx: HashMap::new() };
    for (a, b) in split_statements(&tokens) {
        let stmt = &tokens[a..b];
        if stmt.is_empty() {
            continue;
        }
        let mut c = Cur { t: stmt, i: 0, src };
        p.statement(&mut c);
    }
    p.resolve();
    p.schema
}

/// Split the token stream into statements on top-level semicolons, keeping
/// `BEGIN ATOMIC ... END` function bodies intact.
fn split_statements(t: &[Token]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut atomic = 0i32;
    let mut i = 0;
    while i < t.len() {
        let tok = &t[i];
        match tok.kind {
            Kind::Punct if tok.text == "(" => depth += 1,
            Kind::Punct if tok.text == ")" => depth = (depth - 1).max(0),
            Kind::Word if tok.text == "begin" && t.get(i + 1).map_or(false, |n| n.is_word("atomic")) => atomic += 1,
            Kind::Word if atomic > 0 && tok.text == "case" => atomic += 1,
            Kind::Word if atomic > 0 && tok.text == "end" => atomic -= 1,
            Kind::Punct if tok.text == ";" && depth == 0 && atomic == 0 => {
                out.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < t.len() {
        out.push((start, t.len()));
    }
    out
}

struct Cur<'a> {
    t: &'a [Token],
    i: usize,
    src: &'a str,
}

impl<'a> Cur<'a> {
    fn sub(&self, a: usize, b: usize) -> Cur<'a> {
        Cur { t: &self.t[a..b], i: 0, src: self.src }
    }
    fn done(&self) -> bool {
        self.i >= self.t.len()
    }
    fn peek(&self) -> Option<&'a Token> {
        self.t.get(self.i)
    }
    fn peek_at(&self, n: usize) -> Option<&'a Token> {
        self.t.get(self.i + n)
    }
    fn is_kw(&self, kw: &str) -> bool {
        self.peek().map_or(false, |t| t.is_word(kw))
    }
    fn is_kws(&self, kws: &[&str]) -> bool {
        kws.iter().enumerate().all(|(k, w)| self.peek_at(k).map_or(false, |t| t.is_word(w)))
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn eat_kws(&mut self, kws: &[&str]) -> bool {
        if self.is_kws(kws) {
            self.i += kws.len();
            true
        } else {
            false
        }
    }
    fn is_punct(&self, c: char) -> bool {
        self.peek().map_or(false, |t| t.is_punct(c))
    }
    fn eat_punct(&mut self, c: char) -> bool {
        if self.is_punct(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> Option<String> {
        let t = self.peek()?;
        if t.is_ident() {
            self.i += 1;
            Some(t.text.clone())
        } else {
            None
        }
    }
    /// Dotted name, e.g. `schema.table.column`.
    fn name_parts(&mut self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(first) = self.ident() {
            parts.push(first);
            while self.is_punct('.') && self.peek_at(1).map_or(false, |t| t.is_ident()) {
                self.i += 1;
                parts.push(self.ident().unwrap());
            }
        }
        parts
    }
    /// Index of the `)` matching the `(` at `self.i`.
    fn matching_paren(&self, open: usize) -> usize {
        let mut depth = 0;
        for k in open..self.t.len() {
            if self.t[k].is_punct('(') || self.t[k].is_punct('[') {
                depth += 1;
            } else if self.t[k].is_punct(')') || self.t[k].is_punct(']') {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
        }
        self.t.len()
    }
    /// If at `(`, return the token range inside the parens and advance past `)`.
    fn paren_group(&mut self) -> Option<(usize, usize)> {
        if !self.is_punct('(') {
            return None;
        }
        let open = self.i;
        let close = self.matching_paren(open);
        self.i = (close + 1).min(self.t.len());
        Some((open + 1, close.min(self.t.len())))
    }
    fn text(&self, a: usize, b: usize) -> String {
        if a >= b || a >= self.t.len() {
            return String::new();
        }
        let b = b.min(self.t.len());
        normalize_ws(&self.src[self.t[a].start..self.t[b - 1].end])
    }
    fn raw_text(&self, a: usize, b: usize) -> String {
        if a >= b || a >= self.t.len() {
            return String::new();
        }
        let b = b.min(self.t.len());
        self.src[self.t[a].start..self.t[b - 1].end].trim().to_string()
    }
    fn rest_text(&self) -> String {
        self.text(self.i, self.t.len())
    }
    /// Split a token range on top-level commas.
    fn split_commas(&self, a: usize, b: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut depth = 0;
        let mut s = a;
        for k in a..b {
            let t = &self.t[k];
            if t.is_punct('(') || t.is_punct('[') {
                depth += 1;
            } else if t.is_punct(')') || t.is_punct(']') {
                depth -= 1;
            } else if t.is_punct(',') && depth == 0 {
                out.push((s, k));
                s = k + 1;
            }
        }
        if s < b {
            out.push((s, b));
        }
        out
    }
    /// `(a, b, "C")` → identifiers (or expression text for complex entries).
    fn ident_list(&mut self) -> Vec<String> {
        match self.paren_group() {
            Some((a, b)) => self
                .split_commas(a, b)
                .into_iter()
                .map(|(x, y)| {
                    if y == x + 1 && self.t[x].is_ident() {
                        self.t[x].text.clone()
                    } else {
                        self.text(x, y)
                    }
                })
                .collect(),
            None => Vec::new(),
        }
    }
    /// Advance until one of the given keywords at paren depth 0; returns start index.
    fn skip_until_kw(&mut self, kws: &[&str]) -> usize {
        let start = self.i;
        let mut depth = 0;
        while let Some(t) = self.peek() {
            if t.is_punct('(') || t.is_punct('[') {
                depth += 1;
            } else if t.is_punct(')') || t.is_punct(']') {
                depth -= 1;
            } else if depth == 0 && t.kind == Kind::Word && kws.contains(&t.text.as_str()) {
                break;
            }
            self.i += 1;
        }
        start
    }
}

struct State {
    schema: Schema,
    default_schema: String,
    table_idx: HashMap<String, usize>,
}

enum TConstraint {
    Pk(Vec<String>),
    Unique(Vec<String>),
    Fk(ForeignKey),
    Check(String),
    Other,
}

const COLUMN_CONSTRAINT_KWS: &[&str] =
    &["constraint", "not", "null", "default", "primary", "unique", "references", "check", "collate", "generated"];

impl State {
    fn qualify_parts(&self, parts: &[String]) -> (String, String) {
        match parts.len() {
            0 => (self.default_schema.clone(), String::new()),
            1 => (self.default_schema.clone(), parts[0].clone()),
            n => (parts[n - 2].clone(), parts[n - 1].clone()),
        }
    }
    fn qname(&self, c: &mut Cur) -> Option<(String, String)> {
        let parts = c.name_parts();
        if parts.is_empty() {
            None
        } else {
            Some(self.qualify_parts(&parts))
        }
    }
    fn warn(&mut self, msg: String) {
        if self.schema.warnings.len() < 200 {
            self.schema.warnings.push(msg);
        }
    }
    fn table_mut(&mut self, id: &str) -> Option<&mut Table> {
        let i = *self.table_idx.get(id)?;
        self.schema.tables.get_mut(i)
    }

    fn statement(&mut self, c: &mut Cur) {
        if c.eat_kw("create") {
            c.eat_kws(&["or", "replace"]);
            self.create(c);
        } else if c.eat_kw("alter") {
            if c.eat_kw("table") || c.eat_kws(&["foreign", "table"]) {
                self.alter_table(c);
            } else if c.eat_kw("sequence") {
                self.alter_sequence(c);
            } else if c.eat_kw("type") {
                self.alter_type(c);
            }
        } else if c.eat_kws(&["comment", "on"]) {
            self.comment(c);
        } else if c.eat_kw("set") {
            c.eat_kw("local");
            c.eat_kw("session");
            if c.eat_kw("search_path") {
                if !c.eat_kw("to") {
                    c.eat_punct('=');
                }
                let mut first = None;
                while let Some(t) = c.peek() {
                    if (t.is_ident() || t.kind == Kind::Str) && !t.text.is_empty() && t.text != "$user" {
                        first = Some(t.text.clone());
                        break;
                    }
                    c.i += 1;
                }
                if let Some(s) = first {
                    self.default_schema = s;
                }
            }
        } else if c.eat_kw("drop") {
            self.drop(c);
        }
    }

    fn create(&mut self, c: &mut Cur) {
        c.eat_kw("global");
        c.eat_kw("local");
        let temp = c.eat_kw("temp") || c.eat_kw("temporary");
        let unlogged = c.eat_kw("unlogged");
        if c.eat_kw("table") || c.eat_kws(&["foreign", "table"]) {
            if !temp {
                self.create_table(c, unlogged);
            }
        } else if c.is_kw("unique") || c.is_kw("index") {
            let unique = c.eat_kw("unique");
            c.eat_kw("index");
            self.create_index(c, unique);
        } else if c.eat_kw("view") || c.eat_kws(&["recursive", "view"]) {
            self.create_view(c, false);
        } else if c.eat_kws(&["materialized", "view"]) {
            self.create_view(c, true);
        } else if c.eat_kw("type") {
            self.create_type(c);
        } else if c.eat_kw("sequence") {
            c.eat_kws(&["if", "not", "exists"]);
            if let Some((s, n)) = self.qname(c) {
                self.schema.sequences.push(Sequence { schema: s, name: n, owned_by: None });
            }
        } else if c.eat_kw("function") || c.eat_kw("procedure") || c.eat_kw("aggregate") {
            self.create_function(c);
        } else if c.eat_kw("trigger") || c.eat_kws(&["constraint", "trigger"]) {
            self.create_trigger(c);
        } else if c.eat_kw("extension") {
            c.eat_kws(&["if", "not", "exists"]);
            if let Some(name) = c.ident() {
                c.eat_kw("with");
                let schema = if c.eat_kw("schema") { c.ident() } else { None };
                if !self.schema.extensions.iter().any(|e| e.name == name) {
                    self.schema.extensions.push(Extension { name, schema });
                }
            }
        } else if c.eat_kw("schema") {
            c.eat_kws(&["if", "not", "exists"]);
            if let Some(name) = c.ident() {
                if !self.schema.schemas.contains(&name) {
                    self.schema.schemas.push(name);
                }
            }
        }
    }

    fn create_table(&mut self, c: &mut Cur, unlogged: bool) {
        c.eat_kws(&["if", "not", "exists"]);
        let Some((schema, name)) = self.qname(c) else { return };
        let mut table = Table { schema, name, unlogged, ..Default::default() };
        let id = table.id();
        if c.eat_kws(&["partition", "of"]) {
            if let Some((ps, pn)) = self.qname(c) {
                table.partition_of = Some(qualify(&ps, &pn));
            }
        } else if c.eat_kw("of") {
            let _ = self.qname(c);
        }
        if let Some((a, b)) = c.paren_group() {
            for (x, y) in c.split_commas(a, b) {
                let mut e = c.sub(x, y);
                self.table_element(&mut e, &mut table);
            }
        }
        // trailing clauses
        while !c.done() {
            if c.eat_kw("inherits") {
                let parts = c.ident_list();
                table.inherits = parts.iter().map(|p| self.resolve_name(p)).collect();
            } else if c.eat_kws(&["partition", "by"]) {
                let start = c.skip_until_kw(&["with", "tablespace", "using"]);
                table.partition_by = Some(c.text(start, c.i));
            } else if c.eat_kw("as") {
                self.warn(format!("CREATE TABLE {id} AS ... is not supported; columns unknown"));
                break;
            } else {
                c.i += 1;
            }
        }
        if let Some(&i) = self.table_idx.get(&id) {
            self.warn(format!("table {id} defined twice; keeping the last definition"));
            self.schema.tables[i] = table;
        } else {
            self.table_idx.insert(id, self.schema.tables.len());
            self.schema.tables.push(table);
        }
    }

    fn resolve_name(&self, dotted: &str) -> String {
        let parts: Vec<String> = dotted.split('.').map(|s| s.trim_matches('"').to_string()).collect();
        let (s, n) = self.qualify_parts(&parts);
        qualify(&s, &n)
    }

    fn table_element(&mut self, e: &mut Cur, table: &mut Table) {
        let Some(first) = e.peek() else { return };
        let is_constraint = first.kind == Kind::Word
            && (matches!(first.text.as_str(), "constraint" | "primary" | "unique" | "foreign" | "check" | "like")
                || (first.text == "exclude" && e.peek_at(1).map_or(false, |t| t.is_word("using") || t.is_punct('('))));
        if is_constraint {
            if e.eat_kw("like") {
                return;
            }
            let name = if e.eat_kw("constraint") { e.ident() } else { None };
            let tc = self.table_constraint(e, name.clone());
            apply_constraint(table, tc, name);
            return;
        }
        if let Some(col) = self.column_def(e, table) {
            table.columns.push(col);
        }
    }

    fn table_constraint(&mut self, e: &mut Cur, name: Option<String>) -> TConstraint {
        if e.eat_kws(&["primary", "key"]) {
            TConstraint::Pk(e.ident_list())
        } else if e.eat_kw("unique") {
            if e.eat_kw("nulls") {
                e.eat_kw("not");
                e.eat_kw("distinct");
            }
            TConstraint::Unique(e.ident_list())
        } else if e.eat_kws(&["foreign", "key"]) {
            let columns = e.ident_list();
            if !e.eat_kw("references") {
                return TConstraint::Other;
            }
            let mut fk = self.references_tail(e);
            fk.columns = columns;
            fk.name = name;
            TConstraint::Fk(fk)
        } else if e.eat_kw("check") {
            match e.paren_group() {
                Some((a, b)) => TConstraint::Check(e.text(a, b)),
                None => TConstraint::Other,
            }
        } else {
            TConstraint::Other
        }
    }

    /// Parse after `REFERENCES`: `table [(cols)] [ON DELETE ..] ...`
    fn references_tail(&mut self, e: &mut Cur) -> ForeignKey {
        let (s, n) = self.qname(e).unwrap_or_default();
        let mut fk = ForeignKey { ref_table: qualify(&s, &n), ..Default::default() };
        if e.is_punct('(') {
            fk.ref_columns = e.ident_list();
        }
        loop {
            if e.eat_kws(&["on", "delete"]) {
                fk.on_delete = Some(fk_action(e));
            } else if e.eat_kws(&["on", "update"]) {
                fk.on_update = Some(fk_action(e));
            } else if e.eat_kw("match") {
                e.ident();
            } else if e.eat_kws(&["not", "deferrable"]) {
            } else if e.eat_kw("deferrable") {
                fk.deferrable = true;
            } else if e.eat_kw("initially") {
                e.ident();
            } else if e.eat_kws(&["not", "valid"]) {
            } else {
                break;
            }
        }
        fk
    }

    fn column_def(&mut self, e: &mut Cur, table: &mut Table) -> Option<Column> {
        let name = e.ident()?;
        let tstart = e.skip_until_kw(COLUMN_CONSTRAINT_KWS);
        let data_type = e.text(tstart, e.i);
        let mut col = Column { name: name.clone(), data_type, nullable: true, ..Default::default() };
        let mut cname: Option<String> = None;
        while !e.done() {
            if e.eat_kw("constraint") {
                cname = e.ident();
            } else if e.eat_kws(&["not", "null"]) {
                col.nullable = false;
            } else if e.eat_kw("null") {
                col.nullable = true;
            } else if e.eat_kw("default") {
                let start = e.i;
                e.i += 1; // always take the first token
                if e.t.get(start).map_or(false, |t| t.is_punct('(')) {
                    e.i = e.matching_paren(start) + 1;
                }
                let mut depth = 0;
                while let Some(t) = e.peek() {
                    if t.is_punct('(') {
                        depth += 1;
                    } else if t.is_punct(')') {
                        depth -= 1;
                    } else if depth == 0 && t.kind == Kind::Word {
                        let w = t.text.as_str();
                        let stop = match w {
                            "not" => e.peek_at(1).map_or(false, |n| n.is_word("null")),
                            "null" => {
                                // `DEFAULT x NULL` (nullable marker) – but not `IS NULL`
                                !e.t.get(e.i - 1).map_or(false, |p| p.is_word("is") || p.is_word("not"))
                            }
                            _ => COLUMN_CONSTRAINT_KWS.contains(&w) && w != "not" && w != "null",
                        };
                        if stop {
                            break;
                        }
                    }
                    e.i += 1;
                }
                col.default = Some(e.text(start, e.i));
            } else if e.eat_kws(&["primary", "key"]) {
                table.primary_key = Some(PrimaryKey { name: cname.take(), columns: vec![name.clone()] });
                col.nullable = false;
            } else if e.eat_kw("unique") {
                table.uniques.push(UniqueConstraint { name: cname.take(), columns: vec![name.clone()] });
            } else if e.eat_kw("references") {
                let mut fk = self.references_tail(e);
                fk.columns = vec![name.clone()];
                fk.name = cname.take();
                table.foreign_keys.push(fk);
            } else if e.eat_kw("check") {
                if let Some((a, b)) = e.paren_group() {
                    table.checks.push(CheckConstraint { name: cname.take(), expression: e.text(a, b) });
                }
                e.eat_kws(&["no", "inherit"]);
            } else if e.eat_kw("collate") {
                col.collation = e.name_parts().last().cloned();
            } else if e.eat_kw("generated") {
                if e.eat_kw("always") {
                    if e.eat_kw("as") {
                        if e.eat_kw("identity") {
                            col.identity = Some("ALWAYS".into());
                            e.paren_group();
                        } else if let Some((a, b)) = e.paren_group() {
                            col.generated = Some(e.text(a, b));
                            e.eat_kw("stored");
                            e.eat_kw("virtual");
                        }
                    }
                } else if e.eat_kws(&["by", "default", "as", "identity"]) {
                    col.identity = Some("BY DEFAULT".into());
                    e.paren_group();
                }
            } else {
                e.i += 1;
            }
        }
        Some(col)
    }

    fn create_index(&mut self, c: &mut Cur, unique: bool) {
        c.eat_kw("concurrently");
        c.eat_kws(&["if", "not", "exists"]);
        let name = if c.is_kw("on") { String::new() } else { c.ident().unwrap_or_default() };
        if !c.eat_kw("on") {
            return;
        }
        c.eat_kw("only");
        let Some((s, n)) = self.qname(c) else { return };
        let method = if c.eat_kw("using") { c.ident().unwrap_or_else(|| "btree".into()) } else { "btree".into() };
        let columns = match c.paren_group() {
            Some((a, b)) => c
                .split_commas(a, b)
                .into_iter()
                .map(|(x, y)| {
                    // strip opclass / ordering noise for single identifiers
                    if c.t[x].is_ident() && (y == x + 1 || !c.t.get(x + 1).map_or(false, |t| t.is_punct('(') || t.is_punct('.') || t.kind == Kind::Op)) {
                        c.t[x].text.clone()
                    } else {
                        c.text(x, y)
                    }
                })
                .collect(),
            None => Vec::new(),
        };
        let mut include = Vec::new();
        let mut predicate = None;
        while !c.done() {
            if c.eat_kw("include") {
                include = c.ident_list();
            } else if c.eat_kw("where") {
                predicate = Some(c.rest_text());
                break;
            } else {
                c.i += 1;
            }
        }
        let id = qualify(&s, &n);
        let idx = Index {
            name,
            unique,
            method,
            columns,
            include,
            predicate,
            definition: c.text(0, c.t.len()),
        };
        if let Some(t) = self.table_mut(&id) {
            t.indexes.push(idx);
        } else if let Some(v) = self.schema.views.iter_mut().find(|v| v.id() == id) {
            v.indexes.push(idx);
        } else {
            self.warn(format!("index {} on unknown relation {id}", idx.name));
        }
    }

    fn create_view(&mut self, c: &mut Cur, materialized: bool) {
        c.eat_kws(&["if", "not", "exists"]);
        let Some((s, n)) = self.qname(c) else { return };
        c.paren_group();
        while !c.done() && !c.is_kw("as") {
            c.i += 1;
        }
        if !c.eat_kw("as") {
            return;
        }
        let start = c.i;
        let mut end = c.t.len();
        // trailing `WITH [NO] DATA` / `WITH ... CHECK OPTION`
        if end >= 2 && c.t[end - 1].is_word("data") {
            end -= if c.t[end - 2].is_word("no") { 3 } else { 2 };
        } else if end >= 3 && c.t[end - 1].is_word("option") && c.t[end - 2].is_word("check") {
            end -= 3;
            if end > 0 && (c.t[end - 1].is_word("local") || c.t[end - 1].is_word("cascaded")) {
                end -= 1;
            }
            end = end.saturating_sub(1);
        }
        let mut raw_refs = Vec::new();
        let mut k = start;
        while k < end {
            let t = &c.t[k];
            if t.is_ident() {
                if k + 2 < end && c.t[k + 1].is_punct('.') && c.t[k + 2].is_ident() {
                    raw_refs.push(format!("{}.{}", t.text, c.t[k + 2].text));
                    k += 3;
                    continue;
                }
                if k > start && (c.t[k - 1].is_word("from") || c.t[k - 1].is_word("join")) {
                    raw_refs.push(t.text.clone());
                }
            }
            k += 1;
        }
        let definition = c.raw_text(start, end);
        let view = View { schema: s, name: n, materialized, definition, raw_refs, ..Default::default() };
        let id = view.id();
        self.schema.views.retain(|v| v.id() != id);
        self.schema.views.push(view);
    }

    fn create_type(&mut self, c: &mut Cur) {
        let Some((s, n)) = self.qname(c) else { return };
        if c.eat_kws(&["as", "enum"]) {
            let values = match c.paren_group() {
                Some((a, b)) => c.t[a..b].iter().filter(|t| t.kind == Kind::Str).map(|t| t.text.clone()).collect(),
                None => Vec::new(),
            };
            self.schema.enums.push(EnumType { schema: s, name: n, values });
        }
    }

    fn alter_type(&mut self, c: &mut Cur) {
        let Some((s, n)) = self.qname(c) else { return };
        let id = qualify(&s, &n);
        if c.eat_kws(&["add", "value"]) {
            c.eat_kws(&["if", "not", "exists"]);
            let Some(v) = c.peek().filter(|t| t.kind == Kind::Str).map(|t| t.text.clone()) else { return };
            c.i += 1;
            let before = c.eat_kw("before");
            let after = !before && c.eat_kw("after");
            let anchor = c.peek().filter(|t| t.kind == Kind::Str).map(|t| t.text.clone());
            if let Some(en) = self.schema.enums.iter_mut().find(|e| e.id() == id) {
                if en.values.contains(&v) {
                    return;
                }
                let pos = anchor.and_then(|a| en.values.iter().position(|x| *x == a));
                match (pos, before, after) {
                    (Some(p), true, _) => en.values.insert(p, v),
                    (Some(p), _, true) => en.values.insert(p + 1, v),
                    _ => en.values.push(v),
                }
            }
        }
    }

    fn create_function(&mut self, c: &mut Cur) {
        let Some((s, n)) = self.qname(c) else { return };
        let arguments = match c.paren_group() {
            Some((a, b)) => c.text(a, b),
            None => String::new(),
        };
        let mut returns = None;
        let mut language = None;
        while !c.done() {
            if c.eat_kw("returns") {
                let start = c.skip_until_kw(&[
                    "language", "as", "immutable", "stable", "volatile", "strict", "security", "parallel", "cost", "rows",
                    "set", "begin", "return", "leakproof", "called", "window", "transform", "support",
                ]);
                returns = Some(c.text(start, c.i));
            } else if c.eat_kw("language") {
                language = c.ident().or_else(|| c.peek().map(|t| t.text.clone()));
            } else {
                c.i += 1;
            }
        }
        let definition = c.raw_text(0, c.t.len());
        let f = Function { schema: s, name: n, arguments, returns, language, definition };
        let id = f.id();
        self.schema.functions.retain(|x| x.id() != id);
        self.schema.functions.push(f);
    }

    fn create_trigger(&mut self, c: &mut Cur) {
        let Some(name) = c.ident() else { return };
        while !c.done() && !c.is_kw("on") {
            c.i += 1;
        }
        if !c.eat_kw("on") {
            return;
        }
        let Some((s, n)) = self.qname(c) else { return };
        let definition = c.text(0, c.t.len());
        let trig = Trigger { name, table: qualify(&s, &n), definition };
        let id = trig.id();
        self.schema.triggers.retain(|t| t.id() != id);
        self.schema.triggers.push(trig);
    }

    fn alter_sequence(&mut self, c: &mut Cur) {
        c.eat_kws(&["if", "exists"]);
        let Some((s, n)) = self.qname(c) else { return };
        while !c.done() {
            if c.eat_kws(&["owned", "by"]) {
                let parts = c.name_parts();
                if parts.len() >= 2 {
                    let col = parts[parts.len() - 1].clone();
                    let (ts, tn) = self.qualify_parts(&parts[..parts.len() - 1]);
                    let owner = format!("{}.{}", qualify(&ts, &tn), col);
                    if let Some(seq) = self.schema.sequences.iter_mut().find(|q| q.schema == s && q.name == n) {
                        seq.owned_by = Some(owner);
                    }
                }
            } else {
                c.i += 1;
            }
        }
    }

    fn alter_table(&mut self, c: &mut Cur) {
        c.eat_kws(&["if", "exists"]);
        c.eat_kw("only");
        let Some((s, n)) = self.qname(c) else { return };
        if c.is_punct('*') || c.peek().map_or(false, |t| t.text == "*") {
            c.i += 1;
        }
        let id = qualify(&s, &n);
        if !self.table_idx.contains_key(&id) {
            // e.g. ALTER TABLE on a view or a table we could not parse
            if !c.is_kws(&["owner", "to"]) && !self.schema.views.iter().any(|v| v.id() == id) {
                if c.is_kw("add") || c.is_kw("alter") {
                    self.warn(format!("ALTER TABLE on unknown table {id}"));
                }
            }
            return;
        }
        let a = c.i;
        for (x, y) in c.split_commas(a, c.t.len()) {
            let mut e = c.sub(x, y);
            self.alter_action(&mut e, &id);
        }
    }

    fn alter_action(&mut self, e: &mut Cur, id: &str) {
        if e.eat_kw("add") {
            if e.is_kw("constraint") || e.is_kw("primary") || e.is_kw("unique") || e.is_kw("foreign") || e.is_kw("check") || e.is_kw("exclude") {
                let name = if e.eat_kw("constraint") { e.ident() } else { None };
                let tc = self.table_constraint(e, name.clone());
                if let Some(t) = self.table_mut(id) {
                    apply_constraint(t, tc, name);
                }
            } else {
                e.eat_kw("column");
                e.eat_kws(&["if", "not", "exists"]);
                let mut tmp = std::mem::take(self.table_mut(id).unwrap());
                if let Some(col) = self.column_def(e, &mut tmp) {
                    tmp.columns.retain(|c| c.name != col.name);
                    tmp.columns.push(col);
                }
                *self.table_mut(id).unwrap() = tmp;
            }
        } else if e.eat_kw("alter") {
            e.eat_kw("column");
            let Some(col) = e.ident() else { return };
            let mut new_default: Option<Option<String>> = None;
            let mut nullable = None;
            let mut new_type = None;
            let mut identity = None;
            if e.eat_kws(&["set", "default"]) {
                new_default = Some(Some(e.rest_text()));
            } else if e.eat_kws(&["drop", "default"]) {
                new_default = Some(None);
            } else if e.eat_kws(&["set", "not", "null"]) {
                nullable = Some(false);
            } else if e.eat_kws(&["drop", "not", "null"]) {
                nullable = Some(true);
            } else if e.eat_kws(&["set", "data", "type"]) || e.eat_kw("type") {
                let start = e.skip_until_kw(&["using", "collate"]);
                new_type = Some(e.text(start, e.i));
            } else if e.eat_kws(&["add", "generated"]) {
                identity = Some(if e.eat_kw("always") { "ALWAYS" } else { "BY DEFAULT" }.to_string());
            }
            if let Some(c) = self.table_mut(id).and_then(|t| t.column_mut(&col)) {
                if let Some(d) = new_default {
                    c.default = d;
                }
                if let Some(nl) = nullable {
                    c.nullable = nl;
                }
                if let Some(t) = new_type {
                    c.data_type = t;
                }
                if identity.is_some() {
                    c.identity = identity;
                }
            }
        } else if e.eat_kw("drop") {
            if e.eat_kw("constraint") {
                e.eat_kws(&["if", "exists"]);
                if let Some(name) = e.ident() {
                    if let Some(t) = self.table_mut(id) {
                        let some = Some(name.clone());
                        if t.primary_key.as_ref().map_or(false, |p| p.name == some) {
                            t.primary_key = None;
                        }
                        t.foreign_keys.retain(|f| f.name != some);
                        t.uniques.retain(|u| u.name != some);
                        t.checks.retain(|u| u.name != some);
                    }
                }
            } else {
                e.eat_kw("column");
                e.eat_kws(&["if", "exists"]);
                if let Some(col) = e.ident() {
                    if let Some(t) = self.table_mut(id) {
                        t.columns.retain(|c| c.name != col);
                        t.foreign_keys.retain(|f| !f.columns.contains(&col));
                        t.indexes.retain(|i| !i.columns.contains(&col));
                    }
                }
            }
        } else if e.eat_kws(&["attach", "partition"]) {
            if let Some((cs, cn)) = self.qname(e) {
                let child = qualify(&cs, &cn);
                let parent = id.to_string();
                if let Some(t) = self.table_mut(&child) {
                    t.partition_of = Some(parent);
                }
            }
        } else if e.eat_kw("rename") {
            if e.eat_kw("column") || (!e.is_kw("to") && !e.is_kw("constraint")) {
                let Some(from) = e.ident() else { return };
                e.eat_kw("to");
                let Some(to) = e.ident() else { return };
                if let Some(t) = self.table_mut(id) {
                    if let Some(c) = t.column_mut(&from) {
                        c.name = to.clone();
                    }
                    for f in &mut t.foreign_keys {
                        for c in &mut f.columns {
                            if *c == from {
                                *c = to.clone();
                            }
                        }
                    }
                }
            } else if e.eat_kw("to") {
                if let Some(new_name) = e.ident() {
                    self.rename_table(id, &new_name);
                }
            }
        }
    }

    fn rename_table(&mut self, id: &str, new_name: &str) {
        let Some(i) = self.table_idx.remove(id) else { return };
        let t = &mut self.schema.tables[i];
        t.name = new_name.to_string();
        let new_id = t.id();
        self.table_idx.insert(new_id.clone(), i);
        for t in &mut self.schema.tables {
            for f in &mut t.foreign_keys {
                if f.ref_table == id {
                    f.ref_table = new_id.clone();
                }
            }
        }
    }

    fn drop(&mut self, c: &mut Cur) {
        let kind = if c.eat_kw("table") {
            "table"
        } else if c.eat_kw("view") || c.eat_kws(&["materialized", "view"]) {
            "view"
        } else if c.eat_kw("index") {
            "index"
        } else {
            return;
        };
        c.eat_kw("concurrently");
        c.eat_kws(&["if", "exists"]);
        loop {
            let parts = c.name_parts();
            if parts.is_empty() {
                break;
            }
            let (s, n) = self.qualify_parts(&parts);
            let id = qualify(&s, &n);
            match kind {
                "table" => {
                    if let Some(i) = self.table_idx.remove(&id) {
                        self.schema.tables.remove(i);
                        self.table_idx = self.schema.tables.iter().enumerate().map(|(i, t)| (t.id(), i)).collect();
                    }
                }
                "view" => self.schema.views.retain(|v| v.id() != id),
                _ => {
                    for t in &mut self.schema.tables {
                        t.indexes.retain(|i| i.name != n);
                    }
                }
            }
            if !c.eat_punct(',') {
                break;
            }
        }
    }

    fn comment(&mut self, c: &mut Cur) {
        let kind = if c.eat_kw("table") {
            "table"
        } else if c.eat_kw("column") {
            "column"
        } else if c.eat_kw("view") || c.eat_kws(&["materialized", "view"]) {
            "view"
        } else {
            return;
        };
        let parts = c.name_parts();
        if !c.eat_kw("is") {
            return;
        }
        let text = c.peek().filter(|t| t.kind == Kind::Str).map(|t| t.text.clone());
        match kind {
            "table" => {
                let (s, n) = self.qualify_parts(&parts);
                if let Some(t) = self.table_mut(&qualify(&s, &n)) {
                    t.comment = text;
                }
            }
            "view" => {
                let (s, n) = self.qualify_parts(&parts);
                let id = qualify(&s, &n);
                if let Some(v) = self.schema.views.iter_mut().find(|v| v.id() == id) {
                    v.comment = text;
                }
            }
            _ => {
                if parts.len() < 2 {
                    return;
                }
                let col = parts[parts.len() - 1].clone();
                let (s, n) = self.qualify_parts(&parts[..parts.len() - 1]);
                if let Some(cm) = self.table_mut(&qualify(&s, &n)).and_then(|t| t.column_mut(&col)) {
                    cm.comment = text;
                }
            }
        }
    }

    /// Post-processing: resolve loosely qualified references.
    fn resolve(&mut self) {
        let ids: Vec<String> = self.schema.tables.iter().map(|t| t.id()).collect();
        let view_ids: Vec<String> = self.schema.views.iter().map(|v| v.id()).collect();
        let by_name = |name: &str, pool: &[String]| -> Option<String> {
            let mut hits = pool.iter().filter(|id| split_id(id).1 == name);
            let first = hits.next()?;
            if hits.next().is_none() {
                Some(first.clone())
            } else {
                None
            }
        };
        let mut all: Vec<String> = ids.clone();
        all.extend(view_ids.iter().cloned());
        for t in &mut self.schema.tables {
            let tid = t.id();
            let pk = t.primary_key.clone();
            for f in &mut t.foreign_keys {
                if !ids.contains(&f.ref_table) {
                    if let Some(found) = by_name(split_id(&f.ref_table).1, &ids) {
                        f.ref_table = found;
                    }
                }
                if f.ref_columns.is_empty() {
                    // implicit reference to the primary key
                    if f.ref_table == tid {
                        if let Some(pk) = &pk {
                            f.ref_columns = pk.columns.clone();
                        }
                    }
                }
            }
        }
        // implicit PK references to other tables
        let pks: HashMap<String, Vec<String>> = self
            .schema
            .tables
            .iter()
            .filter_map(|t| t.primary_key.as_ref().map(|p| (t.id(), p.columns.clone())))
            .collect();
        for t in &mut self.schema.tables {
            for f in &mut t.foreign_keys {
                if f.ref_columns.is_empty() {
                    if let Some(cols) = pks.get(&f.ref_table) {
                        f.ref_columns = cols.clone();
                    }
                }
            }
        }
        for v in &mut self.schema.views {
            let vid = v.id();
            let mut deps: Vec<String> = Vec::new();
            for r in &v.raw_refs {
                let cand = if r.contains('.') {
                    if all.contains(r) {
                        Some(r.clone())
                    } else {
                        None
                    }
                } else {
                    let q = qualify(&self.default_schema, r);
                    if all.contains(&q) {
                        Some(q)
                    } else {
                        by_name(r, &all)
                    }
                };
                if let Some(c) = cand {
                    if c != vid && !deps.contains(&c) {
                        deps.push(c);
                    }
                }
            }
            v.depends_on = deps;
        }
        let mut schemas: Vec<String> = self.schema.schemas.clone();
        for t in &self.schema.tables {
            if !schemas.contains(&t.schema) {
                schemas.push(t.schema.clone());
            }
        }
        self.schema.schemas = schemas;
    }
}

fn fk_action(e: &mut Cur) -> String {
    let mut words = Vec::new();
    if e.eat_kw("no") {
        e.eat_kw("action");
        return "NO ACTION".into();
    }
    if e.eat_kw("set") {
        words.push("SET");
        if e.eat_kw("null") {
            words.push("NULL");
        } else if e.eat_kw("default") {
            words.push("DEFAULT");
        }
        e.paren_group();
        return words.join(" ");
    }
    e.ident().map(|w| w.to_uppercase()).unwrap_or_default()
}

fn apply_constraint(table: &mut Table, tc: TConstraint, name: Option<String>) {
    match tc {
        TConstraint::Pk(cols) => {
            for c in &cols {
                if let Some(col) = table.column_mut(c) {
                    col.nullable = false;
                }
            }
            table.primary_key = Some(PrimaryKey { name, columns: cols });
        }
        TConstraint::Unique(cols) => table.uniques.push(UniqueConstraint { name, columns: cols }),
        TConstraint::Fk(fk) => {
            table.foreign_keys.retain(|f| f.name.is_none() || f.name != fk.name);
            table.foreign_keys.push(fk)
        }
        TConstraint::Check(expr) => table.checks.push(CheckConstraint { name, expression: expr }),
        TConstraint::Other => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"
SET statement_timeout = 0;
SELECT pg_catalog.set_config('search_path', '', false);
\restrict abc123
CREATE SCHEMA billing;
CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;
CREATE TYPE public.status AS ENUM (
    'draft',
    'sent'
);
CREATE FUNCTION public.touch() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at = now(); RETURN NEW;
END;
$$;
CREATE TABLE public.users (
    id bigint NOT NULL,
    email character varying DEFAULT ''::character varying NOT NULL,
    "Name" text,
    status public.status DEFAULT 'draft'::public.status,
    created_at timestamp(6) without time zone NOT NULL,
    CONSTRAINT email_present CHECK ((email <> ''::text))
);
COMMENT ON TABLE public.users IS 'People';
COMMENT ON COLUMN public.users.email IS 'Login';
CREATE SEQUENCE public.users_id_seq START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.users_id_seq OWNED BY public.users.id;
CREATE TABLE billing.invoices (
    id bigint NOT NULL,
    user_id bigint,
    total numeric(12,2) DEFAULT 0.0 NOT NULL
)
PARTITION BY RANGE (id);
CREATE TABLE billing.invoices_2024 (
    id bigint NOT NULL,
    user_id bigint,
    total numeric(12,2) DEFAULT 0.0 NOT NULL
);
ALTER TABLE ONLY billing.invoices ATTACH PARTITION billing.invoices_2024 FOR VALUES FROM (0) TO (100);
CREATE VIEW public.active_users AS
 SELECT users.id FROM public.users WHERE (users.status = 'sent'::public.status);
ALTER TABLE ONLY public.users ALTER COLUMN id SET DEFAULT nextval('public.users_id_seq'::regclass);
ALTER TABLE ONLY public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);
ALTER TABLE ONLY billing.invoices ADD CONSTRAINT invoices_pkey PRIMARY KEY (id);
CREATE UNIQUE INDEX index_users_on_email ON public.users USING btree (lower((email)::text)) WHERE (email IS NOT NULL);
CREATE INDEX index_invoices_on_user_id ON ONLY billing.invoices USING btree (user_id);
CREATE TRIGGER touch_users BEFORE UPDATE ON public.users FOR EACH ROW EXECUTE FUNCTION public.touch();
ALTER TABLE ONLY billing.invoices
    ADD CONSTRAINT fk_rails_abc FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;
INSERT INTO "schema_migrations" (version) VALUES ('1'), ('2');
"#;

    #[test]
    fn parses_pg_dump() {
        let s = parse(DUMP);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        assert_eq!(s.tables.len(), 3);
        let u = s.table("public.users").unwrap();
        assert_eq!(u.columns.len(), 5);
        assert_eq!(u.columns[1].data_type, "character varying");
        assert_eq!(u.columns[1].default.as_deref(), Some("''::character varying"));
        assert!(!u.columns[1].nullable);
        assert_eq!(u.columns[2].name, "Name");
        assert_eq!(u.columns[4].data_type, "timestamp(6) without time zone");
        assert_eq!(u.columns[0].default.as_deref(), Some("nextval('public.users_id_seq'::regclass)"));
        assert_eq!(u.primary_key.as_ref().unwrap().columns, vec!["id"]);
        assert_eq!(u.comment.as_deref(), Some("People"));
        assert_eq!(u.columns[1].comment.as_deref(), Some("Login"));
        assert_eq!(u.checks.len(), 1);
        assert_eq!(u.indexes.len(), 1);
        assert!(u.indexes[0].unique);
        assert_eq!(u.indexes[0].predicate.as_deref(), Some("(email IS NOT NULL)"));
        let inv = s.table("billing.invoices").unwrap();
        assert_eq!(inv.partition_by.as_deref(), Some("RANGE (id)"));
        assert_eq!(inv.foreign_keys[0].ref_table, "public.users");
        assert_eq!(inv.foreign_keys[0].on_delete.as_deref(), Some("CASCADE"));
        assert_eq!(inv.indexes[0].columns, vec!["user_id"]);
        assert_eq!(s.table("billing.invoices_2024").unwrap().partition_of.as_deref(), Some("billing.invoices"));
        assert_eq!(s.enums[0].values, vec!["draft", "sent"]);
        assert_eq!(s.views[0].depends_on, vec!["public.users"]);
        assert_eq!(s.functions.len(), 1);
        assert_eq!(s.triggers[0].table, "public.users");
        assert_eq!(s.sequences[0].owned_by.as_deref(), Some("public.users.id"));
        assert!(s.schemas.contains(&"billing".to_string()));
    }

    #[test]
    fn parses_handwritten_ddl() {
        let s = parse(
            r#"
            create table accounts (id serial primary key, name text not null unique);
            create table if not exists posts (
              id uuid primary key default gen_random_uuid(),
              account_id int not null references accounts on delete cascade,
              parent_id uuid references posts(id),
              title varchar(200) default null,
              score double precision generated always as (1.0 * 2) stored,
              foreign key (account_id, parent_id) references accounts (id, x)
            );
            alter table posts add column body text;
            alter table posts rename column body to content;
            "#,
        );
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let p = s.table("public.posts").unwrap();
        assert_eq!(p.primary_key.as_ref().unwrap().columns, vec!["id"]);
        assert_eq!(p.foreign_keys.len(), 3);
        assert_eq!(p.foreign_keys[0].ref_table, "public.accounts");
        assert_eq!(p.foreign_keys[0].ref_columns, vec!["id"]);
        assert_eq!(p.foreign_keys[1].ref_table, "public.posts");
        assert_eq!(p.column("title").unwrap().default.as_deref(), Some("null"));
        assert!(p.column("title").unwrap().nullable);
        assert_eq!(p.column("score").unwrap().generated.as_deref(), Some("1.0 * 2"));
        assert_eq!(p.column("score").unwrap().data_type, "double precision");
        assert!(p.column("content").is_some());
        let a = s.table("public.accounts").unwrap();
        assert_eq!(a.uniques[0].columns, vec!["name"]);
    }

    #[test]
    fn keeps_begin_atomic_bodies_together() {
        let s = parse("CREATE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT CASE WHEN true THEN 1 END; END; CREATE TABLE t (id int);");
        assert_eq!(s.functions.len(), 1);
        assert_eq!(s.tables.len(), 1);
    }
}
