//! Bundles the page scripts in `js/` into `OUT_DIR`.
//!
//! A published crate ships the bundles in `js/dist/` (the release recipes
//! write them); a git checkout has none and builds them with Bun.

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

const ENTRIES: [&str; 2] = ["svg_script", "hotspot_script"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=js");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out_dir = Path::new(&out_dir);
    let packaged = Path::new("js/dist");

    if packaged.is_dir() {
        for entry in ENTRIES {
            let file = format!("{entry}.js");
            std::fs::copy(packaged.join(&file), out_dir.join(&file))
                .unwrap_or_else(|e| panic!("cannot copy js/dist/{file}: {e}"));
        }
        return;
    }

    let output = Command::new("bun")
        .arg("build")
        .args(ENTRIES.map(|entry| format!("js/{entry}.js")))
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
