//! Analysis backend abstraction (syn vs HIR).

use anyhow::Result;
use std::path::Path;

use super::hir::FeatureConfig;
use super::name_resolution::NameResolution;
use super::packages::{ModulePathMap, ModulePaths};
use super::syn_walker::{analyze_modules_syn, collect_syn_module_paths};
use crate::model::{ModuleTree, PackageInfo};
use std::collections::HashMap;

#[cfg(feature = "hir")]
use {
    super::hir::{collect_hir_module_paths, find_crate_in_workspace, load_workspace_hir},
    super::packages::normalize_crate_name,
    ra_ap_ide as ide,
};

/// Backend for module analysis: lightweight syn-based or full HIR-based.
#[allow(clippy::large_enum_variant)]
pub enum AnalysisBackend {
    /// Fast filesystem + syn parsing (default).
    Syn { include_tests: bool },
    /// Full rust-analyzer HIR (requires `--hir` flag + `feature = "hir"`).
    #[cfg(feature = "hir")]
    Hir {
        host: ide::AnalysisHost,
        vfs: ra_ap_vfs::Vfs,
    },
}

impl AnalysisBackend {
    /// Create the appropriate backend.
    /// Default: Syn. Hir only when `use_hir == true` AND `feature = "hir"` is compiled.
    #[allow(clippy::missing_errors_doc)]
    pub fn new(
        manifest_path: &Path,
        feature_config: &FeatureConfig,
        use_hir: bool,
    ) -> Result<Self> {
        #[cfg(feature = "hir")]
        if use_hir {
            let (host, vfs) = load_workspace_hir(manifest_path, feature_config)?;
            return Ok(Self::Hir { host, vfs });
        }

        let include_tests = feature_config.include_tests;
        let _ = (manifest_path, use_hir);
        Ok(Self::Syn { include_tests })
    }

    /// Collect all module paths for a crate (lightweight).
    #[must_use]
    pub fn collect_module_paths(&self, crate_info: &PackageInfo) -> ModulePaths {
        match self {
            Self::Syn { include_tests } => collect_syn_module_paths(crate_info, *include_tests),
            #[cfg(feature = "hir")]
            Self::Hir { host, vfs } => {
                let Ok(krate) = find_crate_in_workspace(crate_info, host, vfs) else {
                    return ModulePaths::default();
                };
                let db = host.raw_database();
                let name = normalize_crate_name(&crate_info.name);
                ModulePaths {
                    paths: collect_hir_module_paths(krate.root_module(db), db, &name, &name),
                    ..ModulePaths::default()
                }
            }
        }
    }

    /// The name resolution of `packages`, whose modules are `module_paths`,
    /// as this backend's walk resolves names.
    pub(crate) fn name_resolution<'m>(
        &self,
        packages: &[PackageInfo],
        module_paths: &'m ModulePathMap,
    ) -> NameResolution<'m> {
        match self {
            Self::Syn { include_tests } => {
                NameResolution::new(packages, module_paths, *include_tests)
            }
            #[cfg(feature = "hir")]
            Self::Hir { .. } => NameResolution::without_table(packages, module_paths),
        }
    }

    /// Full module analysis with dependency extraction.
    #[allow(clippy::missing_errors_doc)]
    // Only the HIR arm is fallible, so without that feature nothing can fail.
    #[cfg_attr(not(feature = "hir"), allow(clippy::unnecessary_wraps))]
    pub(crate) fn analyze_modules(
        &self,
        crate_info: &PackageInfo,
        resolution: &NameResolution,
        external_crate_names: &HashMap<String, String>,
    ) -> Result<ModuleTree> {
        match self {
            Self::Syn { .. } => Ok(analyze_modules_syn(
                crate_info,
                resolution,
                external_crate_names,
            )),
            #[cfg(feature = "hir")]
            Self::Hir { host, vfs } => {
                super::hir::analyze_modules(crate_info, host, vfs, resolution, external_crate_names)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_syn_default() {
        let backend = AnalysisBackend::new(Path::new("."), &FeatureConfig::default(), false)
            .expect("should create backend");
        assert!(matches!(backend, AnalysisBackend::Syn { .. }));
    }

    #[test]
    fn the_syn_backend_resolves_test_code_as_configured() {
        let module_paths = ModulePathMap::default();
        for include_tests in [false, true] {
            let backend = AnalysisBackend::Syn { include_tests };
            let resolution = backend.name_resolution(&[], &module_paths);
            assert_eq!(resolution.include_tests(), include_tests);
        }
    }

    #[cfg(feature = "hir")]
    #[test]
    fn test_backend_syn_when_hir_not_requested() {
        let backend = AnalysisBackend::new(Path::new("."), &FeatureConfig::default(), false)
            .expect("should create backend");
        assert!(matches!(backend, AnalysisBackend::Syn { .. }));
    }
}
