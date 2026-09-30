mod args;
mod assets;
mod export;
mod git;
mod inspect;
mod project;
mod registry;
mod server;
mod skills;

use std::path::PathBuf;
use std::process::ExitCode;

use args::{Cmd, Opts};
use project::Project;
use schema_core::model::display_id;
use schema_core::Session;
use serde_json::json;

pub fn render_esc(s: &str) -> String {
    schema_core::render::esc(s)
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let opts = match args::parse(raw) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("schema: {e}");
            return ExitCode::from(2);
        }
    };
    match run(opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("schema: {e}");
            ExitCode::FAILURE
        }
    }
}

fn write_out(out: &Option<PathBuf>, content: &str, what: &str) -> Result<(), String> {
    match out {
        Some(p) if p.as_os_str() != "-" => {
            std::fs::write(p, content).map_err(|e| format!("{}: {e}", p.display()))?;
            eprintln!("wrote {what} to {} ({} KB)", p.display(), content.len() / 1024);
            Ok(())
        }
        _ => {
            print!("{content}");
            Ok(())
        }
    }
}

fn session_for(p: &Project) -> Result<Session, String> {
    let (cur, base) = p.sources()?;
    let mut s = Session::new();
    s.set_sql(&cur);
    if let Some(b) = base {
        s.set_base_sql(Some(&b));
    }
    Ok(s)
}

