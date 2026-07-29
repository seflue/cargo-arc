//! Test-only constructors for model types.

use crate::model::CrateInfo;
use std::path::PathBuf;

/// A crate laid out by Cargo's default convention, with its roots probed from
/// disk. Hand-built fixtures carry no manifest, so there is no cargo metadata
/// to resolve targets from.
pub(crate) fn conventional_crate(name: &str, path: impl Into<PathBuf>) -> CrateInfo {
    let path = path.into();
    let existing = |p: PathBuf| p.exists().then_some(p);
    CrateInfo {
        name: name.to_string(),
        lib_root: existing(path.join("src/lib.rs")),
        bin_roots: existing(path.join("src/main.rs")).into_iter().collect(),
        path,
        dependencies: Vec::new(),
        dev_dependencies: Vec::new(),
    }
}
