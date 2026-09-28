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
            for i in list.iter().filter(|i| target.as_ref().map_or(true, |t| &i.file == t)) {
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
        Cmd::Embed => {
            let out = o.out.clone().or_else(|| Some(PathBuf::from("schema.embed.js")));
            write_out(&out, &export::embed_bundle(), "embed bundle")?;
            if out.as_ref().map_or(false, |p| p.as_os_str() != "-") {
                eprintln!(
                    "usage:\n  <script src=\"schema.embed.js\"></script>\n  <div id=\"erd\" style=\"height:600px\"></div>\n  <script>Schema.mount(document.getElementById('erd'), {{ sql: SQL, config: {{ focus: ['users'] }} }});</script>"
                );
            }
            Ok(())
        }
        Cmd::Svg => {
            let p = Project::open(&o)?;
            let cfg = p.view_config(&o)?;
            let mut s = session_for(&p)?;
            let v = s.view(cfg);
            for n in &v.stats.notices {
                eprintln!("note: {n}");
            }
            write_out(&o.out, &v.svg, "SVG")
        }
        Cmd::Html => {
            let p = Project::open(&o)?;
            let mut cfg = p.view_config(&o)?;
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

fn viewer_url(port: u16, p: &Project, o: &Opts) -> Result<String, String> {
    let mut q = vec![format!("compare={}", server::url_encode(&p.cmp.compare))];
    q.push(format!("base={}", server::url_encode(p.cmp.base.as_deref().unwrap_or(""))));
    let patch = p.cli_patch(o)?;
    if patch.as_object().map_or(false, |m| !m.is_empty()) {
        q.push(format!("cfg={}", server::url_encode(&patch.to_string())));
    }
    if let Some(v) = &o.view {
        q.push(format!("view={}", server::url_encode(v)));
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
        if o.new {
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
