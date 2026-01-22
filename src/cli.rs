use anyhow::Result;
use clap::Parser;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::analyze::{analyze_modules, analyze_workspace, load_workspace_hir};
use crate::graph::build_graph;
use crate::layout::{build_layout, detect_cycles, topo_sort};
use crate::render::{RenderConfig, render};
use std::collections::HashSet;

#[derive(Parser)]
#[command(name = "cargo-arc", about = "Visualize workspace dependencies")]
pub struct Args {
    /// Output file (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Path to Cargo.toml (default: ./Cargo.toml)
    #[arg(short, long, default_value = "Cargo.toml")]
    pub manifest_path: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    // 1. Analyze workspace
    let crates = analyze_workspace(&args.manifest_path)?;

    // 2. Build workspace crate names set for inter-crate dependency detection
    let workspace_crates: HashSet<String> = crates.iter().map(|c| c.name.clone()).collect();

    // 3. Load rust-analyzer ONCE for the entire workspace
    let (host, vfs) = load_workspace_hir(&args.manifest_path)?;

    // 4. Analyze modules for each crate (reusing loaded workspace)
    let modules: Vec<_> = crates
        .iter()
        .filter_map(|c| analyze_modules(c, &host, &vfs, &workspace_crates).ok())
        .collect();

    // 3. Build dependency graph
    let graph = build_graph(&crates, &modules);

    // 4. Detect cycles
    let cycles = detect_cycles(&graph);

    // 5. Topological sort
    let order = topo_sort(&graph, &cycles);

    // 6. Build layout (CrateDep edges skipped when ModuleDeps exist between crates)
    let layout = build_layout(&graph, &order, &cycles);

    // 7. Render to SVG
    let svg = render(&layout, &RenderConfig::default());

    // 8. Output
    match args.output {
        Some(path) => fs::write(&path, &svg)?,
        None => io::stdout().write_all(svg.as_bytes())?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_default_args() {
        let args = Args::parse_from(["cargo-arc"]);
        assert!(args.output.is_none());
        assert_eq!(args.manifest_path, PathBuf::from("Cargo.toml"));
    }

    #[test]
    #[ignore] // Smoke test - requires rust-analyzer (~30s)
    fn test_run_with_output_file() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let args = Args {
            output: Some(temp.path().to_path_buf()),
            manifest_path: PathBuf::from("Cargo.toml"),
        };
        let result = run(args);
        assert!(result.is_ok());
        let content = std::fs::read_to_string(temp.path()).unwrap();
        assert!(content.contains("<svg"));
    }
}
