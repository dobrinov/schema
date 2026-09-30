//! Local HTTP server backing the browser UI.
use std::io::Read;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::assets;
use crate::git;
use crate::project::{Project, BASE_FILE};
use crate::registry::{self, Instance};

pub const FIRST_PORT: u16 = 5491;

pub struct App {
    pub project: Project,
    pub port: u16,
    pub started: u64,
    /// Result of the background update check, once known.
    pub update: std::sync::Arc<std::sync::Mutex<Option<crate::update::UpdateInfo>>>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn bind(port: Option<u16>) -> Result<(Server, u16), String> {
    if let Some(p) = port {
        return Server::http(("127.0.0.1", p)).map(|s| (s, p)).map_err(|e| format!("cannot listen on port {p}: {e}"));
    }
    for p in FIRST_PORT..FIRST_PORT + 100 {
        if let Ok(s) = Server::http(("127.0.0.1", p)) {
            return Ok((s, p));
        }
    }
    Err("no free port found".into())
}

pub fn url_encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 3 <= b.len() && s.is_char_boundary(i + 3) => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(v) => {
                    out.push(v);
                    i += 2;
                }
                Err(_) => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query(url: &str) -> (String, Vec<(String, String)>) {
    match url.split_once('?') {
        Some((p, q)) => (
            p.to_string(),
            q.split('&').filter(|s| !s.is_empty()).map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                (url_decode(k), url_decode(v))
            }).collect(),
        ),
        None => (url.to_string(), vec![]),
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn respond_json(req: Request, status: u16, v: &Value) {
    let body = serde_json::to_vec(v).unwrap_or_default();
    let _ = req.respond(
        Response::from_data(body)
            .with_status_code(status)
            .with_header(header("Content-Type", "application/json; charset=utf-8"))
            .with_header(header("Cache-Control", "no-store")),
    );
}

fn respond_text(req: Request, status: u16, body: String) {
    let _ = req.respond(
        Response::from_data(body.into_bytes())
            .with_status_code(status)
            .with_header(header("Content-Type", "text/plain; charset=utf-8"))
            .with_header(header("Cache-Control", "no-store")),
    );
}

impl App {
    fn state(&self) -> Value {
        let p = &self.project;
        let repo = p.repo.as_ref();
        let fingerprint = match repo {
            Some(r) => r.fingerprint(&p.file),
            None => std::fs::metadata(&p.file).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis()).to_string(),
        };
        json!({
            "file": p.file.display().to_string(),
            "name": p.display_name(),
            "rel": p.rel,
            "is_git": repo.is_some(),
            "tracked": p.tracked,
            "repo": repo.map(|r| r.root.display().to_string()),
            "repo_name": repo.and_then(|r| r.root.file_name().map(|s| s.to_string_lossy().into_owned())),
            "branch": repo.and_then(|r| r.branch()),
            "head": repo.and_then(|r| r.head()),
            "dirty": match (repo, &p.rel) { (Some(r), Some(rel)) => r.is_dirty(rel), _ => false },
            "base_file": p.base_file.as_ref().map(|f| f.display().to_string()),
            "initial": { "base": p.cmp.base, "compare": p.cmp.compare },
            "fingerprint": fingerprint,
            "session": self.started,
            "config_path": p.config_path().display().to_string(),
            "designs_dir": p.designs_dir().display().to_string(),
            "version": env!("CARGO_PKG_VERSION"),
            "build": crate::update::BUILT_COMMIT.chars().take(7).collect::<String>(),
            "update": self.update.lock().ok().and_then(|u| u.clone()),
        })
    }

