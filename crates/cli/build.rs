//! Builds the WASM bundle (web/pkg) with wasm-pack when it is missing or
//! older than the Rust sources, so `cargo build` / `cargo install` just work.
//! Set SCHEMA_SKIP_WASM=1 to skip (the bundle must then already exist).
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

fn newest(dir: &Path) -> SystemTime {
    let mut t = SystemTime::UNIX_EPOCH;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let m = if p.is_dir() { newest(&p) } else { e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH) };
            t = t.max(m);
        }
    }
    t
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..").canonicalize().unwrap();
    let pkg = root.join("web/pkg");
    let wasm = pkg.join("schema_wasm_bg.wasm");
    for p in ["web/index.html", "web/app.js", "web/app.css", "web/viewer.js", "web/embed.js", "web/embed.css", "web/favicon.svg", "crates/core/src", "crates/wasm/src", "skills", "skills/schema-design"] {
        println!("cargo:rerun-if-changed={}", root.join(p).display());
    }
    println!("cargo:rerun-if-changed={}", wasm.display());
    println!("cargo:rerun-if-env-changed=SCHEMA_SKIP_WASM");

    // which commit this binary comes from, for the startup update check
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").arg("-C").arg(&root).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    if let Some(commit) = git(&["rev-parse", "HEAD"]) {
        println!("cargo:rustc-env=SCHEMA_COMMIT={commit}");
        let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).map(|s| !s.is_empty()).unwrap_or(false);
        println!("cargo:rustc-env=SCHEMA_DIRTY={}", if dirty { 1 } else { 0 });
        println!("cargo:rerun-if-changed={}", root.join(".git/HEAD").display());
        println!("cargo:rerun-if-changed={}", root.join(".git/refs/heads").display());
    }
    println!("cargo:rustc-env=SCHEMA_SRC_DIR={}", root.display());
    // set by the Release workflow: this binary is a tagged release, not a dev build
    println!("cargo:rerun-if-env-changed=SCHEMA_RELEASE_TAG");
    if let Ok(tag) = std::env::var("SCHEMA_RELEASE_TAG") {
        println!("cargo:rustc-env=SCHEMA_RELEASE_TAG={tag}");
    }
    if let Some(url) = git(&["remote", "get-url", "origin"]) {
        let https = url.replacen("git@github.com:", "https://github.com/", 1).trim_end_matches(".git").to_string();
        println!("cargo:rustc-env=SCHEMA_REPO_URL={https}");
    }

    let src_time = newest(&root.join("crates/core/src")).max(newest(&root.join("crates/wasm/src")));
    let wasm_time = std::fs::metadata(&wasm).and_then(|m| m.modified()).ok();
    let stale = wasm_time.is_none_or(|t| t < src_time);
    if !stale || std::env::var_os("SCHEMA_SKIP_WASM").is_some() {
        if !wasm.exists() {
            panic!("web/pkg is missing; run `make wasm` (wasm-pack build crates/wasm --target no-modules --out-dir ../../web/pkg)");
        }
        return;
    }

    // Run wasm-pack in a clean environment so the outer cargo's settings
    // (target dir lock, rustflags, …) don't leak into the nested build.
    let cargo_bin = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .map(|p| p.join("bin"));
    let wasm_pack = cargo_bin.as_ref().map(|b| b.join("wasm-pack")).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("wasm-pack"));
    let mut cmd = Command::new(&wasm_pack);
    cmd.env_clear();
    for k in ["PATH", "HOME", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "TMPDIR", "USER"] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    // Prefer the rustup toolchain (it has the wasm32 target) over e.g. a
    // Homebrew rustc that may shadow it on PATH.
    if let Some(rustc) = Command::new("rustup").args(["which", "rustc"]).output().ok().filter(|o| o.status.success()) {
        let rustc = PathBuf::from(String::from_utf8_lossy(&rustc.stdout).trim());
        if let Some(bin) = rustc.parent() {
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut paths = vec![bin.to_path_buf()];
            paths.extend(cargo_bin.clone());
            paths.extend(std::env::split_paths(&path));
            cmd.env("PATH", std::env::join_paths(paths).unwrap());
            // some macOS toolchains ship a rust-lld that cannot find libLLVM
            if let Some(tc) = bin.parent() {
                cmd.env("DYLD_FALLBACK_LIBRARY_PATH", tc.join("lib"));
            }
        }
    }
    cmd.env("CARGO_TARGET_DIR", root.join("target/wasm"));
    cmd.args(["build", "--release", "--target", "no-modules", "--no-typescript", "--no-pack", "--out-dir"])
        .arg(&pkg)
        .arg(root.join("crates/wasm"));
    match cmd.status() {
        Ok(s) if s.success() => {}
        other => {
            if wasm.exists() {
                println!("cargo:warning=wasm-pack failed ({other:?}); using the existing web/pkg bundle");
            } else {
                panic!("building the WASM bundle failed ({other:?}). Install wasm-pack (`cargo install wasm-pack`) and the wasm32-unknown-unknown target (`rustup target add wasm32-unknown-unknown`).");
            }
        }
    }
}
