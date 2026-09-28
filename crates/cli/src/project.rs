//! Resolves the schema file, git repository, comparison and view config.
use std::path::{Path, PathBuf};

use schema_core::config::merge_json;
use schema_core::ViewConfig;
use serde_json::{json, Value};

use crate::args::{detect_file, Opts};
use crate::git::{self, Comparison, Repo};

pub const CONFIG_FILE: &str = ".schema.json";
/// Pseudo ref for `--base-file`.
pub const BASE_FILE: &str = "BASEFILE";

pub struct Project {
    pub file: PathBuf,
    pub repo: Option<Repo>,
    pub rel: Option<String>,
    pub tracked: bool,
    pub cmp: Comparison,
    pub base_file: Option<PathBuf>,
}

impl Project {
    pub fn open(o: &Opts) -> Result<Project, String> {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let cwd_repo = Repo::discover(&cwd);
        let file = match &o.file {
            Some(f) => f.clone(),
            None => detect_file(cwd_repo.as_ref().map(|r| r.root.as_path()))
                .ok_or("no schema file given and none of db/structure.sql, structure.sql, schema.sql found")?,
        };
        let file = if file.is_absolute() { file } else { cwd.join(file) };
        if !file.is_file() {
            return Err(format!("{} does not exist", file.display()));
        }
        let file = file.canonicalize().map_err(|e| e.to_string())?;
        let repo = Repo::discover(file.parent().unwrap_or(Path::new(".")));
        let rel = repo.as_ref().and_then(|r| r.rel(&file));
        let tracked = match (&repo, &rel) {
            (Some(r), Some(rel)) => r.is_tracked(rel),
            _ => false,
        };
        let base_file = o.base_file.as_ref().map(|p| if p.is_absolute() { p.clone() } else { cwd.join(p) });
        let mut cmp = if base_file.is_some() {
            Comparison { base: Some(BASE_FILE.into()), compare: git::WORKTREE.into() }
        } else if repo.is_some() && (tracked || !o.refs.is_empty() || o.base.is_some()) {
            git::parse_refs(repo.as_ref(), rel.as_deref(), &o.refs, o.base.as_deref(), o.compare.as_deref())?
        } else {
            if !o.refs.is_empty() || o.base.is_some() {
                return Err(format!("{} is not tracked by git; use --base-file to compare files", file.display()));
            }
            Comparison { base: None, compare: git::WORKTREE.into() }
        };
        if let (Some(b), Some(f)) = (&cmp.base, &base_file) {
            if b == BASE_FILE && !f.is_file() {
                return Err(format!("{} does not exist", f.display()));
            }
        }
        if cmp.compare.is_empty() {
            cmp.compare = git::WORKTREE.into();
        }
        Ok(Project { file, repo, rel, tracked, cmp, base_file })
    }

    pub fn display_name(&self) -> String {
        self.rel.clone().unwrap_or_else(|| self.file.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
    }

    /// File contents at a ref. `WORKTREE` works without git.
    pub fn source(&self, r: &str) -> Result<String, String> {
        if r == git::WORKTREE {
            return std::fs::read_to_string(&self.file).map_err(|e| format!("{}: {e}", self.file.display()));
        }
        if r == BASE_FILE {
            let f = self.base_file.as_ref().ok_or("no --base-file given")?;
            return std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()));
        }
        match (&self.repo, &self.rel) {
            (Some(repo), Some(rel)) => repo.show(r, rel),
            _ => Err("not a git repository".into()),
        }
    }

    pub fn sources(&self) -> Result<(String, Option<String>), String> {
        let cur = self.source(&self.cmp.compare)?;
        let base = match &self.cmp.base {
            Some(b) => Some(self.source(b)?),
            None => None,
        };
        Ok((cur, base))
    }

    pub fn config_path(&self) -> PathBuf {
        match &self.repo {
            Some(r) => r.root.join(CONFIG_FILE),
            None => self.file.parent().unwrap_or(Path::new(".")).join(CONFIG_FILE),
        }
    }

    /// Contents of `.schema.json` (`{default: {...}, views: {name: {...}}}`).
    pub fn project_config(&self) -> Value {
        std::fs::read_to_string(self.config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}))
    }

    /// Config patch coming from the command line: named view, --config, flags.
    pub fn cli_patch(&self, o: &Opts) -> Result<Value, String> {
        let mut patch = json!({});
        if let Some(v) = &o.view {
            let pc = self.project_config();
            let view = pc.get("views").and_then(|vs| vs.get(v)).ok_or_else(|| {
                let names: Vec<String> = pc.get("views").and_then(|v| v.as_object()).map(|m| m.keys().cloned().collect()).unwrap_or_default();
                format!("no view named {v:?} in {} (available: {})", self.config_path().display(), names.join(", "))
            })?;
            merge_json(&mut patch, view);
        }
        if let Some(c) = &o.config {
            let text = if c.trim_start().starts_with('{') { c.clone() } else { std::fs::read_to_string(c).map_err(|e| format!("{c}: {e}"))? };
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("--config: {e}"))?;
            merge_json(&mut patch, &v);
        }
        merge_json(&mut patch, &o.patch);
        Ok(patch)
    }

    /// Full config: defaults ← project default ← CLI patch.
    pub fn view_config(&self, o: &Opts) -> Result<ViewConfig, String> {
        let mut v = serde_json::to_value(ViewConfig::default()).unwrap();
        if let Some(d) = self.project_config().get("default") {
            merge_json(&mut v, d);
        }
        merge_json(&mut v, &self.cli_patch(o)?);
        serde_json::from_value(v).map_err(|e| format!("invalid view config: {e}"))
    }

    pub fn describe_comparison(&self) -> String {
        let name = |r: &str| match r {
            git::WORKTREE => "working tree".to_string(),
            git::INDEX => "index".to_string(),
            BASE_FILE => self.base_file.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            other => other.to_string(),
        };
        match &self.cmp.base {
            Some(b) if b == BASE_FILE && self.cmp.compare == git::WORKTREE => format!("{} → {}", name(b), self.file.display()),
            Some(b) => format!("{} → {}", name(b), name(&self.cmp.compare)),
            None => name(&self.cmp.compare),
        }
    }
}
