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
/// Set by the Release workflow (`vX.Y.Z`): this binary is a tagged release
/// (Homebrew or a downloaded archive), not a build of a source clone.
pub const RELEASE_TAG: &str = match option_env!("SCHEMA_RELEASE_TAG") {
    Some(t) => t,
    None => "",
};
pub fn is_release() -> bool {
    !RELEASE_TAG.is_empty()
}
pub const HOMEBREW_FORMULA: &str = "dobrinov/tap/schema";

/// Installed through Homebrew? (the binary lives under a Cellar)
fn via_homebrew() -> bool {
    std::env::current_exe().map(|p| p.to_string_lossy().to_lowercase()).is_ok_and(|p| p.contains("/cellar/") || p.contains("/homebrew/") || p.contains("/linuxbrew/"))
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// Newest `vX.Y.Z` tag in the repository.
fn latest_release_tag() -> Option<String> {
    let out = git(None, &["ls-remote", "--tags", "--refs", REPO_URL, "refs/tags/v*"], Duration::from_secs(10))?;
    out.lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter_map(|r| r.strip_prefix("refs/tags/"))
        .filter_map(|t| parse_version(t).map(|v| (v, t.to_string())))
        .max()
        .map(|(_, t)| t)
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub built: String,
    pub remote: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
    pub message: String,
    pub command: String,
    /// How this binary was installed: `homebrew`, `source` (a clone it was
    /// built from) or `download` (a release archive).
    pub how: &'static str,
    /// Where to get the new version by hand (`download` installs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// CHANGELOG.md sections newer than this build, newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// Source builds: subjects of the commits on main this build lacks, newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<String>,
}

/// One version's section of CHANGELOG.md, its body as Markdown.
#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub body: String,
}

/// `## [X.Y.Z] - date` sections of a changelog that are newer than `built`
/// (and no newer than `upto`, when given), plus a non-empty `[Unreleased]`
/// when `unreleased` is set.
fn changelog_notes(text: &str, built: &str, upto: Option<&str>, unreleased: bool) -> Vec<Note> {
    let mut all: Vec<(String, Option<String>, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## [") {
            if let Some((v, tail)) = rest.split_once(']') {
                let date = tail.trim().trim_start_matches('-').trim();
                all.push((v.to_string(), (!date.is_empty()).then(|| date.to_string()), Vec::new()));
                continue;
            }
        }
        // link reference definitions (`[0.2.0]: https://…`) are not content
        let is_ref = line.starts_with('[') && line.split_once("]:").is_some_and(|(k, _)| !k.contains(' '));
        if let Some(last) = all.last_mut() {
            if !is_ref {
                last.2.push(line);
            }
        }
    }
    let built = parse_version(built);
    let upto = upto.and_then(parse_version);
    all.into_iter()
        .filter(|(v, _, _)| match parse_version(v) {
            Some(n) => Some(n) > built && upto.is_none_or(|u| n <= u),
            None => unreleased && v.eq_ignore_ascii_case("unreleased"),
        })
        .map(|(version, date, body)| Note { version, date, body: body.join("\n").trim().to_string() })
        .filter(|n| !n.body.is_empty())
        .collect()
}

/// `body` without the `- ` entries (bullet plus its wrapped lines) that
/// `known` contains, and without headings left with nothing under them.
fn without_entries_in(body: &str, known: &str) -> String {
    let known: std::collections::HashSet<String> = entries(known).into_iter().map(|(_, e)| e).collect();
    let mut out: Vec<String> = Vec::new();
    let mut last: Option<String> = None;
    for (heading, entry) in entries(body) {
        if known.contains(&entry) {
            continue;
        }
        if heading.is_some() && heading != last {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(heading.clone().unwrap());
            out.push(String::new());
            last = heading;
        }
        out.push(entry);
    }
    out.join("\n")
}

/// The `- ` entries of a changelog fragment (bullet plus wrapped lines),
/// each with the `### ` heading of its section.
fn entries(text: &str) -> Vec<(Option<String>, String)> {
    let mut out: Vec<(Option<String>, String)> = Vec::new();
    let mut heading: Option<String> = None;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("### ") {
            heading = Some(format!("### {}", h.trim()));
        } else if line.starts_with("- ") {
            out.push((heading.clone(), line.trim_end().to_string()));
        } else if line.starts_with("  ") {
            if let Some((_, e)) = out.last_mut() {
                e.push('\n');
                e.push_str(line.trim_end());
            }
        }
    }
    out
}

/// CHANGELOG.md at `rev` of the GitHub repository (curl; `None` offline or off GitHub).
fn fetch_changelog(rev: &str) -> Option<String> {
    let raw = REPO_URL.strip_prefix("https://github.com/")?;
    let url = format!("https://raw.githubusercontent.com/{raw}/{rev}/CHANGELOG.md");
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "--max-time", "10", &url]);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output().ok());
    });
    let out = rx.recv_timeout(Duration::from_secs(12)).ok()??;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
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

/// How this binary was installed (see `UpdateInfo::how`).
fn how() -> &'static str {
    // a path under Homebrew alone is not enough: a source build can be
    // installed there too (`cargo install --root /opt/homebrew`)
    match (is_release(), via_homebrew()) {
        (true, true) => "homebrew",
        (true, false) => "download",
        _ => "source",
    }
}

/// The update command shown to the user: what actually updates this install.
pub fn command() -> String {
    match how() {
        "homebrew" => format!("brew upgrade {HOMEBREW_FORMULA}"),
        "download" => format!("download the new version from {REPO_URL}/releases/latest"),
        _ => "schema update".to_string(),
    }
}

