//! Command line parsing (hand-rolled to support positional git refs).
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub const HELP: &str = r#"schema — visualise Postgres structure.sql files and their git history

USAGE
  schema [FILE] [REFS...] [OPTIONS]      open the interactive viewer in your browser
  schema html    [FILE] [REFS...] -o out.html   standalone HTML with the embedded WASM viewer
  schema svg     [FILE] [REFS...] -o out.svg    static SVG diagram
  schema diff    [FILE] [REFS...] [--json]      schema diff as Markdown (or JSON)
  schema inspect [FILE] [--table T] [--search Q] [--json]   machine readable schema info
  schema embed   [-o schema.embed.js]        write the embeddable JS+WASM bundle
  schema list    [--json]                       running viewer instances
  schema stop    [FILE | --all]                 stop running instances
  schema skills  install [--global] | list | show NAME    LLM agent skills

FILE defaults to db/structure.sql, structure.sql or schema.sql (cwd, then repo root).

REFS
  (none)            HEAD vs working tree if the file has uncommitted changes
  main              main vs working tree
  main..feature     main vs feature        main...feature   merge-base(main, feature) vs feature
  main feature      main vs feature        HEAD~3           last 3 commits
  work | staged | unstaged                 working tree / index comparisons
  --base REF --compare REF                 explicit (use WORKTREE / INDEX pseudo refs)
  --base-file PATH                         diff against another file (no git needed)

VIEW OPTIONS (also usable with html / svg)
  --focus a,b          show these tables and their neighbours    --depth N (default 1)
  --direction both|in|out   neighbour direction for --focus
  --changes-only       only changed tables (+ --context N neighbours); the default for comparisons
  --all-tables         show every table in a comparison, not just the changed ones
  --layout layered|force|grid|circular|radial     --rankdir LR|TB|RL|BT
  --edges curved|orthogonal|straight|hidden       --anchor column|table
  --columns auto|all|keys|relations|referenced|changed|none  --max-columns N
  --unchanged-columns MODE   columns for tables that did not change in a diff (default: referenced;
                             use --all-columns to show them like the changed tables)
  --hide-columns created_at,updated_at,users.encrypted_*
  --include pat,..  --exclude pat,..  --schemas a,b  --group-by none|schema|prefix|custom
  --views  --partitions  --no-isolated  --inferred  --labels  --indexes none|changed|all
  --view NAME          saved view from .schema.json      --config FILE|JSON  extra config
  --title TEXT  --dark

SERVER OPTIONS
  --port N  --no-open  --new (restart existing instance)  --quiet  -d/--detach (run in background)
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Serve,
    List,
    Stop,
    Html,
    Svg,
    Diff,
    Inspect,
    Embed,
    Skills,
    Help,
    Version,
}

#[derive(Debug, Clone)]
pub struct Opts {
    pub cmd: Cmd,
    pub file: Option<PathBuf>,
    pub refs: Vec<String>,
    pub positionals: Vec<String>,
    pub base: Option<String>,
    pub compare: Option<String>,
    pub base_file: Option<PathBuf>,
    pub port: Option<u16>,
    pub no_open: bool,
    pub new: bool,
    pub quiet: bool,
    pub detach: bool,
    pub json: bool,
    pub all: bool,
    pub global: bool,
    pub static_html: bool,
    pub out: Option<PathBuf>,
    pub config: Option<String>,
    pub view: Option<String>,
    pub table: Option<String>,
    pub search: Option<String>,
    pub depth: Option<u32>,
    /// View-config patch built from flags.
    pub patch: Value,
    pub raw: Vec<String>,
}

