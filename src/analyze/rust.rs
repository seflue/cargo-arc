//! The Rust language adapter: Cargo metadata plus syn, or rust-analyzer HIR.

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use super::LanguageAdapter;
use super::backend::AnalysisBackend;
use super::externals::analyze_externals;
use super::hir::FeatureConfig;
use super::packages::{ModulePathMap, PackageExportMap, WorkspacePackages};
use super::reexports::collect_crate_reexports;
use super::syn_walker::collect_crate_exports;
use super::use_parser::ReExportMap;
use super::workspace::analyze_workspace;
use crate::model::{Analysis, LanguageProfile, Wording};

pub(crate) static PROFILE: LanguageProfile = LanguageProfile {
    wording: Wording {
        package: "crate",
        packages: "crates",
        manifest: "Cargo.toml",
    },
    extensions: &["rs"],
    manifest_files: &["Cargo.toml"],
    tokei_language: tokei::LanguageType::Rust,
    analyzes_tests: true,
    analyzes_externals: true,
};

/// A Cargo workspace, read through `cargo metadata` and parsed with syn, or
/// with rust-analyzer HIR when `use_hir` is set.
pub(crate) struct Rust {
    pub manifest_path: PathBuf,
    pub feature_config: FeatureConfig,
    pub use_hir: bool,
    pub externals: bool,
    pub transitive_deps: bool,
}

impl LanguageAdapter for Rust {
    fn profile(&self) -> &'static LanguageProfile {
        &PROFILE
    }

    fn analyze(&self) -> Result<Analysis> {
        let manifest_path = self.manifest_path.as_path();
        let packages = analyze_workspace(manifest_path, &self.feature_config)?;
        tracing::debug!("phase: workspace analyzed ({} crates)", packages.len());
        let workspace_packages: WorkspacePackages =
            packages.iter().map(|krate| krate.name.clone()).collect();
        let backend = AnalysisBackend::new(manifest_path, &self.feature_config, self.use_hir)?;

        let all_module_paths: ModulePathMap = packages
            .iter()
            .map(|krate| (krate.key.clone(), backend.collect_module_paths(krate)))
            .collect();
        tracing::debug!("phase: module paths collected");

        let package_exports: PackageExportMap = packages
            .iter()
            .map(|krate| (krate.key.clone(), collect_crate_exports(krate)))
            .collect();
        tracing::debug!("phase: crate exports collected");

        let reexport_map: ReExportMap = packages
            .iter()
            .map(|krate| {
                let exports = collect_crate_reexports(
                    krate,
                    &all_module_paths,
                    &workspace_packages,
                    &package_exports,
                );
                (krate.key.clone(), exports)
            })
            .collect();
        tracing::debug!("phase: reexport map collected");

        // Run externals analysis before module analysis so crate_name_map
        // is available for use-parser resolution of external package imports.
        let (externals, crate_name_map) = if self.externals {
            use cargo_metadata::MetadataCommand;
            let metadata = MetadataCommand::new().manifest_path(manifest_path).exec()?;
            let (result, names) = analyze_externals(&metadata, self.transitive_deps);
            (Some(result), names)
        } else {
            (None, HashMap::new())
        };

        let empty_name_map = HashMap::new();
        let modules = packages
            .iter()
            .filter_map(|krate| {
                tracing::debug!("analyzing crate: {}", krate.key);
                let ext_names = crate_name_map.get(&krate.key).unwrap_or(&empty_name_map);
                match backend.analyze_modules(
                    krate,
                    &workspace_packages,
                    &all_module_paths,
                    &package_exports,
                    &reexport_map,
                    ext_names,
                ) {
                    Ok(tree) => Some(tree),
                    Err(err) => {
                        tracing::warn!("Skipping crate {}: {err}", krate.name);
                        None
                    }
                }
            })
            .collect();
        tracing::debug!("phase: all crates analyzed");

        Ok(Analysis {
            packages,
            modules,
            externals,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_profile_tells_rust_sources_and_cargo_manifests_apart() {
        assert!(PROFILE.is_source(Path::new("/ws/src/lib.rs")));
        assert!(!PROFILE.is_source(Path::new("/ws/src/lib.RS")));
        assert!(!PROFILE.is_source(Path::new("/ws/Cargo.toml")));
        assert!(PROFILE.is_manifest(Path::new("/ws/a/Cargo.toml")));
        assert!(!PROFILE.is_manifest(Path::new("/ws/arc-rules.toml")));
    }

    #[test]
    fn an_analysis_survives_a_json_round_trip() {
        let adapter = Rust {
            manifest_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
            feature_config: FeatureConfig::default(),
            use_hir: false,
            externals: true,
            transitive_deps: false,
        };
        let analysis = adapter.analyze().expect("cargo-arc analyzes");

        let json = serde_json::to_value(&analysis).unwrap();
        let back: Analysis = serde_json::from_value(json.clone()).unwrap();

        assert_eq!(serde_json::to_value(&back).unwrap(), json);
        assert!(back.packages.iter().any(|p| p.name == "cargo-arc"));
        let externals = back.externals.expect("externals were asked for");
        assert!(externals.packages.iter().any(|p| p.name == "petgraph"));
    }
}
