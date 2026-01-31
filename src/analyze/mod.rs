//! Workspace & Module Analysis

mod filtering;
mod hir;
#[allow(dead_code)] // Phase 2: API ready, call sites come in Phase 3
mod syn_walker;
mod use_parser;
mod workspace;

pub use hir::{FeatureConfig, analyze_modules, cargo_config_with_features, load_workspace_hir};
pub(crate) use hir::{collect_hir_module_paths, find_crate_in_workspace};
#[allow(unused_imports)] // Phase 2: API ready, call sites come in Phase 3
pub(crate) use syn_walker::{analyze_modules_syn, collect_syn_module_paths};
pub(crate) use use_parser::normalize_crate_name;
pub use workspace::analyze_workspace;