fn list(v: &str) -> Vec<String> {
    v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn set(patch: &mut Value, path: &[&str], v: Value) {
    let mut cur = patch;
    for (i, k) in path.iter().enumerate() {
        if i == path.len() - 1 {
            cur[*k] = v;
            return;
        }
        if !cur[*k].is_object() {
            cur[*k] = json!({});
        }
        cur = &mut cur[*k];
    }
}

pub fn parse(args: Vec<String>) -> Result<Opts, String> {
    let mut o = Opts {
        cmd: Cmd::Serve,
        file: None,
        refs: vec![],
        positionals: vec![],
        base: None,
        compare: None,
        base_file: None,
        port: None,
        no_open: false,
        new: false,
        quiet: false,
        detach: false,
        json: false,
        all: false,
        global: false,
        static_html: false,
        out: None,
        config: None,
        view: None,
        table: None,
        search: None,
        depth: None,
        patch: json!({}),
        raw: args.clone(),
    };
    let mut it = args.into_iter().peekable();
    if let Some(first) = it.peek() {
        let cmd = match first.as_str() {
            "list" | "ls" => Some(Cmd::List),
            "stop" | "kill" => Some(Cmd::Stop),
            "html" | "export" => Some(Cmd::Html),
            "svg" => Some(Cmd::Svg),
            "diff" => Some(Cmd::Diff),
            "inspect" | "info" => Some(Cmd::Inspect),
            "embed" => Some(Cmd::Embed),
            "skills" | "skill" => Some(Cmd::Skills),
            "open" | "serve" => Some(Cmd::Serve),
            "help" => Some(Cmd::Help),
            _ => None,
        };
        if let Some(c) = cmd {
            if !Path::new(first).is_file() {
                o.cmd = c;
                it.next();
            }
        }
    }
    let mut exclude: Option<Vec<String>> = None;
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut val = |name: &str| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            it.next().ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => o.cmd = Cmd::Help,
            "-V" | "--version" => o.cmd = Cmd::Version,
            "--base" => o.base = Some(val("--base")?),
            "--compare" => o.compare = Some(val("--compare")?),
            "--base-file" => o.base_file = Some(PathBuf::from(val("--base-file")?)),
            "--port" | "-p" => o.port = Some(val("--port")?.parse().map_err(|_| "invalid --port")?),
            "--no-open" => o.no_open = true,
            "--new" => o.new = true,
            "--quiet" | "-q" => o.quiet = true,
            "--detach" | "-d" | "--background" => o.detach = true,
            "--json" => o.json = true,
            "--all" => o.all = true,
            "--global" | "-g" => o.global = true,
            "--static" => o.static_html = true,
            "-o" | "--out" | "--output" => o.out = Some(PathBuf::from(val("--out")?)),
            "--config" => o.config = Some(val("--config")?),
            "--view" => o.view = Some(val("--view")?),
            "--table" | "-t" => o.table = Some(val("--table")?),
            "--search" | "-s" => o.search = Some(val("--search")?),
            "--dir" if o.cmd == Cmd::Skills => {
                let v = val("--dir")?;
                o.positionals.push("--dir".into());
                o.positionals.push(v);
            }
            "--dark" => set(&mut o.patch, &["theme"], json!("dark")),
            "--light" => set(&mut o.patch, &["theme"], json!("light")),
            "--title" => set(&mut o.patch, &["title"], json!(val("--title")?)),
            "--focus" | "-f" => set(&mut o.patch, &["focus"], json!(list(&val("--focus")?))),
            "--depth" => {
                let d: u32 = val("--depth")?.parse().map_err(|_| "invalid --depth")?;
                o.depth = Some(d);
                set(&mut o.patch, &["focus_depth"], json!(d));
            }
            "--direction" => {
                let d = match val("--direction")?.as_str() {
                    "in" | "incoming" => "incoming",
                    "out" | "outgoing" => "outgoing",
                    _ => "both",
                };
                set(&mut o.patch, &["focus_direction"], json!(d));
            }
            "--changes-only" | "--changes" => set(&mut o.patch, &["changes_only"], json!(true)),
            "--all-tables" => set(&mut o.patch, &["changes_only"], json!(false)),
            "--context" => set(&mut o.patch, &["changes_context"], json!(val("--context")?.parse::<u32>().map_err(|_| "invalid --context")?)),
            "--layout" => set(&mut o.patch, &["layout", "algorithm"], json!(val("--layout")?)),
            "--rankdir" => set(&mut o.patch, &["layout", "direction"], json!(val("--rankdir")?.to_uppercase())),
            "--group-by" => set(&mut o.patch, &["layout", "group_by"], json!(val("--group-by")?)),
            "--edges" => set(&mut o.patch, &["edges", "style"], json!(val("--edges")?)),
            "--anchor" => set(&mut o.patch, &["edges", "anchor"], json!(val("--anchor")?)),
            "--inferred" => set(&mut o.patch, &["edges", "inferred"], json!(true)),
            "--labels" => set(&mut o.patch, &["edges", "labels"], json!(true)),
            "--columns" => set(&mut o.patch, &["columns"], json!(val("--columns")?)),
            "--all-columns" => set(&mut o.patch, &["unchanged_columns"], serde_json::Value::Null),
            "--unchanged-columns" => set(&mut o.patch, &["unchanged_columns"], json!(val("--unchanged-columns")?)),
            "--max-columns" => set(&mut o.patch, &["max_columns"], json!(val("--max-columns")?.parse::<usize>().map_err(|_| "invalid --max-columns")?)),
            "--hide-columns" => set(&mut o.patch, &["hide_columns"], json!(list(&val("--hide-columns")?))),
            "--include" => set(&mut o.patch, &["include"], json!(list(&val("--include")?))),
            "--exclude" => exclude.get_or_insert_with(Vec::new).extend(list(&val("--exclude")?)),
            "--schemas" => set(&mut o.patch, &["schemas"], json!(list(&val("--schemas")?))),
            "--views" => set(&mut o.patch, &["show_views"], json!(true)),
            "--partitions" => set(&mut o.patch, &["show_partitions"], json!(true)),
            "--no-isolated" => set(&mut o.patch, &["show_isolated"], json!(false)),
            "--indexes" => set(&mut o.patch, &["indexes"], json!(val("--indexes")?)),
            f if f.starts_with('-') && f.len() > 1 => return Err(format!("unknown option {f} (see --help)")),
            _ => o.positionals.push(a),
        }
    }
    if let Some(mut ex) = exclude {
        let mut all: Vec<String> = schema_core::config::DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect();
        all.append(&mut ex);
        set(&mut o.patch, &["exclude"], json!(all));
    }
    // first positional that looks like a file is the schema file; the rest are refs
    let mut rest = Vec::new();
    for p in std::mem::take(&mut o.positionals) {
        if o.file.is_none() && o.cmd != Cmd::Skills && (Path::new(&p).is_file() || p.ends_with(".sql")) {
            o.file = Some(PathBuf::from(p));
        } else {
            rest.push(p);
        }
    }
    if o.cmd == Cmd::Skills {
        o.positionals = rest;
    } else {
        o.refs = rest;
    }
    Ok(o)
}

