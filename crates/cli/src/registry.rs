//! Running instances are recorded in ~/.schema/instances/<port>.json so a
//! second invocation for the same file reuses the existing server.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub pid: u32,
    pub port: u16,
    pub file: String,
    pub cwd: String,
    pub started: u64,
    pub url: String,
}

pub fn home() -> PathBuf {
    
    std::env::var_os("SCHEMA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".schema")))
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".schema")))
        .unwrap_or_else(|| std::env::temp_dir().join("schema"))
}

fn dir() -> PathBuf {
    home().join("instances")
}

pub fn register(i: &Instance) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let _ = std::fs::write(d.join(format!("{}.json", i.port)), serde_json::to_string_pretty(i).unwrap());
}

pub fn unregister(port: u16) {
    let _ = std::fs::remove_file(dir().join(format!("{port}.json")));
}

/// Minimal HTTP request to a local instance.
pub fn http(port: u16, method: &str, path: &str) -> Option<String> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(400)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    let req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    let (head, body) = buf.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return None;
    }
    Some(body.to_string())
}

/// Live instances (stale registry entries are removed).
pub fn instances() -> Vec<Instance> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir()) else { return out };
    for e in rd.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(inst) = serde_json::from_str::<Instance>(&text) else {
            let _ = std::fs::remove_file(e.path());
            continue;
        };
        let alive = http(inst.port, "GET", "/api/health")
            .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok())
            .is_some_and(|v| v["file"].as_str() == Some(inst.file.as_str()));
        if alive {
            out.push(inst);
        } else {
            let _ = std::fs::remove_file(e.path());
        }
    }
    out.sort_by_key(|i| i.port);
    out
}

pub fn find(file: &str) -> Option<Instance> {
    instances().into_iter().find(|i| i.file == file)
}

pub fn stop(i: &Instance) -> bool {
    let ok = http(i.port, "POST", "/api/shutdown").is_some();
    unregister(i.port);
    ok
}
