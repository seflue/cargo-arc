//! Workspace & Module Analysis

mod backend;
mod filtering;
mod hir;
mod reexports;
mod syn_walker;
mod use_parser;
mod workspace;

pub(crate) use crate::model::normalize_crate_name;
pub use backend::AnalysisBackend;
pub use hir::FeatureConfig;
#[cfg(feature = "hir")]
pub use hir::{analyze_modules, cargo_config_with_features, load_workspace_hir};
pub(crate) use reexports::ReExportMap;
pub(crate) use reexports::collect_crate_reexports;
pub(crate) use syn_walker::collect_crate_exports;
pub use workspace::analyze_workspace;
