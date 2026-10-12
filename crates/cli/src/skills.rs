//! Agent skills shipped inside the binary (`schema skills install`).
//! Installs are recorded in ~/.schema/skills.txt so `schema skills refresh`
//! (run by `schema update`) can bring every installed copy up to date.
use std::path::{Path, PathBuf};

macro_rules! skill {
    ($name:literal) => {
        ($name, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills/", $name, "/SKILL.md")))
    };
}

pub const SKILLS: &[(&str, &str)] = &[skill!("schema-view"), skill!("schema-diff"), skill!("schema-inspect"), skill!("schema-embed"), skill!("schema-design")];

pub fn run(args: &[String], global: bool) -> Result<(), String> {
    match args.first().map(|s| s.as_str()) {
        None | Some("list") => {
            for (name, body) in SKILLS {
                let desc = body.lines().find_map(|l| l.strip_prefix("description:")).unwrap_or("").trim();
                println!("{name:16} {desc}");
            }
            println!("\nInstall with: schema skills install [--global]");
            Ok(())
        }
        Some("show") => {
            let name = args.get(1).ok_or("usage: schema skills show NAME")?;
            let (_, body) = SKILLS.iter().find(|(n, _)| n == name).ok_or_else(|| format!("unknown skill {name}"))?;
            print!("{body}");
            Ok(())
        }
        Some("install") => {
            let base = if global {
                PathBuf::from(std::env::var_os("HOME").ok_or("HOME not set")?).join(".claude/skills")
            } else {
                let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
                let root = crate::git::Repo::discover(&cwd).map(|r| r.root).unwrap_or(cwd);
                root.join(".claude/skills")
            };
            let base = match args.iter().position(|a| a == "--dir") {
                Some(i) => PathBuf::from(args.get(i + 1).ok_or("--dir needs a value")?),
                None => base,
            };
            for (name, body) in SKILLS {
                let dir = base.join(name);
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                std::fs::write(dir.join("SKILL.md"), body).map_err(|e| e.to_string())?;
                println!("installed {}", dir.join("SKILL.md").display());
            }
            remember(&record_path(), &base);
            Ok(())
        }
        Some("refresh") => {
            refresh();
            Ok(())
        }
        Some(other) => Err(format!("unknown skills command {other:?} (list | show NAME | install [--global] [--dir PATH] | refresh)")),
    }
}

fn record_path() -> PathBuf {
    crate::registry::home().join("skills.txt")
}

fn recorded(record: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(record).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect()
}

fn save_record(record: &Path, bases: &[PathBuf]) {
    if let Some(dir) = record.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = bases.iter().map(|b| format!("{}\n", b.display())).collect();
    let _ = std::fs::write(record, text);
}

/// Note a directory skills were installed into (best effort).
fn remember(record: &Path, base: &Path) {
    let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let mut all = recorded(record);
    if !all.contains(&base) {
        all.push(base);
        save_record(record, &all);
    }
}

/// Does `base` hold any of our skills?
fn has_ours(base: &Path) -> bool {
    SKILLS.iter().any(|(name, _)| base.join(name).join("SKILL.md").is_file())
}

/// The usual install places: the global ~/.claude/skills and the current repo's.
fn default_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        bases.push(PathBuf::from(home).join(".claude/skills"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        let root = crate::git::Repo::discover(&cwd).map(|r| r.root).unwrap_or(cwd);
        bases.push(root.join(".claude/skills"));
    }
    bases
}

/// Install directories (global, current repo) whose skills differ from the
/// ones built into this binary, e.g. after an upgrade with `brew upgrade`.
pub fn stale() -> Vec<PathBuf> {
    default_bases()
        .into_iter()
        .filter(|b| has_ours(b) && SKILLS.iter().any(|(name, body)| std::fs::read_to_string(b.join(name).join("SKILL.md")).map_or(true, |old| old != *body)))
        .collect()
}

/// Rewrite the skills wherever they are installed: the recorded places, the
/// global ~/.claude/skills and the current repo. Skills a release added are
/// installed next to the others. Directories without our skills are forgotten.
pub fn refresh() {
    refresh_in(&record_path(), default_bases());
}

fn refresh_in(record: &Path, extra: Vec<PathBuf>) {
    let mut bases = recorded(record);
    bases.extend(extra);
    let mut seen: Vec<PathBuf> = Vec::new();
    for b in bases {
        let b = b.canonicalize().unwrap_or(b);
        if !seen.contains(&b) {
            seen.push(b);
        }
    }
    let live: Vec<PathBuf> = seen.into_iter().filter(|b| has_ours(b)).collect();
    if live.is_empty() {
        println!("no installed agent skills found (install them with: schema skills install [--global])");
        return;
    }
    for base in &live {
        let (mut changed, mut failed) = (Vec::new(), Vec::new());
        for (name, body) in SKILLS {
            let file = base.join(name).join("SKILL.md");
            if std::fs::read_to_string(&file).is_ok_and(|old| old == *body) {
                continue;
            }
            match std::fs::create_dir_all(base.join(name)).and_then(|_| std::fs::write(&file, body)) {
                Ok(()) => changed.push(*name),
                Err(e) => failed.push(format!("{name}: {e}")),
            }
        }
        if changed.is_empty() && failed.is_empty() {
            println!("skills up to date in {}", base.display());
        } else if !changed.is_empty() {
            println!("updated {} in {}", changed.join(", "), base.display());
        }
        for f in failed {
            eprintln!("could not update {f} in {}", base.display());
        }
    }
    save_record(record, &live);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_updates_installed_copies_and_adds_new_skills() {
        let tmp = std::env::temp_dir().join(format!("schema-skills-test-{}", std::process::id()));
        let record = tmp.join("home/skills.txt");
        let base = tmp.join("repo/.claude/skills");
        let gone = tmp.join("deleted/.claude/skills");
        let unrelated = tmp.join("other/.claude/skills");
        std::fs::create_dir_all(base.join("schema-view")).unwrap();
        std::fs::write(base.join("schema-view/SKILL.md"), "old").unwrap();
        std::fs::create_dir_all(&unrelated).unwrap();
        remember(&record, &base);
        remember(&record, &gone);
        refresh_in(&record, vec![unrelated.clone()]);
        for (name, body) in SKILLS {
            assert_eq!(std::fs::read_to_string(base.join(name).join("SKILL.md")).unwrap(), *body);
            assert!(!unrelated.join(name).exists(), "a directory without our skills gets none");
        }
        assert_eq!(recorded(&record), vec![base.canonicalize().unwrap()]);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
