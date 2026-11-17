//! Bundles the page scripts in `js/` into `OUT_DIR` and sets
//! `CARGO_ARC_VERSION`.
//!
//! A published crate ships the bundles in `js/dist/` (the release recipes
//! write them); a git checkout has none and builds them with Bun.

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

const ENTRIES: [&str; 2] = ["svg_script", "hotspot_script"];

/// The paths that make up the binary: a change to one of them outside a
/// commit marks the build dirty.
const SOURCES: [&str; 5] = ["src", "js", "build.rs", "Cargo.toml", "Cargo.lock"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=CARGO_ARC_VERSION={}", version());

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out_dir = Path::new(&out_dir);
    let packaged = Path::new("js/dist");

    if packaged.is_dir() {
        for entry in ENTRIES {
            let file = format!("{entry}.js");
            println!("cargo:rerun-if-changed=js/dist/{file}");
            std::fs::copy(packaged.join(&file), out_dir.join(&file))
                .unwrap_or_else(|e| panic!("cannot copy js/dist/{file}: {e}"));
        }
        return;
    }

    // Watching all of js/ would rerun on every `cargo test`: the ts-rs
    // exports rewrite js/generated/ each time.
    let modules = std::fs::read_dir("js").unwrap_or_else(|e| panic!("cannot list js/: {e}"));
    for module in modules {
        let path = module
            .unwrap_or_else(|e| panic!("cannot list js/: {e}"))
            .path();
        let name = path.to_string_lossy();
        if name.ends_with(".ts") && !name.ends_with(".d.ts") {
            println!("cargo:rerun-if-changed={name}");
        }
    }

    let output = Command::new("bun")
        .arg("build")
        .args(ENTRIES.map(|entry| format!("js/{entry}.ts")))
        .arg("--format=iife")
        .arg("--outdir")
        .arg(out_dir)
        .output();
    match output {
        Ok(output) if output.status.success() => {}
        Ok(output) => panic!(
            "bun build failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(e) if e.kind() == ErrorKind::NotFound => panic!(
            "building cargo-arc from a git checkout needs Bun (https://bun.sh) \
             to bundle js/; a release from crates.io ships the bundles"
        ),
        Err(e) => panic!("cannot run bun: {e}"),
    }
}

/// The package version, with the commit as semver build metadata in a git
/// checkout: `<version>+<commit>`, or `<version>+<commit>.dirty` when a
/// source differs from that commit.
fn version() -> String {
    let package = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    // A published crate has no .git of its own; git would describe the
    // repository around it.
    if !Path::new(".git").exists() {
        return package;
    }
    let Some(commit) = git(&["rev-parse", "--short", "HEAD"]) else {
        return package;
    };
    watch_git_state();
    // The .ts files in js/ are watched where they are bundled.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    let mut status = vec!["status", "--porcelain", "--untracked-files=no", "--"];
    status.extend(SOURCES);
    let dirty = git(&status).is_some_and(|changes| !changes.is_empty());
    let suffix = if dirty { ".dirty" } else { "" };
    format!("{package}+{commit}{suffix}")
}

/// Rerun after a commit, a checkout or a staged change.
fn watch_git_state() {
    if let Some(git_dir) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
        println!("cargo:rerun-if-changed={git_dir}/index");
    }
}

/// The trimmed stdout of `git args`, or None when git fails.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