fn run(o: Opts) -> Result<(), String> {
    match o.cmd {
        Cmd::Help => {
            print!("{}", args::HELP);
            Ok(())
        }
        Cmd::Version => {
            println!("schema {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Cmd::List => {
            let list = registry::instances();
            if o.json {
                println!("{}", serde_json::to_string_pretty(&list).unwrap());
            } else if list.is_empty() {
                println!("no running instances");
            } else {
                for i in list {
                    println!("{}  pid {:<7} {}", i.url, i.pid, i.file);
                }
            }
            Ok(())
        }
        Cmd::Stop => {
            let list = registry::instances();
            let target: Option<String> = match (&o.file, o.all) {
                (_, true) => None,
                (Some(f), _) => Some(f.canonicalize().map_err(|e| e.to_string())?.display().to_string()),
                (None, false) => {
                    let p = Project::open(&o)?;
                    Some(p.file.display().to_string())
                }
            };
            let mut n = 0;
            for i in list.iter().filter(|i| target.as_ref().is_none_or(|t| &i.file == t)) {
                if registry::stop(i) {
                    n += 1;
                    println!("stopped {} ({})", i.url, i.file);
                }
            }
            if n == 0 {
                println!("nothing to stop");
            }
            Ok(())
        }
        Cmd::Skills => skills::run(&o.positionals, o.global),
        Cmd::Design => design_cmd(&o),
        Cmd::Embed => {
            let out = o.out.clone().or_else(|| Some(PathBuf::from("schema.embed.js")));
            write_out(&out, &export::embed_bundle(), "embed bundle")?;
            if out.as_ref().is_some_and(|p| p.as_os_str() != "-") {
                eprintln!(
                    "usage:\n  <script src=\"schema.embed.js\"></script>\n  <div id=\"erd\" style=\"height:600px\"></div>\n  <script>Schema.mount(document.getElementById('erd'), {{ sql: SQL, config: {{ focus: ['users'] }} }});</script>"
                );
            }
            Ok(())
        }
        Cmd::Svg => {
            let p = Project::open(&o)?;
            let mut cfg = p.view_config(&o)?;
            let mut s = session_for(&p)?;
            auto_changes_only(&p, &o, &s, &mut cfg)?;
            let v = s.view(cfg);
            for n in &v.stats.notices {
                eprintln!("note: {n}");
            }
            write_out(&o.out, &v.svg, "SVG")
        }
        Cmd::Html => {
            let p = Project::open(&o)?;
            let mut cfg = p.view_config(&o)?;
            auto_changes_only(&p, &o, &session_for(&p)?, &mut cfg)?;
            let (cur, base) = p.sources()?;
            let title = cfg.title.clone().unwrap_or_else(|| format!("{} — schema", p.display_name()));
            cfg.title.get_or_insert(title.clone());
            let out = o.out.clone().unwrap_or_else(|| PathBuf::from("schema.html"));
            let html = export::html(&export::HtmlInput {
                sql: &cur,
                base_sql: base.as_deref(),
                cfg: &cfg,
                title,
                subtitle: p.describe_comparison(),
                static_only: o.static_html,
            });
            write_out(&Some(out.clone()), &html, "HTML")?;
            if !o.no_open && std::env::var_os("SCHEMA_NO_OPEN").is_none() && o.out.is_none() {
                server::open_browser(&format!("file://{}", out.canonicalize().unwrap_or(out.clone()).display()));
            }
            Ok(())
        }
        Cmd::Diff => {
            let p = Project::open(&o)?;
            if p.cmp.base.is_none() {
                return Err(format!("nothing to compare: {} has no uncommitted changes. Pass refs (e.g. `main`, `HEAD~1`, `main..feature`) or --base-file", p.display_name()));
            }
            let s = session_for(&p)?;
            if o.json {
                let v = json!({"file": p.display_name(), "base": p.cmp.base, "compare": p.cmp.compare, "diff": s.diff_json()});
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
            } else {
                println!("# Schema changes: {} ({})\n", p.display_name(), p.describe_comparison());
                print!("{}", s.diff_markdown());
            }
            Ok(())
        }
        Cmd::Inspect => {
            let p = Project::open(&o)?;
            let r = o.compare.clone().or_else(|| o.refs.first().cloned()).unwrap_or_else(|| git::WORKTREE.into());
            let schema = schema_core::parse(&p.source(&r)?);
            if let Some(q) = &o.search {
                let hits = inspect::search(&schema, q);
                if o.json {
                    let v: Vec<_> = hits.iter().map(|(t, c, ty)| json!({"table": t, "column": c, "info": ty})).collect();
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                } else {
                    for (t, c, ty) in hits {
                        match c {
                            Some(c) => println!("{}.{}  {ty}", display_id(&t), c),
                            None => println!("{}  (table, {ty})", display_id(&t)),
                        }
                    }
                }
                return Ok(());
            }
            if let Some(tp) = &o.table {
                let tables = inspect::resolve_table(&schema, tp);
                if tables.is_empty() {
                    return Err(format!("no table matches {tp:?}"));
                }
                let ids: Vec<String> = tables.iter().map(|t| t.id()).collect();
                let near = o.depth.map(|d| inspect::neighbourhood(&schema, &ids, d));
                if o.json {
                    let mut s = Session::new();
                    s.set_schema(schema.clone(), None);
                    let v = json!({
                        "tables": ids.iter().map(|id| s.table(id)).collect::<Vec<_>>(),
                        "neighbourhood": near.map(|n| n.into_iter().map(|(id, d)| json!({"id": id, "distance": d})).collect::<Vec<_>>()),
                    });
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                } else {
                    for t in tables {
                        println!("{}", inspect::table_text(&schema, t));
                    }
                    if let Some(n) = near {
                        println!("neighbourhood (depth {}):", o.depth.unwrap_or(1));
                        for (id, d) in n {
                            println!("  {d}  {}", display_id(&id));
                        }
                    }
                }
                return Ok(());
            }
            if o.json {
                println!("{}", serde_json::to_string_pretty(&inspect::compact_json(&schema)).unwrap());
            } else {
                print!("{}", inspect::summary_text(&schema, &p.display_name()));
                for w in &schema.warnings {
                    eprintln!("warning: {w}");
                }
            }
            Ok(())
        }
        Cmd::Serve => serve(o),
    }
}

/// A comparison with table changes opens on what changed, unless the user
/// (flags, --config, --view or the project default) decided otherwise.
fn auto_changes_only(p: &Project, o: &Opts, s: &Session, cfg: &mut schema_core::ViewConfig) -> Result<(), String> {
    let has_changes = s.diff().is_some_and(|d| !d.tables.is_empty());
    let explicit = p.cli_patch(o)?.get("changes_only").is_some()
        || p.project_config().get("default").and_then(|d| d.get("changes_only")).is_some();
    if has_changes && !explicit {
        cfg.changes_only = true;
    }
    Ok(())
}

fn design_cmd(o: &Opts) -> Result<(), String> {
    let p = Project::open(o)?;
    let sub = o.positionals.first().map(|s| s.as_str()).unwrap_or("list");
    let name = || o.positionals.get(1).cloned().ok_or_else(|| format!("usage: schema design {sub} NAME"));
    let generator = format!("schema {}", env!("CARGO_PKG_VERSION"));
    // designs made outside the viewer may lack the base snapshot: take it from the file
    let load = |n: &str| -> Result<schema_core::design::Design, String> {
        let mut d = p.load_design(n)?;
        if d.base_tables.is_empty() && !d.ops.is_empty() {
            let base = schema_core::parse(&p.source(d.source.git_ref.as_deref().unwrap_or(git::WORKTREE)).or_else(|_| p.source(git::WORKTREE))?);
            d.capture_base(&base);
        }
        Ok(d)
    };
    match sub {
        "list" | "ls" => {
            let list = p.list_designs();
            if o.json {
                let v: Vec<_> = list.iter().map(|(s, d)| json!({"slug": s, "name": d.name, "description": d.description, "ops": d.ops.len(), "updated": d.updated})).collect();
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
            } else if list.is_empty() {
                println!("no designs in {} — create one in the viewer's Design tab", p.designs_dir().display());
            } else {
                for (slug, d) in list {
                    println!("{slug:24} {:3} ops  {}{}", d.ops.len(), d.name, if d.description.is_empty() { String::new() } else { format!(" — {}", d.description.lines().next().unwrap_or("")) });
                }
            }
            Ok(())
        }
        "show" | "export" => {
            let d = load(&name()?)?;
            let out = match o.format.as_deref().unwrap_or(if o.json { "json" } else { "md" }) {
                "sql" => schema_core::design::to_sql(&d),
                "json" => serde_json::to_string_pretty(&d).unwrap() + "\n",
                "md" | "markdown" => schema_core::design::to_markdown(&d, &generator),
                "prompt" => schema_core::design::to_agent_prompt(&d, &generator),
                f => return Err(format!("unknown --format {f} (md, sql, json, prompt)")),
            };
            write_out(&o.out, &out, "design")
        }
        "check" => {
            let d = load(&name()?)?;
            let r = o.compare.clone().unwrap_or_else(|| git::WORKTREE.into());
            let actual = schema_core::parse(&p.source(&r)?);
            let items = schema_core::design::check(&d, &actual);
            let done = items.iter().filter(|i| i.status == schema_core::design::CheckStatus::Done).count();
            if o.json {
                println!("{}", serde_json::to_string_pretty(&json!({"design": d.name, "done": done, "total": items.len(), "tables": items})).unwrap());
            } else {
                println!("Design \"{}\" vs {} ({}): {done}/{} tables implemented\n", d.name, p.display_name(), if r == git::WORKTREE { "working tree".to_string() } else { r.clone() }, items.len());
                for i in &items {
                    let mark = match i.status {
                        schema_core::design::CheckStatus::Done => "✓",
                        schema_core::design::CheckStatus::Missing => "✗",
                        schema_core::design::CheckStatus::Differs => "~",
                    };
                    println!("{mark} {}", display_id(&i.table));
                    if i.status != schema_core::design::CheckStatus::Done {
                        for det in &i.details {
                            println!("    - {det}");
                        }
                    }
                }
            }
            if done == items.len() {
                Ok(())
            } else {
                std::process::exit(1)
            }
        }
        other => Err(format!("unknown design command {other:?} (list | show NAME [--format md|sql|json] | check NAME)")),
    }
}

fn viewer_url(port: u16, p: &Project, o: &Opts) -> Result<String, String> {
    let mut q = vec![format!("compare={}", server::url_encode(&p.cmp.compare))];
    q.push(format!("base={}", server::url_encode(p.cmp.base.as_deref().unwrap_or(""))));
    let patch = p.cli_patch(o)?;
    if patch.as_object().is_some_and(|m| !m.is_empty()) {
        q.push(format!("cfg={}", server::url_encode(&patch.to_string())));
    }
    if let Some(v) = &o.view {
        q.push(format!("view={}", server::url_encode(v)));
    }
    if let Some(d) = &o.design {
        q.push(format!("design={}", server::url_encode(&schema_core::design::slugify(d))));
    }
    Ok(format!("http://127.0.0.1:{port}/?{}", q.join("&")))
}

fn serve(o: Opts) -> Result<(), String> {
    let p = Project::open(&o)?;
    // validate the sources and config up front so errors show in the terminal
    let (cur, _) = p.sources()?;
    p.view_config(&o)?;
    let file_key = p.file.display().to_string();
    let quiet = o.quiet;

    if let Some(existing) = registry::find(&file_key) {
        // a rebuilt binary replaces a running instance from an older build
        let stale = !existing.build.is_empty() && existing.build != registry::build_id();
        if o.new || stale {
            if stale && !quiet {
                println!("restarting the running instance (started from an older build)");
            }
            registry::stop(&existing);
            std::thread::sleep(std::time::Duration::from_millis(250));
        } else {
            let url = viewer_url(existing.port, &p, &o)?;
            if !quiet {
                println!("schema already running for {} → {url}", p.display_name());
            } else {
                println!("{url}");
            }
            if !o.no_open {
                server::open_browser(&url);
            }
            return Ok(());
        }
    }

    if o.detach && std::env::var_os("SCHEMA_CHILD").is_none() {
        return detach(&o, &p);
    }

    let (srv, port) = server::bind(o.port)?;
    let url = viewer_url(port, &p, &o)?;
    let schema = schema_core::parse(&cur);
    if !quiet {
        println!("schema {} → {url}", env!("CARGO_PKG_VERSION"));
        println!("  file     {}", p.display_name());
        println!("  compare  {}", p.describe_comparison());
        println!("  tables   {} ({} views, {} enums)", schema.tables.len(), schema.views.len(), schema.enums.len());
        if !schema.warnings.is_empty() {
            println!("  warnings {} (first: {})", schema.warnings.len(), schema.warnings[0]);
        }
        println!("  press Ctrl-C to stop");
    } else {
        println!("{url}");
    }
    if !o.no_open {
        server::open_browser(&url);
    }
    let app = server::App { project: p, port, started: server::now() };
    server::run(srv, app);
    Ok(())
}

/// Re-run ourselves in the background and return once the server is up.
fn detach(o: &Opts, p: &Project) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let args: Vec<String> = o.raw.iter().filter(|a| !matches!(a.as_str(), "-d" | "--detach" | "--background")).cloned().collect();
    let logs = registry::home().join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let log_path = logs.join(format!("{}.log", server::now()));
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(&args)
        .arg("--no-open")
        .arg("--quiet")
        .env("SCHEMA_CHILD", "1")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let child = cmd.spawn().map_err(|e| format!("failed to start background server: {e}"))?;
    let key = p.file.display().to_string();
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(inst) = registry::find(&key) {
            let url = viewer_url(inst.port, p, o)?;
            if o.quiet {
                println!("{url}");
            } else {
                println!("schema running in the background (pid {}) → {url}", child.id());
                println!("  stop with: schema stop {}", p.display_name());
            }
            if !o.no_open {
                server::open_browser(&url);
            }
            return Ok(());
        }
    }
    Err(format!("background server did not start; see {}", log_path.display()))
}