/// Find a schema file when none was given.
pub fn detect_file(repo_root: Option<&Path>) -> Option<PathBuf> {
    let candidates = ["db/structure.sql", "structure.sql", "db/schema.sql", "schema.sql"];
    let cwd = std::env::current_dir().ok()?;
    for dir in std::iter::once(cwd.as_path()).chain(repo_root) {
        for c in candidates {
            let p = dir.join(c);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs_and_flags() {
        let o = parse(vec!["db/structure.sql".into(), "main..feature".into(), "--focus".into(), "users,posts".into(), "--layout=force".into(), "--exclude".into(), "audit_*".into()]).unwrap();
        assert_eq!(o.cmd, Cmd::Serve);
        assert_eq!(o.file.as_deref(), Some(Path::new("db/structure.sql")));
        assert_eq!(o.refs, vec!["main..feature"]);
        assert_eq!(o.patch["focus"], json!(["users", "posts"]));
        assert_eq!(o.patch["layout"]["algorithm"], json!("force"));
        assert_eq!(o.patch["exclude"].as_array().unwrap().len(), 3);
        let o = parse(vec!["html".into(), "x.sql".into(), "-o".into(), "out.html".into()]).unwrap();
        assert_eq!(o.cmd, Cmd::Html);
        assert_eq!(o.out.as_deref(), Some(Path::new("out.html")));
    }
}