    fn handle(&self, mut req: Request) {
        // DNS-rebinding protection: only accept local Host headers.
        let host_ok = req.headers().iter().find(|h| h.field.equiv("Host")).is_none_or(|h| {
            let v = h.value.as_str();
            let host = v.rsplit_once(':').map_or(v, |(h, _)| h);
            matches!(host, "127.0.0.1" | "localhost" | "[::1]")
        });
        if !host_ok {
            return respond_text(req, 403, "forbidden".into());
        }
        let (path, q) = query(req.url());
        let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        let method = req.method().clone();
        match (method, path.as_str()) {
            (Method::Get, "/api/health") => {
                respond_json(req, 200, &json!({"ok": true, "file": self.project.file.display().to_string(), "pid": std::process::id(), "port": self.port}))
            }
            (Method::Get, "/api/state") => respond_json(req, 200, &self.state()),
            (Method::Get, "/api/source") => {
                let r = get("ref").unwrap_or_else(|| git::WORKTREE.into());
                match self.project.source(&r) {
                    Ok(s) => respond_text(req, 200, s),
                    Err(e) => respond_text(req, 404, e),
                }
            }
            (Method::Get, "/api/git/log") => {
                let limit = get("limit").and_then(|l| l.parse().ok()).unwrap_or(40usize).min(500);
                let log = match (&self.project.repo, &self.project.rel) {
                    (Some(r), Some(rel)) => r.log(rel, limit),
                    _ => vec![],
                };
                respond_json(req, 200, &serde_json::to_value(log).unwrap())
            }
            (Method::Get, "/api/git/refs") => {
                let (branches, tags) = self.project.repo.as_ref().map(|r| r.refs()).unwrap_or_default();
                respond_json(req, 200, &json!({"branches": branches, "tags": tags}))
            }
            (Method::Get, "/api/git/merge-base") => {
                let (a, b) = (get("a").unwrap_or_default(), get("b").unwrap_or_default());
                match self.project.repo.as_ref().and_then(|r| r.merge_base(&a, &b)) {
                    Some(sha) => respond_json(req, 200, &json!({"sha": sha})),
                    None => respond_json(req, 404, &json!({"error": format!("no merge base for {a} and {b}")})),
                }
            }
            (Method::Get, "/api/git/resolve") => {
                let r = get("ref").unwrap_or_default();
                let ok = r == BASE_FILE || self.project.repo.as_ref().and_then(|repo| repo.resolve(&r)).is_some();
                respond_json(req, if ok { 200 } else { 404 }, &json!({"ok": ok}))
            }
            (Method::Get, "/api/config") => respond_json(req, 200, &self.project.project_config()),
            (Method::Get, "/api/designs") => {
                let list: Vec<Value> = self
                    .project
                    .list_designs()
                    .into_iter()
                    .map(|(slug, d)| json!({"slug": slug, "name": d.name, "description": d.description, "ops": d.ops.len(), "updated": d.updated}))
                    .collect();
                respond_json(req, 200, &Value::Array(list))
            }
            (m, p) if p.starts_with("/api/designs/") => {
                let slug = p.trim_start_matches("/api/designs/").to_string();
                let valid = !slug.is_empty() && slug.len() <= 64 && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                if !valid {
                    return respond_json(req, 400, &json!({"error": "invalid design name"}));
                }
                match m {
                    Method::Get => match self.project.load_design(&slug) {
                        Ok(d) => respond_json(req, 200, &serde_json::to_value(d).unwrap()),
                        Err(e) => respond_json(req, 404, &json!({"error": e})),
                    },
                    Method::Put | Method::Post => {
                        let mut body = String::new();
                        let _ = req.as_reader().take(16 << 20).read_to_string(&mut body);
                        match serde_json::from_str::<schema_core::design::Design>(&body) {
                            Ok(d) => match self.project.save_design(&slug, &d) {
                                Ok((j, m)) => respond_json(req, 200, &json!({"ok": true, "path": j.display().to_string(), "md_path": m.display().to_string()})),
                                Err(e) => respond_json(req, 500, &json!({"error": e})),
                            },
                            Err(e) => respond_json(req, 400, &json!({"error": format!("invalid design: {e}")})),
                        }
                    }
                    Method::Delete => {
                        let dir = self.project.designs_dir();
                        let _ = std::fs::remove_file(dir.join(format!("{slug}.md")));
                        match std::fs::remove_file(dir.join(format!("{slug}.json"))) {
                            Ok(_) => respond_json(req, 200, &json!({"ok": true})),
                            Err(e) => respond_json(req, 404, &json!({"error": e.to_string()})),
                        }
                    }
                    _ => respond_text(req, 405, "method not allowed".into()),
                }
            }
            (Method::Put, "/api/config") | (Method::Post, "/api/config") => {
                let mut body = String::new();
                let _ = req.as_reader().take(4 << 20).read_to_string(&mut body);
                match serde_json::from_str::<Value>(&body) {
                    Ok(v) if v.is_object() => {
                        let path = self.project.config_path();
                        match std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap() + "\n") {
                            Ok(_) => respond_json(req, 200, &json!({"ok": true, "path": path.display().to_string()})),
                            Err(e) => respond_json(req, 500, &json!({"error": e.to_string()})),
                        }
                    }
                    _ => respond_json(req, 400, &json!({"error": "expected a JSON object"})),
                }
            }
            (Method::Post, "/api/export/html") => {
                let mut body = String::new();
                let _ = req.as_reader().take(4 << 20).read_to_string(&mut body);
                let res = (|| -> Result<String, String> {
                    let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
                    let compare = v["compare"].as_str().unwrap_or(git::WORKTREE).to_string();
                    let base = v["base"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string());
                    let mut cfg: schema_core::ViewConfig = serde_json::from_value(v["config"].clone()).map_err(|e| e.to_string())?;
                    let cur = self.project.source(&compare)?;
                    let base_sql = base.as_ref().map(|b| self.project.source(b).unwrap_or_default());
                    let title = cfg.title.clone().unwrap_or_else(|| format!("{} — schema", self.project.display_name()));
                    cfg.title.get_or_insert(title.clone());
                    let subtitle = match &base { Some(b) => format!("{b} → {compare}"), None => compare.clone() };
                    Ok(crate::export::html(&crate::export::HtmlInput { sql: &cur, base_sql: base_sql.as_deref(), cfg: &cfg, title, subtitle, static_only: false }))
                })();
                match res {
                    Ok(html) => {
                        let _ = req.respond(Response::from_data(html.into_bytes()).with_header(header("Content-Type", "text/html; charset=utf-8")));
                    }
                    Err(e) => respond_text(req, 400, e),
                }
            }
            (Method::Post, "/api/shutdown") => {
                respond_json(req, 200, &json!({"ok": true}));
                registry::unregister(self.port);
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    std::process::exit(0);
                });
            }
            (Method::Get, p) => match assets::get(p) {
                Some((bytes, ct)) => {
                    let _ = req.respond(
                        Response::from_data(bytes.into_owned())
                            .with_header(header("Content-Type", ct))
                            .with_header(header("Cache-Control", "no-cache")),
                    );
                }
                None => respond_text(req, 404, "not found".into()),
            },
            _ => respond_text(req, 405, "method not allowed".into()),
        }
    }
}

pub fn run(server: Server, app: App) {
    let server = Arc::new(server);
    let app = Arc::new(app);
    registry::register(&Instance {
        pid: std::process::id(),
        port: app.port,
        file: app.project.file.display().to_string(),
        cwd: std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
        started: app.started,
        url: format!("http://127.0.0.1:{}/", app.port),
        build: registry::build_id(),
    });
    let mut handles = Vec::new();
    for _ in 0..4 {
        let (s, a) = (server.clone(), app.clone());
        handles.push(std::thread::spawn(move || loop {
            match s.recv() {
                Ok(req) => a.handle(req),
                Err(_) => break,
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    registry::unregister(app.port);
}

pub fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).status();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", url]).status();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(url).status();
    if r.map_or(true, |s| !s.success()) {
        eprintln!("could not open a browser; visit {url}");
    }
}
