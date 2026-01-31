//! Analysis backend abstraction (syn vs HIR).

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::hir::FeatureConfig;
use super::syn_walker::{analyze_modules_syn, collect_syn_module_paths};
use crate::model::{CrateInfo, ModuleTree};

#[cfg(feature = "hir")]
use {
    super::hir::{collect_hir_module_paths, find_crate_in_workspace, load_workspace_hir},
    super::use_parser::normalize_crate_name,
    ra_ap_ide as ide,
};

/// Backend for module analysis: lightweight syn-based or full HIR-based.
#[allow(clippy::large_enum_variant)]
pub enum AnalysisBackend {
    /// Fast filesystem + syn parsing (default).
    Syn { include_cfg_test: bool },
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

        let include_cfg_test = feature_config.cfg_flags.contains(&"test".to_string());
        let _ = (manifest_path, use_hir);
        Ok(Self::Syn { include_cfg_test })
    }

    /// Collect all module paths for a crate (lightweight).
    pub fn collect_module_paths(&self, crate_info: &CrateInfo) -> HashSet<String> {
        match self {
            Self::Syn { include_cfg_test } => {
                collect_syn_module_paths(&crate_info.path, &crate_info.name, *include_cfg_test)
            }
            #[cfg(feature = "hir")]
            Self::Hir { host, vfs } => {
                let Ok(krate) = find_crate_in_workspace(crate_info, host, vfs) else {
                    return HashSet::new();
                };
                let db = host.raw_database();
                let name = normalize_crate_name(&crate_info.name);
                collect_hir_module_paths(krate.root_module(db), db, &name, &name)
            }
        }
    }

    /// Full module analysis with dependency extraction.
    pub fn analyze_modules(
        &self,
        crate_info: &CrateInfo,
        workspace_crates: &HashSet<String>,
        all_module_paths: &HashMap<String, HashSet<String>>,
    ) -> Result<ModuleTree> {
        match self {
            Self::Syn { include_cfg_test } => analyze_modules_syn(
                crate_info,
                workspace_crates,
                all_module_paths,
                *include_cfg_test,
            ),
            #[cfg(feature = "hir")]
            Self::Hir { host, vfs } => super::hir::analyze_modules(
                crate_info,
                host,
                vfs,
                workspace_crates,
                all_module_paths,
            ),
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

    #[cfg(feature = "hir")]
    #[test]
    fn test_backend_syn_when_hir_not_requested() {
        let backend = AnalysisBackend::new(Path::new("."), &FeatureConfig::default(), false)
            .expect("should create backend");
        assert!(matches!(backend, AnalysisBackend::Syn { .. }));
    }
}
