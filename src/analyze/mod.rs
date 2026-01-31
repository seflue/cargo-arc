//! Workspace & Module Analysis

mod filtering;
mod hir;
mod syn_walker;
mod use_parser;
mod workspace;

pub use hir::{FeatureConfig, analyze_modules, cargo_config_with_features, load_workspace_hir};
pub(crate) use hir::{collect_hir_module_paths, find_crate_in_workspace};
pub(crate) use use_parser::normalize_crate_name;
pub use workspace::analyze_workspace;
