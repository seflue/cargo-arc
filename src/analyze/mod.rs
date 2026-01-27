//! Workspace & Module Analysis

mod filtering;
mod hir;
mod use_parser;
mod workspace;

pub use hir::{FeatureConfig, analyze_modules, cargo_config_with_features, load_workspace_hir};
pub use workspace::analyze_workspace;
