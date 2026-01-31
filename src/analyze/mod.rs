//! Workspace & Module Analysis

mod backend;
mod filtering;
mod hir;
mod syn_walker;
mod use_parser;
mod workspace;

pub use backend::AnalysisBackend;
pub use hir::FeatureConfig;
#[cfg(feature = "hir")]
pub use hir::{analyze_modules, cargo_config_with_features, load_workspace_hir};
pub(crate) use use_parser::normalize_crate_name;
pub use workspace::analyze_workspace;
