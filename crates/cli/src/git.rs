//! Thin wrapper around the `git` CLI.
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// Pseudo refs understood everywhere a ref is accepted.
pub const WORKTREE: &str = "WORKTREE";
pub const INDEX: &str = "INDEX";

#[derive(Debug, Clone)]
pub struct Repo {
    pub root: PathBuf,
    pub git_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct Commit {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Refs are passed to git as arguments; refuse anything that could be read
/// as an option or contains whitespace / control characters.
pub fn valid_ref(r: &str) -> bool {
    !r.is_empty()
        && !r.starts_with('-')
        && r.len() < 256
        && r.chars().all(|c| c.is_ascii_alphanumeric() || "/._-~^@{}+".contains(c))
        && !r.contains("..")
}

impl Repo {
    pub fn discover(dir: &Path) -> Option<Repo> {
        let root = git(dir, &["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(root.trim());
        let git_dir = git(&root, &["rev-parse", "--absolute-git-dir"]).map(|s| PathBuf::from(s.trim())).unwrap_or_else(|| root.join(".git"));
        Some(Repo { root, git_dir })
    }

    pub fn rel(&self, file: &Path) -> Option<String> {
        let root = self.root.canonicalize().ok()?;
        let f = file.canonicalize().ok()?;
        f.strip_prefix(&root).ok().map(|p| p.to_string_lossy().replace('\\', "/"))
    }

    pub fn is_tracked(&self, rel: &str) -> bool {
        git(&self.root, &["ls-files", "--error-unmatch", "--", rel]).is_some()
    }

    pub fn has_head(&self) -> bool {
        git(&self.root, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]).is_some()
    }

    /// Working tree differs from HEAD (staged or unstaged).
    pub fn is_dirty(&self, rel: &str) -> bool {
        git(&self.root, &["status", "--porcelain", "--", rel]).is_some_and(|s| !s.trim().is_empty())
    }

    pub fn branch(&self) -> Option<String> {
        git(&self.root, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_string())
    }

    pub fn head(&self) -> Option<String> {
        git(&self.root, &["rev-parse", "--short", "HEAD"]).map(|s| s.trim().to_string())
    }

    pub fn resolve(&self, r: &str) -> Option<String> {
        if r == WORKTREE || r == INDEX {
            return Some(r.to_string());
        }
        if !valid_ref(r) {
            return None;
        }
        git(&self.root, &["rev-parse", "--verify", "--quiet", &format!("{r}^{{commit}}")]).map(|s| s.trim().to_string())
    }

    pub fn merge_base(&self, a: &str, b: &str) -> Option<String> {
        if !valid_ref(a) || !valid_ref(b) {
            return None;
        }
        git(&self.root, &["merge-base", a, b]).map(|s| s.trim().to_string())
    }

    /// File contents at a ref (`WORKTREE`, `INDEX` or any commit-ish).
    pub fn show(&self, r: &str, rel: &str) -> Result<String, String> {
        match r {
            WORKTREE => std::fs::read_to_string(self.root.join(rel)).map_err(|e| e.to_string()),
            INDEX => git(&self.root, &["show", &format!(":{rel}")]).ok_or_else(|| format!("{rel} is not in the index")),
            _ => {
                if !valid_ref(r) {
                    return Err(format!("invalid ref {r:?}"));
                }
                git(&self.root, &["show", &format!("{r}:{rel}")]).ok_or_else(|| format!("{rel} does not exist at {r}"))
            }
        }
    }

    pub fn log(&self, rel: &str, limit: usize) -> Vec<Commit> {
        let out = git(
            &self.root,
            &["log", "--follow", &format!("-n{limit}"), "--format=%H%x1f%h%x1f%s%x1f%an%x1f%aI", "--", rel],
        )
        .unwrap_or_default();
        out.lines()
            .filter_map(|l| {
                let p: Vec<&str> = l.split('\u{1f}').collect();
                (p.len() == 5).then(|| Commit {
                    sha: p[0].into(),
                    short: p[1].into(),
                    subject: p[2].into(),
                    author: p[3].into(),
                    date: p[4].into(),
                })
            })
            .collect()
    }

    pub fn refs(&self) -> (Vec<String>, Vec<String>) {
        let list = |pat: &[&str]| -> Vec<String> {
            let mut args = vec!["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)"];
            args.extend_from_slice(pat);
            git(&self.root, &args).unwrap_or_default().lines().map(|s| s.to_string()).filter(|s| !s.ends_with("/HEAD")).take(60).collect()
        };
        (list(&["refs/heads", "refs/remotes"]), list(&["refs/tags"]))
    }

    /// A cheap fingerprint that changes whenever HEAD, the index or the file change.
    pub fn fingerprint(&self, file: &Path) -> String {
        let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
        let head = std::fs::read_to_string(self.git_dir.join("HEAD")).unwrap_or_default();
        let head_ref = head.strip_prefix("ref: ").map(|r| mtime(&self.git_dir.join(r.trim()))).unwrap_or(0);
        format!("{}-{}-{}-{}", mtime(file), mtime(&self.git_dir.join("index")), head.trim().len(), head_ref.max(mtime(&self.git_dir.join("HEAD"))))
    }
}

/// Resolved base / compare pair for a comparison.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Comparison {
    pub base: Option<String>,
    pub compare: String,
}

/// Interpret positional refs:
/// - none: HEAD vs working tree when the file has changes, otherwise no diff
/// - `work` / `staged` / `unstaged`
/// - `a..b`, `a...b` (merge base), `a b`, or a single `a` (vs working tree)
pub fn parse_refs(repo: Option<&Repo>, rel: Option<&str>, refs: &[String], base: Option<&str>, compare: Option<&str>) -> Result<Comparison, String> {
    let mut c = Comparison { base: None, compare: WORKTREE.into() };
    let resolve = |r: &str| -> Result<String, String> {
        let repo = repo.ok_or("not a git repository")?;
        match r {
            "HEAD" | "head" => Ok("HEAD".into()),
            "work" | "worktree" | "WORKTREE" => Ok(WORKTREE.into()),
            "index" | "INDEX" | "staged" => Ok(INDEX.into()),
            _ => repo.resolve(r).map(|_| r.to_string()).ok_or_else(|| format!("unknown git ref {r:?}")),
        }
    };
    match refs {
        [] => {
            if let (Some(repo), Some(rel)) = (repo, rel) {
                if repo.has_head() && repo.is_tracked(rel) && repo.is_dirty(rel) {
                    c.base = Some("HEAD".into());
                }
            }
        }
        [one] => match one.as_str() {
            "work" => c.base = Some("HEAD".into()),
            "staged" => {
                c.base = Some("HEAD".into());
                c.compare = INDEX.into();
            }
            "unstaged" => c.base = Some(INDEX.into()),
            s if s.contains("...") => {
                let (a, b) = s.split_once("...").unwrap();
                let b = if b.is_empty() { "HEAD" } else { b };
                let repo = repo.ok_or("not a git repository")?;
                c.base = Some(repo.merge_base(a, b).ok_or_else(|| format!("no merge base for {s}"))?);
                c.compare = resolve(b)?;
            }
            s if s.contains("..") => {
                let (a, b) = s.split_once("..").unwrap();
                c.base = Some(resolve(a)?);
                c.compare = if b.is_empty() { "HEAD".into() } else { resolve(b)? };
            }
            s => c.base = Some(resolve(s)?),
        },
        [a, b] => {
            c.base = Some(resolve(a)?);
            c.compare = resolve(b)?;
        }
        _ => return Err("expected at most two refs".into()),
    }
    if let Some(b) = base {
        c.base = Some(resolve(b)?);
    }
    if let Some(cmp) = compare {
        c.compare = resolve(cmp)?;
    }
    Ok(c)
}
