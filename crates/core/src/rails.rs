//! Rails `db/schema.rb` support.
//!
//! The Ruby schema DSL is translated into the equivalent Postgres DDL (the
//! shape `pg_dump` would produce for the same database), which then goes
//! through the regular SQL parser. That keeps one model, one set of
//! resolution rules and identical diffs regardless of the dump format.
//!
//! Like the SQL parser this never fails: unknown calls are ignored and
//! malformed ones produce warnings.
use std::collections::HashMap;

/// Heuristic: does this source look like a Rails `schema.rb`?
pub fn is_schema_rb(src: &str) -> bool {
    src.contains("ActiveRecord::Schema")
        || src.lines().any(|l| {
            let l = l.trim_start();
            l.starts_with("create_table \"") || l.starts_with("create_table :") || l.starts_with("create_table(")
        })
}

/// Translate `schema.rb` into Postgres DDL. Returns the SQL and any warnings.
pub fn to_sql(src: &str) -> (String, Vec<String>) {
    let tokens = lex(src);
    let mut p = Parser { t: &tokens, i: 0 };
    let stmts = p.block(false);
    let mut tr = Translator::default();
    tr.stmts(&stmts);
    tr.flush_foreign_keys();
    (tr.out, tr.warnings)
}

// ---------------------------------------------------------------------------
// Lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Const(String),
    Str(String),
    Sym(String),
    /// `name:` or `"name":` hash label.
    Label(String),
    Num(String),
    /// `%w[...]` / `%i[...]` word arrays.
    Words(Vec<String>),
    Punct(&'static str),
    Newline,
}

const PUNCTS: &[&str] = &["=>", "->", "::", "(", ")", "[", "]", "{", "}", ",", ".", "|", "=", "-", "+", "*", "&", "!", "?", ":", "<", ">", ";"];

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}
fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

