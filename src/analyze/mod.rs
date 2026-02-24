//! Workspace & Module Analysis

mod backend;
pub(crate) mod externals;
mod filtering;
mod hir;
mod mod_resolver;
mod reexports;
mod syn_walker;
mod use_parser;
mod workspace;

pub(crate) use crate::model::normalize_crate_name;
pub use backend::AnalysisBackend;
pub use hir::FeatureConfig;
#[cfg(feature = "hir")]
pub use hir::{analyze_modules, cargo_config_with_features, load_workspace_hir};
pub(crate) use reexports::collect_crate_reexports;
pub(crate) use syn_walker::collect_crate_exports;
pub(crate) use use_parser::ReExportMap;
pub use workspace::analyze_workspace;
