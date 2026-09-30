//! Startup update check: is the binary built from a commit that is behind
//! the tool's repository? Runs in the background and never blocks startup.
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;

pub const REPO_URL: &str = match option_env!("SCHEMA_REPO_URL") {
    Some(u) => u,
    None => "https://github.com/dobrinov/schema",
};
pub const BUILT_COMMIT: &str = match option_env!("SCHEMA_COMMIT") {
    Some(c) => c,
    None => "",
};
const BUILT_DIRTY_FLAG: &str = match option_env!("SCHEMA_DIRTY") {
    Some(d) => d,
    None => "0",
};
pub fn built_dirty() -> bool {
    BUILT_DIRTY_FLAG == "1"
}
pub const SRC_DIR: &str = match option_env!("SCHEMA_SRC_DIR") {
    Some(d) => d,
    None => "",
};

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub built: String,
    pub remote: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
    pub message: String,
    pub command: String,
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn git(dir: Option<&Path>, args: &[&str], timeout: Duration) -> Option<String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.arg("-C").arg(d);
    }
    cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output().ok());
    });
    let out = rx.recv_timeout(timeout).ok()??;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The update command shown to the user.
pub fn command() -> String {
    "schema update".to_string()
}

/// Compare the built commit with the repository's `main`. `None` when the
/// check could not be made (offline, no git, unknown build).
pub fn check() -> Option<UpdateInfo> {
    if BUILT_COMMIT.is_empty() || std::env::var_os("SCHEMA_NO_UPDATE_CHECK").is_some() {
        return None;
    }
    let remote_line = git(None, &["ls-remote", REPO_URL, "refs/heads/main"], Duration::from_secs(10))?;
    let remote = remote_line.split_whitespace().next()?.to_string();
    let built = BUILT_COMMIT.to_string();
    let src = Path::new(SRC_DIR);
    let mut behind = None;
    if !SRC_DIR.is_empty() && src.join(".git").exists() {
        // count with the local clone when we have one (fetch is read-only)
        if git(Some(src), &["fetch", "--quiet", "origin", "main"], Duration::from_secs(15)).is_some() {
            behind = git(Some(src), &["rev-list", "--count", &format!("{built}..origin/main")], Duration::from_secs(5)).and_then(|s| s.parse().ok());
        }
    }
    let available = match behind {
        Some(n) => n > 0,
        None => remote != built,
    };
    let message = if !available {
        format!("schema is up to date ({}{})", short(&built), if built_dirty() { " with local changes" } else { "" })
    } else {
        match behind {
            Some(n) => format!("a newer schema is available: {n} commit{} behind main (built from {}, main is at {})", if n == 1 { "" } else { "s" }, short(&built), short(&remote)),
            None => format!("schema was built from {} but main is at {}", short(&built), short(&remote)),
        }
    };
    Some(UpdateInfo { available, built, remote, behind, message, command: command() })
}

/// `schema update`: pull the source clone and reinstall.
pub fn run_update() -> Result<(), String> {
    let src = Path::new(SRC_DIR);
    if SRC_DIR.is_empty() || !src.join(".git").exists() {
        return Err(format!(
            "the source clone this binary was built from is not available.\nUpdate manually:\n  git clone {REPO_URL} schema && cd schema && make install"
        ));
    }
    println!("→ git pull --ff-only in {}", src.display());
    let st = Command::new("git").arg("-C").arg(src).args(["pull", "--ff-only", "origin", "main"]).status().map_err(|e| e.to_string())?;
    if !st.success() {
        return Err(format!("git pull failed; update manually in {}", src.display()));
    }
    println!("→ cargo install --path crates/cli --force (builds the WASM bundle too)");
    let st = Command::new("cargo").arg("install").arg("--path").arg(src.join("crates/cli")).arg("--force").status().map_err(|e| e.to_string())?;
    if !st.success() {
        return Err("cargo install failed".into());
    }
    println!("updated. Running viewers restart on their next launch.");
    Ok(())
}