fn lex(src: &str) -> Vec<Tok> {
    let b = src.as_bytes();
    let n = b.len();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    // heredocs whose bodies start on the next line: (token index, id, squiggly, raw)
    let mut pending: Vec<(usize, String, bool)> = Vec::new();
    let mut line_start = true;
    while i < n {
        let c = b[i];
        if c == b'\n' {
            out.push(Tok::Newline);
            i += 1;
            for (idx, id, squiggly) in std::mem::take(&mut pending) {
                let (body, next) = heredoc_body(src, i, &id, squiggly);
                out[idx] = Tok::Str(body);
                i = next;
            }
            line_start = true;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if line_start && src[i..].starts_with("=begin") {
            match src[i..].find("\n=end") {
                Some(k) => {
                    i += k + 5;
                    while i < n && b[i] != b'\n' {
                        i += 1;
                    }
                }
                None => i = n,
            }
            continue;
        }
        line_start = false;
        if c == b'#' {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'\\' && i + 1 < n && b[i + 1] == b'\n' {
            i += 2; // explicit line continuation
            continue;
        }
        // heredoc: <<~ID, <<-ID, <<ID, optionally quoted
        if c == b'<' && src[i..].starts_with("<<") {
            let mut j = i + 2;
            let squiggly = j < n && b[j] == b'~';
            if j < n && (b[j] == b'~' || b[j] == b'-') {
                j += 1;
            }
            let quote = if j < n && (b[j] == b'\'' || b[j] == b'"') { Some(b[j]) } else { None };
            if quote.is_some() {
                j += 1;
            }
            let s = j;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j > s && b[s].is_ascii_uppercase() {
                let id = src[s..j].to_string();
                if quote.is_some() && j < n && Some(b[j]) == quote {
                    j += 1;
                }
                pending.push((out.len(), id, squiggly));
                out.push(Tok::Str(String::new()));
                i = j;
                continue;
            }
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            let (s, next) = read_str(src, i);
            i = next;
            // `"name": value` label
            if c != b'`' && i < n && b[i] == b':' && !(i + 1 < n && b[i + 1] == b':') {
                i += 1;
                out.push(Tok::Label(s));
            } else {
                out.push(Tok::Str(s));
            }
            continue;
        }
        if c == b'%' && i + 2 < n && matches!(b[i + 1], b'w' | b'W' | b'i' | b'I') && matches!(b[i + 2], b'[' | b'(' | b'{' | b'<') {
            let close = match b[i + 2] {
                b'[' => b']',
                b'(' => b')',
                b'{' => b'}',
                _ => b'>',
            };
            let s = i + 3;
            let mut j = s;
            while j < n && b[j] != close {
                j += 1;
            }
            out.push(Tok::Words(src[s..j].split_whitespace().map(str::to_string).collect()));
            i = (j + 1).min(n);
            continue;
        }
        // `:sym` — but not the `:` of a ternary or `Foo::Bar` (handled above)
        let after_space = i == 0 || b[i - 1].is_ascii_whitespace() || b"([{,|>".contains(&b[i - 1]);
        if c == b':' && i + 1 < n && b[i + 1] != b':' && after_space {
            if b[i + 1] == b'"' || b[i + 1] == b'\'' {
                let (s, next) = read_str(src, i + 1);
                out.push(Tok::Sym(s));
                i = next;
                continue;
            }
            if is_ident_start(b[i + 1]) {
                let mut j = i + 1;
                while j < n && is_ident_char(b[j]) {
                    j += 1;
                }
                if j < n && matches!(b[j], b'?' | b'!' | b'=') {
                    j += 1;
                }
                out.push(Tok::Sym(src[i + 1..j].to_string()));
                i = j;
                continue;
            }
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < n && (b[j].is_ascii_digit() || b[j] == b'_' || (b[j] == b'.' && j + 1 < n && b[j + 1].is_ascii_digit())) {
                j += 1;
            }
            out.push(Tok::Num(src[i..j].replace('_', "")));
            i = j;
            continue;
        }
        if is_ident_start(c) {
            let mut j = i;
            while j < n && is_ident_char(b[j]) {
                j += 1;
            }
            if j < n && (b[j] == b'?' || b[j] == b'!') && !(j + 1 < n && b[j + 1] == b'=') {
                j += 1;
            }
            let word = &src[i..j];
            // `name: value` label (but not `Foo::Bar`)
            if j < n && b[j] == b':' && !(j + 1 < n && b[j + 1] == b':') {
                out.push(Tok::Label(word.to_string()));
                i = j + 1;
                continue;
            }
            out.push(if c.is_ascii_uppercase() { Tok::Const(word.to_string()) } else { Tok::Ident(word.to_string()) });
            i = j;
            continue;
        }
        if let Some(p) = PUNCTS.iter().find(|p| src[i..].starts_with(**p)) {
            out.push(Tok::Punct(p));
            i += p.len();
            continue;
        }
        i += src[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// Read a quoted string starting at `open`; returns (unescaped, index after).
fn read_str(src: &str, open: usize) -> (String, usize) {
    let b = src.as_bytes();
    let q = b[open];
    let mut s = String::new();
    let mut i = open + 1;
    while i < b.len() {
        let c = b[i];
        if c == q {
            return (s, i + 1);
        }
        if c == b'\\' && i + 1 < b.len() {
            let e = b[i + 1];
            if q == b'"' {
                match e {
                    b'n' => s.push('\n'),
                    b't' => s.push('\t'),
                    b'r' => s.push('\r'),
                    b'0' => s.push('\0'),
                    b'e' => s.push('\x1b'),
                    b's' => s.push(' '),
                    _ => s.push(e as char),
                }
                i += 2;
                continue;
            }
            if e == q || e == b'\\' {
                s.push(e as char);
                i += 2;
                continue;
            }
        }
        let ch = src[i..].chars().next().unwrap();
        s.push(ch);
        i += ch.len_utf8();
    }
    (s, b.len())
}

/// Read a heredoc body starting at `start` (beginning of the next line).
fn heredoc_body(src: &str, start: usize, id: &str, squiggly: bool) -> (String, usize) {
    let mut lines = Vec::new();
    let mut i = start;
    let mut end = src.len();
    while i < src.len() {
        let e = src[i..].find('\n').map_or(src.len(), |k| i + k);
        let line = &src[i..e];
        if line.trim() == id {
            end = (e + 1).min(src.len());
            break;
        }
        lines.push(line);
        i = (e + 1).min(src.len());
        end = i;
    }
    let indent = if squiggly {
        lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0)
    } else {
        0
    };
    let body: Vec<&str> = lines.iter().map(|l| if l.len() >= indent { &l[indent..] } else { l.trim_start() }).collect();
    (body.join("\n"), end)
}

// ---------------------------------------------------------------------------
// Statements and values

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Str(String),
    Sym(String),
    Num(String),
    Bool(bool),
    Nil,
    Arr(Vec<Val>),
    Hash(Vec<(String, Val)>),
    /// `-> { "now()" }`
    Lambda(Box<Val>),
    Other,
}

impl Val {
    /// String or symbol contents.
    fn text(&self) -> Option<&str> {
        match self {
            Val::Str(s) | Val::Sym(s) => Some(s),
            _ => None,
        }
    }
    fn truthy(&self) -> bool {
        !matches!(self, Val::Bool(false) | Val::Nil)
    }
    /// A single name or a list of names.
    fn names(&self) -> Vec<String> {
        match self {
            Val::Arr(items) => items.iter().filter_map(|v| v.text().map(str::to_string)).collect(),
            v => v.text().map(|s| vec![s.to_string()]).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Default)]
struct Call {
    recv: Option<String>,
    method: String,
    args: Vec<Val>,
    opts: Vec<(String, Val)>,
}

impl Call {
    fn opt(&self, k: &str) -> Option<&Val> {
        self.opts.iter().find(|(n, _)| n == k).map(|(_, v)| v)
    }
    fn opt_str(&self, k: &str) -> Option<String> {
        self.opt(k).and_then(|v| v.text()).map(str::to_string)
    }
    fn has(&self, k: &str) -> bool {
        self.opt(k).is_some_and(Val::truthy)
    }
}

#[derive(Debug)]
struct Stmt {
    head: Vec<Tok>,
    block: Option<Vec<Stmt>>,
}

struct Parser<'a> {
    t: &'a [Tok],
    i: usize,
}

const BLOCK_KWS: &[&str] = &["if", "unless", "case", "while", "until", "begin", "def", "class", "module", "for"];

impl Parser<'_> {
    /// Parse statements until EOF or a closing `end` (when `nested`).
    fn block(&mut self, nested: bool) -> Vec<Stmt> {
        let mut out = Vec::new();
        loop {
            while matches!(self.t.get(self.i), Some(Tok::Newline) | Some(Tok::Punct(";"))) {
                self.i += 1;
            }
            let Some(first) = self.t.get(self.i) else { return out };
            if let Tok::Ident(w) = first {
                if w == "end" {
                    self.i += 1;
                    if nested {
                        return out;
                    }
                    continue;
                }
            }
            let opens_kw = matches!(first, Tok::Ident(w) if BLOCK_KWS.contains(&w.as_str()));
            let mut head = Vec::new();
            let mut depth = 0i32;
            let mut has_do = false;
            while let Some(t) = self.t.get(self.i) {
                match t {
                    Tok::Newline | Tok::Punct(";") if depth == 0 => {
                        let cont = matches!(head.last(), Some(Tok::Punct(",")) | Some(Tok::Punct("=>")) | Some(Tok::Punct("(")) | Some(Tok::Punct("[")) | Some(Tok::Label(_)));
                        if !cont {
                            break;
                        }
                    }
                    Tok::Newline => {}
                    Tok::Punct("(") | Tok::Punct("[") | Tok::Punct("{") => {
                        depth += 1;
                        head.push(t.clone());
                    }
                    Tok::Punct(")") | Tok::Punct("]") | Tok::Punct("}") => {
                        depth -= 1;
                        head.push(t.clone());
                    }
                    Tok::Ident(w) if w == "do" && depth == 0 => {
                        has_do = true;
                        // skip `|t|` and the rest of the line
                        while !matches!(self.t.get(self.i), None | Some(Tok::Newline)) {
                            self.i += 1;
                        }
                        break;
                    }
                    _ => head.push(t.clone()),
                }
                self.i += 1;
            }
            let block = if has_do || opens_kw { Some(self.block(true)) } else { None };
            out.push(Stmt { head, block });
        }
    }
}

/// Parse a statement head (`recv.method arg, key: val`) into a call.
fn call(head: &[Tok]) -> Option<Call> {
    let mut i = 0;
    let mut c = Call::default();
    // ActiveRecord::Schema[7.1].define(...) — keep only the last method name
    while i < head.len() {
        match &head[i] {
            Tok::Ident(w) | Tok::Const(w) => {
                if matches!(head.get(i + 1), Some(Tok::Punct(".")) | Some(Tok::Punct("::"))) {
                    c.recv = Some(w.clone());
                    i += 2;
                    continue;
                }
                if let (Tok::Const(_), Some(Tok::Punct("["))) = (&head[i], head.get(i + 1)) {
                    // Schema[7.1].define
                    let close = head[i..].iter().position(|t| *t == Tok::Punct("]")).map_or(head.len(), |k| i + k);
                    if let Some(Tok::Num(v)) = head.get(i + 2) {
                        c.args.push(Val::Num(v.clone()));
                    }
                    i = close + 1;
                    if matches!(head.get(i), Some(Tok::Punct("."))) {
                        i += 1;
                    }
                    continue;
                }
                c.method = w.clone();
                i += 1;
                break;
            }
            _ => return None,
        }
    }
    if c.method.is_empty() {
        return None;
    }
    let mut end = head.len();
    if matches!(head.get(i), Some(Tok::Punct("("))) {
        end = matching(head, i);
        i += 1;
    }
    let mut v = ValParser { t: &head[..end.min(head.len())], i };
    v.args(&mut c.args, &mut c.opts);
    Some(c)
}

fn matching(t: &[Tok], open: usize) -> usize {
    let mut depth = 0;
    for (k, tok) in t.iter().enumerate().skip(open) {
        match tok {
            Tok::Punct("(") | Tok::Punct("[") | Tok::Punct("{") => depth += 1,
            Tok::Punct(")") | Tok::Punct("]") | Tok::Punct("}") => {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
            _ => {}
        }
    }
    t.len()
}

struct ValParser<'a> {
    t: &'a [Tok],
    i: usize,
}

impl ValParser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn args(&mut self, args: &mut Vec<Val>, opts: &mut Vec<(String, Val)>) {
        while self.i < self.t.len() {
            if let Some(Tok::Label(k)) = self.peek() {
                let k = k.clone();
                self.i += 1;
                opts.push((k, self.value()));
            } else {
                let v = self.value();
                if matches!(self.peek(), Some(Tok::Punct("=>"))) {
                    self.i += 1;
                    let k = v.text().unwrap_or_default().to_string();
                    opts.push((k, self.value()));
                } else if let Val::Hash(h) = v {
                    opts.extend(h);
                } else {
                    args.push(v);
                }
            }
            if !matches!(self.peek(), Some(Tok::Punct(","))) {
                break;
            }
            self.i += 1;
        }
    }
    fn value(&mut self) -> Val {
        let Some(t) = self.peek().cloned() else { return Val::Other };
        self.i += 1;
        let v = match t {
            Tok::Str(s) => {
                let mut s = s;
                // adjacent literals concatenate: "a" "b"
                while let Some(Tok::Str(more)) = self.peek() {
                    s.push_str(more);
                    self.i += 1;
                }
                Val::Str(s)
            }
            Tok::Sym(s) => Val::Sym(s),
            Tok::Num(n) => Val::Num(n),
            Tok::Punct("-") => match self.peek().cloned() {
                Some(Tok::Num(n)) => {
                    self.i += 1;
                    Val::Num(format!("-{n}"))
                }
                _ => Val::Other,
            },
            Tok::Words(w) => Val::Arr(w.into_iter().map(Val::Str).collect()),
            Tok::Ident(w) if w == "true" => Val::Bool(true),
            Tok::Ident(w) if w == "false" => Val::Bool(false),
            Tok::Ident(w) if w == "nil" => Val::Nil,
            Tok::Punct("[") => {
                let mut items = Vec::new();
                while !matches!(self.peek(), None | Some(Tok::Punct("]"))) {
                    items.push(self.value());
                    if matches!(self.peek(), Some(Tok::Punct(","))) {
                        self.i += 1;
                    } else if !matches!(self.peek(), Some(Tok::Punct("]"))) {
                        self.skip_to(&["]"]);
                    }
                }
                self.i += 1;
                Val::Arr(items)
            }
            Tok::Punct("{") => {
                let mut h = Vec::new();
                while !matches!(self.peek(), None | Some(Tok::Punct("}"))) {
                    let k = if let Some(Tok::Label(k)) = self.peek().cloned() {
                        self.i += 1;
                        k
                    } else {
                        let k = self.value().text().unwrap_or_default().to_string();
                        if matches!(self.peek(), Some(Tok::Punct("=>"))) {
                            self.i += 1;
                        }
                        k
                    };
                    h.push((k, self.value()));
                    if matches!(self.peek(), Some(Tok::Punct(","))) {
                        self.i += 1;
                    } else if !matches!(self.peek(), Some(Tok::Punct("}"))) {
                        self.skip_to(&["}"]);
                    }
                }
                self.i += 1;
                Val::Hash(h)
            }
            Tok::Punct("->") => {
                if matches!(self.peek(), Some(Tok::Punct("{"))) {
                    let close = matching(self.t, self.i);
                    let body = self.t.get(self.i + 1).cloned();
                    self.i = (close + 1).min(self.t.len());
                    match body {
                        Some(Tok::Str(s)) => Val::Lambda(Box::new(Val::Str(s))),
                        _ => Val::Other,
                    }
                } else {
                    Val::Other
                }
            }
            Tok::Punct("(") => {
                let close = matching(self.t, self.i - 1);
                self.i = (close + 1).min(self.t.len());
                Val::Other
            }
            _ => Val::Other,
        };
        // tolerate method chains / operators we don't model: `"x".freeze`
        while matches!(self.peek(), Some(Tok::Punct("."))) {
            self.i += 2;
        }
        v
    }
    fn skip_to(&mut self, stops: &[&str]) {
        let mut depth = 0;
        while let Some(t) = self.peek() {
            match t {
                Tok::Punct(p) if depth == 0 && (stops.contains(p) || *p == ",") => return,
                Tok::Punct("(") | Tok::Punct("[") | Tok::Punct("{") => depth += 1,
                Tok::Punct(")") | Tok::Punct("]") | Tok::Punct("}") => depth -= 1,
                _ => {}
            }
            self.i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Translation to SQL

#[derive(Default)]
struct Translator {
    out: String,
    warnings: Vec<String>,
    /// Rails version from `ActiveRecord::Schema[x.y]`; 0 when unknown.
    version: f32,
    /// table id → column names, for inferring foreign key columns.
    columns: HashMap<String, Vec<String>>,
    /// table id → primary key columns.
    pks: HashMap<String, Vec<String>>,
    fks: Vec<Call>,
}

/// Words that would make the SQL parser misread a column definition.
const RESERVED: &[&str] = &[
    "all", "and", "any", "as", "check", "collate", "column", "constraint", "default", "desc", "distinct", "do", "else", "end", "exclude", "false",
    "for", "foreign", "from", "generated", "group", "in", "is", "key", "like", "limit", "not", "null", "offset", "on", "only", "or", "order",
    "primary", "references", "select", "table", "then", "to", "true", "union", "unique", "user", "using", "when", "where", "with",
];

fn ident(s: &str) -> String {
    let simple = s.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if simple && !RESERVED.contains(&s) {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
}

/// `billing.invoices` → (`billing`, `invoices`).
fn split_name(s: &str) -> (String, String) {
    match s.split_once('.') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => ("public".to_string(), s.to_string()),
    }
}

fn qname(s: &str) -> String {
    let (sc, n) = split_name(s);
    format!("{}.{}", ident(&sc), ident(&n))
}

fn table_id(s: &str) -> String {
    let (sc, n) = split_name(s);
    format!("{sc}.{n}")
}

fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Ruby value → JSON text (for json/jsonb defaults).
fn json(v: &Val) -> String {
    match v {
        Val::Str(s) | Val::Sym(s) => serde_json::Value::String(s.clone()).to_string(),
        Val::Num(n) => n.clone(),
        Val::Bool(b) => b.to_string(),
        Val::Nil | Val::Other | Val::Lambda(_) => "null".into(),
        Val::Arr(items) => format!("[{}]", items.iter().map(json).collect::<Vec<_>>().join(",")),
        Val::Hash(h) => format!(
            "{{{}}}",
            h.iter().map(|(k, v)| format!("{}:{}", serde_json::Value::String(k.clone()), json(v))).collect::<Vec<_>>().join(",")
        ),
    }
}

/// Ruby array value → Postgres array literal text (`{a,b}`).
fn pg_array(v: &Val) -> String {
    match v {
        Val::Arr(items) => format!("{{{}}}", items.iter().map(pg_array).collect::<Vec<_>>().join(",")),
        Val::Str(s) | Val::Sym(s) => {
            if s.is_empty() || s.contains([',', '{', '}', '"', '\\', ' ']) {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                s.clone()
            }
        }
        Val::Num(n) => n.clone(),
        Val::Bool(b) => b.to_string(),
        _ => "NULL".into(),
    }
}

impl Translator {
    fn emit(&mut self, sql: impl AsRef<str>) {
        self.out.push_str(sql.as_ref().trim_end().trim_end_matches(';'));
        self.out.push_str(";\n\n");
    }
    fn warn(&mut self, msg: String) {
        if self.warnings.len() < 200 {
            self.warnings.push(msg);
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            let Some(c) = call(&s.head) else { continue };
            match c.method.as_str() {
                "define" => {
                    if let Some(Val::Num(v)) = c.args.first() {
                        self.version = v.parse().unwrap_or(0.0);
                    }
                    if let Some(b) = &s.block {
                        self.stmts(b);
                    }
                }
                "create_table" => self.create_table(&c, s.block.as_deref().unwrap_or(&[])),
                "enable_extension" => {
                    if let Some(n) = c.args.first().and_then(Val::text) {
                        let n = n.rsplit('.').next().unwrap_or(n).to_string();
                        self.emit(format!("CREATE EXTENSION IF NOT EXISTS {}", ident(&n)));
                    }
                }
                "create_schema" => {
                    if let Some(n) = c.args.first().and_then(Val::text) {
                        self.emit(format!("CREATE SCHEMA {}", ident(n)));
                    }
                }
                "create_enum" => {
                    let name = c.args.first().and_then(Val::text).map(str::to_string);
                    let values = c.args.get(1).map(Val::names).unwrap_or_default();
                    if let Some(n) = name {
                        let vals: Vec<String> = values.iter().map(|v| lit(v)).collect();
                        self.emit(format!("CREATE TYPE {} AS ENUM ({})", qname(&n), vals.join(", ")));
                    }
                }
                "add_index" => {
                    if let (Some(t), Some(cols)) = (c.args.first().and_then(Val::text).map(str::to_string), c.args.get(1).cloned()) {
                        self.index(&t, &cols, &c);
                    }
                }
                "add_foreign_key" => self.fks.push(c),
                "add_check_constraint" => {
                    if let (Some(t), Some(e)) = (c.args.first().and_then(Val::text), c.args.get(1).and_then(Val::text)) {
                        let name = c.opt_str("name").map(|n| format!("CONSTRAINT {} ", ident(&n))).unwrap_or_default();
                        self.emit(format!("ALTER TABLE ONLY {} ADD {name}CHECK ({e})", qname(t)));
                    }
                }
                "add_unique_constraint" => {
                    if let (Some(t), Some(cols)) = (c.args.first().and_then(Val::text), c.args.get(1)) {
                        let name = c.opt_str("name").map(|n| format!("CONSTRAINT {} ", ident(&n))).unwrap_or_default();
                        let cols: Vec<String> = cols.names().iter().map(|c| ident(c)).collect();
                        self.emit(format!("ALTER TABLE ONLY {} ADD {name}UNIQUE ({})", qname(t), cols.join(", ")));
                    }
                }
                // scenic
                "create_view" => {
                    let name = c.args.first().and_then(Val::text).map(str::to_string);
                    let sql = c.opt_str("sql_definition");
                    if let (Some(n), Some(sql)) = (name, sql) {
                        let kind = if c.has("materialized") { "MATERIALIZED VIEW" } else { "VIEW" };
                        self.emit(format!("CREATE {kind} {} AS\n{}", qname(&n), sql.trim()));
                    }
                }
                // fx, and raw SQL
                "create_function" | "create_trigger" => {
                    if let Some(sql) = c.opt_str("sql_definition") {
                        self.emit(sql);
                    }
                }
                "execute" => {
                    if let Some(sql) = c.args.first().and_then(Val::text).map(str::to_string) {
                        self.emit(sql);
                    }
                }
                _ => {
                    if let Some(b) = &s.block {
                        self.stmts(b);
                    }
                }
            }
        }
    }

    fn create_table(&mut self, c: &Call, body: &[Stmt]) {
        let Some(name) = c.args.first().and_then(Val::text).map(str::to_string) else {
            self.warn("create_table without a table name".into());
            return;
        };
        let (_, bare) = split_name(&name);
        let id = table_id(&name);
        let mut cols: Vec<(String, String)> = Vec::new(); // (name, definition)
        let mut constraints: Vec<String> = Vec::new();
        let mut comments: Vec<(String, String)> = Vec::new();
        let mut indexes: Vec<(Val, Call)> = Vec::new();

        // primary key
        let id_opt = c.opt("id");
        let pk_opt = c.opt("primary_key");
        let mut pk: Vec<String> = match pk_opt {
            Some(v @ Val::Arr(_)) => v.names(),
            Some(v) => v.names(),
            None => vec!["id".into()],
        };
        if matches!(id_opt, Some(Val::Bool(false))) && !matches!(pk_opt, Some(Val::Arr(_))) {
            pk.clear();
        } else if pk.len() == 1 {
            let ty = match id_opt.and_then(Val::text) {
                Some(t) => t.to_string(),
                None => "primary_key".into(),
            };
            let mut def = match ty.as_str() {
                "primary_key" | "bigserial" => {
                    let seq = format!("{}_{}_seq", bare, pk[0]);
                    format!("bigint DEFAULT nextval({}::regclass)", lit(&format!("{}.{}", split_name(&name).0, seq)))
                }
                "serial" => {
                    let seq = format!("{}_{}_seq", bare, pk[0]);
                    format!("integer DEFAULT nextval({}::regclass)", lit(&format!("{}.{}", split_name(&name).0, seq)))
                }
                other => {
                    let mut d = self.sql_type(other, c);
                    if let Some(dv) = c.opt("default").and_then(|v| self.default_sql(v, &d)) {
                        d = format!("{d} DEFAULT {dv}");
                    }
                    d
                }
            };
            def.push_str(" NOT NULL");
            cols.push((pk[0].clone(), def));
        }

        for s in body {
            let Some(tc) = call(&s.head) else { continue };
            if tc.recv.is_none() {
                continue;
            }
            match tc.method.as_str() {
                "index" => {
                    if let Some(v) = tc.args.first().cloned() {
                        indexes.push((v, tc));
                    }
                }
                "check_constraint" => {
                    if let Some(e) = tc.args.first().and_then(Val::text) {
                        let name = tc.opt_str("name").map(|n| format!("CONSTRAINT {} ", ident(&n))).unwrap_or_default();
                        constraints.push(format!("{name}CHECK ({e})"));
                    }
                }
                "unique_constraint" => {
                    if let Some(v) = tc.args.first() {
                        let name = tc.opt_str("name").map(|n| format!("CONSTRAINT {} ", ident(&n))).unwrap_or_default();
                        let cs: Vec<String> = v.names().iter().map(|c| ident(c)).collect();
                        constraints.push(format!("{name}UNIQUE ({})", cs.join(", ")));
                    }
                }
                "exclusion_constraint" | "foreign_key" => {}
                "timestamps" => {
                    for n in ["created_at", "updated_at"] {
                        let mut d = self.sql_type("datetime", &tc);
                        if !matches!(tc.opt("null"), Some(Val::Bool(true))) {
                            d.push_str(" NOT NULL");
                        }
                        cols.push((n.into(), d));
                    }
                }
                "references" | "belongs_to" => {
                    for r in tc.args.iter().filter_map(Val::text) {
                        let col = format!("{r}_id");
                        let ty = tc.opt_str("type").unwrap_or_else(|| "bigint".into());
                        let mut d = self.sql_type(&ty, &tc);
                        if matches!(tc.opt("null"), Some(Val::Bool(false))) {
                            d.push_str(" NOT NULL");
                        }
                        let polymorphic = tc.has("polymorphic");
                        if polymorphic {
                            cols.push((format!("{r}_type"), "character varying".into()));
                        }
                        cols.push((col.clone(), d));
                        if !matches!(tc.opt("index"), Some(Val::Bool(false))) {
                            let on = if polymorphic { Val::Arr(vec![Val::Str(format!("{r}_type")), Val::Str(col.clone())]) } else { Val::Str(col.clone()) };
                            let mut ic = Call::default();
                            if let Some(Val::Hash(h)) = tc.opt("index") {
                                ic.opts = h.clone();
                            }
                            indexes.push((on, ic));
                        }
                        if let Some(fk) = tc.opt("foreign_key").filter(|v| v.truthy()) {
                            let mut fc = Call { method: "add_foreign_key".into(), ..Default::default() };
                            let to = match fk {
                                Val::Hash(h) => {
                                    fc.opts = h.iter().filter(|(k, _)| k != "to_table").cloned().collect();
                                    h.iter().find(|(k, _)| k == "to_table").and_then(|(_, v)| v.text()).map(str::to_string)
                                }
                                _ => None,
                            };
                            fc.args = vec![Val::Str(name.clone()), Val::Str(to.unwrap_or_else(|| pluralize(r)))];
                            fc.opts.push(("column".into(), Val::Str(col.clone())));
                            self.fks.push(fc);
                        }
                    }
                }
                method => {
                    let (ty, names): (String, Vec<String>) = if method == "column" {
                        let n = tc.args.first().map(Val::names).unwrap_or_default();
                        let ty = tc.args.get(1).and_then(Val::text).unwrap_or("string").to_string();
                        (ty, n)
                    } else {
                        (method.to_string(), tc.args.iter().flat_map(Val::names).collect())
                    };
                    for n in names {
                        let def = self.column(&ty, &tc);
                        if let Some(cm) = tc.opt_str("comment") {
                            comments.push((n.clone(), cm));
                        }
                        if tc.has("index") {
                            let mut ic = Call::default();
                            if let Some(Val::Hash(h)) = tc.opt("index") {
                                ic.opts = h.clone();
                            }
                            indexes.push((Val::Str(n.clone()), ic));
                        }
                        cols.retain(|(c, _)| *c != n);
                        cols.push((n, def));
                    }
                }
            }
        }

        if !pk.is_empty() {
            for (n, d) in cols.iter_mut() {
                if pk.contains(n) && !d.contains("NOT NULL") {
                    d.push_str(" NOT NULL");
                }
            }
            let pk_cols: Vec<String> = pk.iter().map(|c| ident(c)).collect();
            constraints.push(format!("CONSTRAINT {} PRIMARY KEY ({})", ident(&format!("{bare}_pkey")), pk_cols.join(", ")));
        }

        let mut body_sql: Vec<String> = cols.iter().map(|(n, d)| format!("    {} {d}", ident(n))).collect();
        body_sql.extend(constraints.iter().map(|c| format!("    {c}")));
        self.emit(format!("CREATE TABLE {} (\n{}\n)", qname(&name), body_sql.join(",\n")));
        if pk.len() == 1 && cols.iter().any(|(n, d)| *n == pk[0] && d.contains("nextval(")) {
            let seq = format!("{}.{bare}_{}_seq", split_name(&name).0, pk[0]);
            self.emit(format!("CREATE SEQUENCE {}", qname(&seq)));
            self.emit(format!("ALTER SEQUENCE {} OWNED BY {}.{}", qname(&seq), qname(&name), ident(&pk[0])));
        }
        if let Some(cm) = c.opt_str("comment") {
            self.emit(format!("COMMENT ON TABLE {} IS {}", qname(&name), lit(&cm)));
        }
        for (col, cm) in comments {
            self.emit(format!("COMMENT ON COLUMN {}.{} IS {}", qname(&name), ident(&col), lit(&cm)));
        }
        self.columns.insert(id.clone(), cols.into_iter().map(|(n, _)| n).collect());
        self.pks.insert(id, pk);
        for (on, ic) in indexes {
            self.index(&name, &on, &ic);
        }
    }

    /// Column definition (type and modifiers) for `t.<type> "name", opts`.
    fn column(&mut self, ty: &str, c: &Call) -> String {
        if ty == "virtual" {
            let inner = c.opt_str("type").unwrap_or_else(|| "string".into());
            let mut d = self.sql_type(&inner, c);
            if let Some(expr) = c.opt_str("as") {
                d.push_str(&format!(" GENERATED ALWAYS AS ({expr}){}", if c.has("stored") { " STORED" } else { "" }));
            }
            return d;
        }
        let mut d = self.sql_type(ty, c);
        if let Some(coll) = c.opt_str("collation") {
            d.push_str(&format!(" COLLATE {}", ident(&coll)));
        }
        if let Some(v) = c.opt("default").and_then(|v| self.default_sql(v, &self.sql_type(ty, c))) {
            d.push_str(&format!(" DEFAULT {v}"));
        }
        if matches!(c.opt("null"), Some(Val::Bool(false))) {
            d.push_str(" NOT NULL");
        }
        d
    }

    /// Rails column type (+ limit / precision / scale / array) → Postgres type.
    fn sql_type(&self, ty: &str, c: &Call) -> String {
        let num = |k: &str| c.opt(k).and_then(|v| if let Val::Num(n) = v { Some(n.clone()) } else { None });
        let precision_given = c.opt("precision").is_some();
        let precision = num("precision");
        let limit = num("limit");
        let time_precision = || {
            if precision_given {
                precision.clone()
            } else if self.version >= 7.0 {
                Some("6".into())
            } else {
                None
            }
        };
        let with_p = |base: &str, p: Option<String>, tail: &str| match p {
            Some(p) => format!("{base}({p}){tail}"),
            None => format!("{base}{tail}"),
        };
        let base = match ty {
            "string" => with_p("character varying", limit.clone(), ""),
            "char" | "character" => with_p("character", limit.clone(), ""),
            "text" => "text".into(),
            "integer" | "int" => match limit.as_deref() {
                Some("1") | Some("2") => "smallint".into(),
                Some("8") => "bigint".into(),
                _ => "integer".into(),
            },
            "bigint" => "bigint".into(),
            "smallint" => "smallint".into(),
            "float" => match limit.as_deref().and_then(|l| l.parse::<u32>().ok()) {
                Some(l) if l <= 24 => "real".into(),
                _ => "double precision".into(),
            },
            "real" => "real".into(),
            "decimal" | "numeric" => match (precision, num("scale")) {
                (Some(p), Some(s)) => format!("numeric({p},{s})"),
                (Some(p), None) => format!("numeric({p})"),
                _ => "numeric".into(),
            },
            "datetime" | "timestamp" => with_p("timestamp", time_precision(), " without time zone"),
            "timestamptz" => with_p("timestamp", time_precision(), " with time zone"),
            "time" => with_p("time", precision, " without time zone"),
            "timetz" => with_p("time", precision, " with time zone"),
            "date" => "date".into(),
            "boolean" => "boolean".into(),
            "binary" => "bytea".into(),
            "bit" => with_p("bit", limit.clone(), ""),
            "bit_varying" => with_p("bit varying", limit.clone(), ""),
            "primary_key" => "bigint".into(),
            "serial" => "integer".into(),
            "bigserial" => "bigint".into(),
            "enum" => {
                let e = c.opt_str("enum_type").unwrap_or_else(|| "enum".into());
                qname(&e)
            }
            other => other.replace('_', " "),
        };
        if c.has("array") {
            format!("{base}[]")
        } else {
            base
        }
    }

    /// Ruby default value → SQL default expression.
    fn default_sql(&self, v: &Val, ty: &str) -> Option<String> {
        Some(match v {
            Val::Nil | Val::Other => return None,
            Val::Lambda(b) => b.text()?.to_string(),
            Val::Bool(b) => b.to_string(),
            Val::Num(n) => {
                if n.starts_with('-') {
                    format!("'{n}'::{ty}")
                } else {
                    n.clone()
                }
            }
            Val::Str(s) | Val::Sym(s) => format!("{}::{ty}", lit(s)),
            Val::Arr(_) => format!("{}::{ty}", lit(&pg_array(v))),
            Val::Hash(_) => format!("{}::{ty}", lit(&json(v))),
        })
    }

    /// `t.index` / `add_index` → CREATE INDEX.
    fn index(&mut self, table: &str, on: &Val, c: &Call) {
        let (_, bare) = split_name(table);
        let order = c.opt("order");
        let opclass = c.opt("opclass");
        let per_col = |h: Option<&Val>, col: &str| -> Option<String> {
            match h? {
                Val::Hash(h) => h.iter().find(|(k, _)| k == col).and_then(|(_, v)| v.text()).map(str::to_string),
                v => v.text().map(str::to_string),
            }
        };
        let (cols, names): (Vec<String>, Vec<String>) = match on {
            Val::Str(expr) if !matches!(on, Val::Arr(_)) && (expr.contains('(') || expr.contains(' ') || expr.contains(',')) => {
                (vec![expr.clone()], vec![])
            }
            _ => {
                let names = on.names();
                let cols = names
                    .iter()
                    .map(|n| {
                        let mut s = ident(n);
                        if let Some(op) = per_col(opclass, n) {
                            s.push(' ');
                            s.push_str(&op);
                        }
                        if let Some(o) = per_col(order, n) {
                            s.push(' ');
                            s.push_str(&o.to_uppercase().replace("_", " "));
                        }
                        s
                    })
                    .collect();
                (cols, names)
            }
        };
        if cols.is_empty() {
            return;
        }
        let name = c.opt_str("name").unwrap_or_else(|| {
            if names.is_empty() {
                format!("index_{bare}_on_expression")
            } else {
                format!("index_{bare}_on_{}", names.join("_and_"))
            }
        });
        let unique = if c.has("unique") { "UNIQUE " } else { "" };
        let using = c.opt_str("using").unwrap_or_else(|| "btree".into());
        let mut sql = format!("CREATE {unique}INDEX {} ON {} USING {} ({})", ident(&name), qname(table), ident(&using), cols.join(", "));
        if let Some(inc) = c.opt("include") {
            let inc: Vec<String> = inc.names().iter().map(|n| ident(n)).collect();
            sql.push_str(&format!(" INCLUDE ({})", inc.join(", ")));
        }
        if c.has("nulls_not_distinct") {
            sql.push_str(" NULLS NOT DISTINCT");
        }
        if let Some(w) = c.opt_str("where") {
            sql.push_str(&format!(" WHERE {w}"));
        }
        self.emit(sql);
    }

    /// `add_foreign_key` statements, emitted once every table is known so the
    /// default column (`<singular>_id`) can be checked against real columns.
    fn flush_foreign_keys(&mut self) {
        for c in std::mem::take(&mut self.fks) {
            let (Some(from), Some(to)) = (c.args.first().and_then(Val::text), c.args.get(1).and_then(Val::text)) else {
                self.warn("add_foreign_key needs a from and to table".into());
                continue;
            };
            let from_cols = self.columns.get(&table_id(from)).cloned().unwrap_or_default();
            let cols = match c.opt("column") {
                Some(v) => v.names(),
                None => vec![fk_column(to, &from_cols)],
            };
            let refs = match c.opt("primary_key") {
                Some(v) => v.names(),
                None => match self.pks.get(&table_id(to)) {
                    Some(pk) if pk.len() == cols.len() => pk.clone(),
                    _ => vec!["id".into()],
                },
            };
            let mut sql = format!("ALTER TABLE ONLY {} ADD ", qname(from));
            if let Some(n) = c.opt_str("name") {
                sql.push_str(&format!("CONSTRAINT {} ", ident(&n)));
            }
            let q = |v: &[String]| v.iter().map(|c| ident(c)).collect::<Vec<_>>().join(", ");
            sql.push_str(&format!("FOREIGN KEY ({}) REFERENCES {}({})", q(&cols), qname(to), q(&refs)));
            for (k, kw) in [("on_delete", "ON DELETE"), ("on_update", "ON UPDATE")] {
                let action = match c.opt_str(k).as_deref() {
                    Some("cascade") => Some("CASCADE"),
                    Some("nullify") => Some("SET NULL"),
                    Some("restrict") => Some("RESTRICT"),
                    Some("no_action") => Some("NO ACTION"),
                    Some("set_default") => Some("SET DEFAULT"),
                    _ => None,
                };
                if let Some(a) = action {
                    sql.push_str(&format!(" {kw} {a}"));
                }
            }
            match c.opt("deferrable") {
                Some(Val::Sym(d)) | Some(Val::Str(d)) if d == "deferred" => sql.push_str(" DEFERRABLE INITIALLY DEFERRED"),
                Some(Val::Sym(d)) | Some(Val::Str(d)) if d == "immediate" => sql.push_str(" DEFERRABLE"),
                Some(Val::Bool(true)) => sql.push_str(" DEFERRABLE"),
                _ => {}
            }
            if matches!(c.opt("validate"), Some(Val::Bool(false))) {
                sql.push_str(" NOT VALID");
            }
            self.emit(sql);
        }
    }
}

/// Rails' default foreign key column for a reference to `to_table`, checked
/// against the columns that actually exist on the referencing table.
fn fk_column(to_table: &str, existing: &[String]) -> String {
    let bare = to_table.rsplit('.').next().unwrap_or(to_table);
    let guess = format!("{}_id", singularize(bare));
    if existing.is_empty() || existing.contains(&guess) {
        return guess;
    }
    let alternates = [bare.strip_suffix('s'), bare.strip_suffix("es"), bare.strip_suffix("ies").map(|_| ""), Some(bare)];
    for a in alternates.into_iter().flatten() {
        let cand = if a.is_empty() { format!("{}y_id", &bare[..bare.len() - 3]) } else { format!("{a}_id") };
        if existing.contains(&cand) {
            return cand;
        }
    }
    guess
}

/// A compact take on ActiveSupport's English singularization rules.
pub fn singularize(word: &str) -> String {
    const UNCOUNTABLE: &[&str] = &["equipment", "information", "rice", "money", "species", "series", "fish", "sheep", "jeans", "police", "news", "metadata"];
    const IRREGULAR: &[(&str, &str)] = &[
        ("people", "person"),
        ("men", "man"),
        ("children", "child"),
        ("sexes", "sex"),
        ("moves", "move"),
        ("zombies", "zombie"),
        ("databases", "database"),
    ];
    // operate on the last `_`-separated word, like ActiveSupport
    let (prefix, last) = match word.rfind('_') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let lower = last.to_ascii_lowercase();
    if UNCOUNTABLE.contains(&lower.as_str()) {
        return word.to_string();
    }
    if let Some((p, s)) = IRREGULAR.iter().find(|(p, _)| lower.ends_with(p)) {
        return format!("{prefix}{}{s}", &last[..last.len() - p.len()]);
    }
    // (suffix, replacement), first match wins
    const RULES: &[(&str, &str)] = &[
        ("quizzes", "quiz"),
        ("matrices", "matrix"),
        ("vertices", "vertex"),
        ("indices", "index"),
        ("oxen", "ox"),
        ("aliases", "alias"),
        ("statuses", "status"),
        ("octopi", "octopus"),
        ("viri", "virus"),
        ("axes", "axis"),
        ("crises", "crisis"),
        ("testes", "testis"),
        ("shoes", "shoe"),
        ("buses", "bus"),
        ("mice", "mouse"),
        ("lice", "louse"),
        ("analyses", "analysis"),
        ("bases", "basis"),
        ("diagnoses", "diagnosis"),
        ("parentheses", "parenthesis"),
        ("prognoses", "prognosis"),
        ("synopses", "synopsis"),
        ("theses", "thesis"),
        ("movies", "movie"),
        ("hives", "hive"),
        ("tives", "tive"),
        ("xes", "x"),
        ("ches", "ch"),
        ("sses", "ss"),
        ("shes", "sh"),
        ("oes", "o"),
    ];
    if let Some((suf, rep)) = RULES.iter().find(|(s, _)| lower.ends_with(s)) {
        return format!("{prefix}{}{rep}", &last[..last.len() - suf.len()]);
    }
    let b = lower.as_bytes();
    let n = b.len();
    if lower.ends_with("ies") && n > 3 && (!b"aeiouy".contains(&b[n - 4]) || lower.ends_with("quies")) {
        return format!("{prefix}{}y", &last[..n - 3]);
    }
    if lower.ends_with("ves") && n > 3 {
        let stem = &last[..n - 3];
        return if matches!(b[n - 4], b'l' | b'r') { format!("{prefix}{stem}f") } else { format!("{prefix}{stem}fe") };
    }
    if (lower.ends_with("ta") || lower.ends_with("ia")) && n > 2 && !lower.ends_with("data") {
        // `media` → `medium`, `criteria` stays close enough
        return format!("{prefix}{}um", &last[..n - 1]);
    }
    if lower.ends_with("ss") || lower.ends_with("us") && !lower.ends_with("ius") {
        return word.to_string();
    }
    if let Some(s) = last.strip_suffix('s') {
        return format!("{prefix}{s}");
    }
    word.to_string()
}

fn pluralize(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if lower.ends_with('y') && !lower.ends_with("ay") && !lower.ends_with("ey") && !lower.ends_with("oy") && !lower.ends_with("uy") {
        format!("{}ies", &word[..word.len() - 1])
    } else if ["s", "x", "z", "ch", "sh"].iter().any(|s| lower.ends_with(s)) {
        format!("{word}es")
    } else if lower == "person" {
        "people".into()
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    const SCHEMA_RB: &str = r#"
# This file is auto-generated from the current state of the database.

ActiveRecord::Schema[7.1].define(version: 2024_05_01_120000) do
  # These are extensions that must be enabled in order to support this database
  enable_extension "pgcrypto"
  enable_extension "plpgsql"

  # Custom types defined in this database.
  # Note that some types may not work with other database engines. Be careful if changing database.
  create_enum "invoice_status", ["draft", "sent", "paid"]

  create_table "accounts", id: :uuid, default: -> { "gen_random_uuid()" }, force: :cascade do |t|
    t.string "name", null: false
    t.timestamps
  end

  create_table "categories", force: :cascade, comment: "Product categories" do |t|
    t.string "title", limit: 120, default: "", null: false, comment: "Shown in the menu"
    t.bigint "parent_id"
    t.index ["parent_id"], name: "index_categories_on_parent_id"
  end

  create_table "invoices", force: :cascade do |t|
    t.uuid "account_id", null: false
    t.decimal "total", precision: 12, scale: 2, default: "0.0", null: false
    t.enum "status", default: "draft", null: false, enum_type: "invoice_status"
    t.jsonb "meta", default: {}, null: false
    t.string "tags", default: [], array: true
    t.datetime "sent_at", precision: nil
    t.datetime "created_at", null: false
    t.virtual "total_cents", type: :bigint, as: "((total * (100)::numeric))::bigint", stored: true
    t.index "lower((status)::text)", name: "index_invoices_on_lower_status"
    t.index ["account_id", "status"], name: "index_invoices_on_account_id_and_status", unique: true, where: "(sent_at IS NOT NULL)"
    t.check_constraint "total >= 0", name: "total_positive"
  end

  create_table "invoice_lines", primary_key: ["invoice_id", "position"], force: :cascade do |t|
    t.bigint "invoice_id", null: false
    t.integer "position", null: false
    t.string "order"
  end

  create_table "people", id: :serial, force: :cascade do |t|
    t.string "email"
  end

  create_table "companies", force: :cascade do |t|
    t.integer "person_id"
    t.bigint "parent_category_id"
  end

  add_foreign_key "categories", "categories", column: "parent_id", on_delete: :nullify
  add_foreign_key "invoices", "accounts", on_delete: :cascade
  add_foreign_key "invoice_lines", "invoices"
  add_foreign_key "companies", "people"
  add_foreign_key "companies", "categories", column: "parent_category_id", name: "fk_parent_category"

  create_view "paid_invoices", sql_definition: <<-SQL
      SELECT invoices.id, invoices.total
     FROM invoices
    WHERE (invoices.status = 'paid'::invoice_status);
  SQL
end
"#;

    #[test]
    fn detects_schema_rb() {
        assert!(is_schema_rb(SCHEMA_RB));
        assert!(!is_schema_rb("CREATE TABLE users (id bigint);"));
    }

    #[test]
    fn parses_schema_rb() {
        let s = parse(SCHEMA_RB);
        assert!(s.warnings.is_empty(), "{:?}\n{}", s.warnings, to_sql(SCHEMA_RB).0);
        assert_eq!(s.extensions.len(), 2);
        assert_eq!(s.enums[0].values, vec!["draft", "sent", "paid"]);
        assert_eq!(s.tables.len(), 6);

        let acc = s.table("public.accounts").unwrap();
        assert_eq!(acc.columns[0].data_type, "uuid");
        assert_eq!(acc.columns[0].default.as_deref(), Some("gen_random_uuid()"));
        assert_eq!(acc.primary_key.as_ref().unwrap().columns, vec!["id"]);
        assert_eq!(acc.column("created_at").unwrap().data_type, "timestamp(6) without time zone");
        assert!(!acc.column("updated_at").unwrap().nullable);

        let cat = s.table("public.categories").unwrap();
        assert_eq!(cat.comment.as_deref(), Some("Product categories"));
        assert_eq!(cat.columns[0].data_type, "bigint");
        assert_eq!(cat.columns[0].default.as_deref(), Some("nextval('public.categories_id_seq'::regclass)"));
        let title = cat.column("title").unwrap();
        assert_eq!(title.data_type, "character varying(120)");
        assert_eq!(title.default.as_deref(), Some("''::character varying(120)"));
        assert_eq!(title.comment.as_deref(), Some("Shown in the menu"));
        assert!(!title.nullable);
        assert_eq!(cat.indexes[0].columns, vec!["parent_id"]);
        assert_eq!(cat.foreign_keys[0].ref_table, "public.categories");
        assert_eq!(cat.foreign_keys[0].on_delete.as_deref(), Some("SET NULL"));

        let inv = s.table("public.invoices").unwrap();
        assert_eq!(inv.column("total").unwrap().data_type, "numeric(12,2)");
        assert_eq!(inv.column("status").unwrap().data_type, "public.invoice_status");
        assert_eq!(inv.column("meta").unwrap().default.as_deref(), Some("'{}'::jsonb"));
        assert_eq!(inv.column("tags").unwrap().data_type, "character varying[]");
        assert_eq!(inv.column("tags").unwrap().default.as_deref(), Some("'{}'::character varying[]"));
        assert_eq!(inv.column("sent_at").unwrap().data_type, "timestamp without time zone");
        assert_eq!(inv.column("total_cents").unwrap().generated.as_deref(), Some("((total * (100)::numeric))::bigint"));
        assert_eq!(inv.indexes.len(), 2);
        assert_eq!(inv.indexes[0].columns, vec!["lower((status)::text)"]);
        assert!(inv.indexes[1].unique);
        assert_eq!(inv.indexes[1].columns, vec!["account_id", "status"]);
        assert_eq!(inv.indexes[1].predicate.as_deref(), Some("(sent_at IS NOT NULL)"));
        assert_eq!(inv.checks[0].name.as_deref(), Some("total_positive"));
        assert_eq!(inv.foreign_keys[0].columns, vec!["account_id"]);
        assert_eq!(inv.foreign_keys[0].ref_table, "public.accounts");
        assert_eq!(inv.foreign_keys[0].on_delete.as_deref(), Some("CASCADE"));

        let lines = s.table("public.invoice_lines").unwrap();
        assert_eq!(lines.primary_key.as_ref().unwrap().columns, vec!["invoice_id", "position"]);
        assert!(lines.column("id").is_none());
        assert!(lines.column("order").is_some());
        assert_eq!(lines.foreign_keys[0].columns, vec!["invoice_id"]);

        assert_eq!(s.table("public.people").unwrap().columns[0].data_type, "integer");
        let co = s.table("public.companies").unwrap();
        assert_eq!(co.foreign_keys[0].columns, vec!["person_id"]);
        assert_eq!(co.foreign_keys[1].name.as_deref(), Some("fk_parent_category"));

        assert_eq!(s.views[0].name, "paid_invoices");
        assert_eq!(s.views[0].depends_on, vec!["public.invoices"]);
    }

    #[test]
    fn handles_older_and_handwritten_schemas() {
        let s = parse(
            r#"
ActiveRecord::Schema.define(version: 20190101000000) do
  create_table "billing.invoices", force: :cascade do |t|
    t.datetime "created_at", null: false
    t.references :user, foreign_key: true, null: false
    t.column :amount, :integer, limit: 8
  end
  create_table :users do |t|
    t.string :email, index: { unique: true }
  end
  add_index "billing.invoices", ["created_at"], order: { created_at: :desc }
end
"#,
        );
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let inv = s.table("billing.invoices").unwrap();
        assert_eq!(inv.column("created_at").unwrap().data_type, "timestamp without time zone");
        assert_eq!(inv.column("user_id").unwrap().data_type, "bigint");
        assert_eq!(inv.column("amount").unwrap().data_type, "bigint");
        assert_eq!(inv.foreign_keys[0].ref_table, "public.users");
        assert_eq!(inv.indexes.len(), 2);
        assert!(inv.indexes[1].definition.contains("DESC"));
        let u = s.table("public.users").unwrap();
        assert!(u.indexes[0].unique);
        assert!(s.schemas.contains(&"billing".to_string()));
    }

    #[test]
    fn singularizes_like_rails() {
        for (p, s) in [
            ("users", "user"),
            ("categories", "category"),
            ("people", "person"),
            ("addresses", "address"),
            ("statuses", "status"),
            ("boxes", "box"),
            ("matches", "match"),
            ("line_items", "line_item"),
            ("wolves", "wolf"),
            ("knives", "knife"),
            ("news", "news"),
            ("buses", "bus"),
            ("analyses", "analysis"),
            ("companies", "company"),
            ("days", "day"),
            ("heroes", "hero"),
            ("sales_people", "sales_person"),
        ] {
            assert_eq!(singularize(p), s, "{p}");
        }
    }
}