/// Release builds: is there a newer `vX.Y.Z` tag than the one we were built from?
fn check_release() -> Option<UpdateInfo> {
    let built = env!("CARGO_PKG_VERSION").to_string();
    let latest = latest_release_tag()?;
    let available = parse_version(&latest) > parse_version(&built);
    let message = if available {
        format!("schema {} is available (you have {built})", latest.trim_start_matches('v'))
    } else {
        format!("schema {built} is the latest release")
    };
    let notes = if available { fetch_changelog(&latest).map(|t| changelog_notes(&t, &built, Some(&latest), false)).unwrap_or_default() } else { Vec::new() };
    let url = (how() == "download").then(|| format!("{REPO_URL}/releases/tag/{latest}"));
    Some(UpdateInfo { available, built, remote: latest, behind: None, message, command: command(), how: how(), url, notes, commits: Vec::new() })
}

/// Compare the built commit with the repository's `main` (or, for a release
/// build, the version with the newest release tag). `None` when the check
/// could not be made (offline, no git, unknown build).
pub fn check() -> Option<UpdateInfo> {
    if std::env::var_os("SCHEMA_NO_UPDATE_CHECK").is_some() {
        return None;
    }
    if is_release() {
        return check_release();
    }
    if BUILT_COMMIT.is_empty() {
        return None;
    }
    let remote_line = git(None, &["ls-remote", REPO_URL, "refs/heads/main"], Duration::from_secs(10))?;
    let remote = remote_line.split_whitespace().next()?.to_string();
    let built = BUILT_COMMIT.to_string();
    let src = Path::new(SRC_DIR);
    let mut behind = None;
    let (mut notes, mut commits) = (Vec::new(), Vec::new());
    if !SRC_DIR.is_empty() && src.join(".git").exists() {
        // count with the local clone when we have one (fetch is read-only)
        if git(Some(src), &["fetch", "--quiet", "origin", "main"], Duration::from_secs(15)).is_some() {
            let range = format!("{built}..origin/main");
            behind = git(Some(src), &["rev-list", "--count", &range], Duration::from_secs(5)).and_then(|s| s.parse().ok());
            if behind.is_some_and(|n| n > 0) {
                commits = git(Some(src), &["log", "--no-merges", "--max-count=50", "--format=%s", &range], Duration::from_secs(5))
                    .map(|s| s.lines().map(str::to_string).collect())
                    .unwrap_or_default();
                if let Some(text) = git(Some(src), &["show", "origin/main:CHANGELOG.md"], Duration::from_secs(5)) {
                    // entries the built commit's changelog already has are not new
                    let had = git(Some(src), &["show", &format!("{built}:CHANGELOG.md")], Duration::from_secs(5)).unwrap_or_default();
                    notes = changelog_notes(&text, env!("CARGO_PKG_VERSION"), None, true)
                        .into_iter()
                        .map(|n| Note { body: without_entries_in(&n.body, &had), ..n })
                        .filter(|n| !n.body.is_empty())
                        .collect();
                }
            }
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
    Some(UpdateInfo { available, built, remote, behind, message, command: command(), how: how(), url: None, notes, commits })
}

/// `schema update`: `brew upgrade` for Homebrew installs, pull + reinstall
/// for a source clone, and a pointer to the Releases page otherwise.
pub fn run_update() -> Result<(), String> {
    if how() == "homebrew" {
        println!("→ brew upgrade {HOMEBREW_FORMULA}");
        let st = Command::new("brew").args(["upgrade", HOMEBREW_FORMULA]).status().map_err(|e| format!("could not run brew: {e}"))?;
        return if st.success() { Ok(()) } else { Err("brew upgrade failed".into()) };
    }
    if is_release() {
        return match check_release() {
            Some(u) if u.available => Err(format!("{}\n{}", u.message, u.command)),
            Some(u) => {
                println!("{}", u.message);
                Ok(())
            }
            None => Err(format!("could not check for releases (offline?). Releases: {REPO_URL}/releases")),
        };
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\nintro\n\n## [Unreleased]\n\n### Added\n\n- next thing\n\n## [0.4.0] - 2026-10-02\n\n### Added\n\n- banner\n  wrapped\n\n## [0.3.0] - 2026-10-01\n\n- rails\n\n## [0.2.0] - 2026-09-30\n\n- first\n\n[0.2.0]: https://example.com\n";

    #[test]
    fn notes_between_versions() {
        let n = changelog_notes(LOG, "0.2.0", Some("v0.4.0"), false);
        assert_eq!(n.iter().map(|n| n.version.as_str()).collect::<Vec<_>>(), ["0.4.0", "0.3.0"]);
        assert_eq!(n[0].date.as_deref(), Some("2026-10-02"));
        assert_eq!(n[0].body, "### Added\n\n- banner\n  wrapped");
        assert_eq!(n[1].body, "- rails");
    }

    #[test]
    fn unreleased_entries_the_build_has_are_dropped() {
        let main = "### Added\n\n- old thing\n- new thing\n  wrapped\n\n### Fixed\n\n- old fix\n";
        let built = "## [Unreleased]\n\n### Added\n\n- old thing\n\n### Fixed\n\n- old fix\n";
        assert_eq!(without_entries_in(main, built), "### Added\n\n- new thing\n  wrapped");
        assert_eq!(without_entries_in(main, main), "");
        assert_eq!(without_entries_in("- a\n- b", ""), "- a\n- b");
    }

    #[test]
    fn unreleased_for_source_builds() {
        let n = changelog_notes(LOG, "0.3.0", None, true);
        assert_eq!(n.iter().map(|n| n.version.as_str()).collect::<Vec<_>>(), ["Unreleased", "0.4.0"]);
        assert!(changelog_notes(LOG, "0.4.0", None, false).is_empty());
    }
}
