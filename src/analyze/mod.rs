//! Workspace & Module Analysis

mod backend;
pub(crate) mod externals;
mod filtering;
mod hir;
pub(crate) mod lua;
mod mod_resolver;
pub(crate) mod packages;
mod reexports;
pub(crate) mod rust;
mod syn_walker;
mod use_parser;
mod workspace;

pub use backend::AnalysisBackend;
pub use hir::FeatureConfig;
#[cfg(feature = "hir")]
pub use hir::{analyze_modules, cargo_config_with_features, load_workspace_hir};
pub(crate) use lua::Lua;
pub(crate) use rust::Rust;
pub use workspace::analyze_workspace;

use crate::model::{Analysis, LanguageProfile};

/// The analysis of one language: reads a project and delivers the packages,
/// module trees and externals the dependency graph is built from.
pub(crate) trait LanguageAdapter {
    #[allow(clippy::missing_errors_doc)]
    fn analyze(&self) -> anyhow::Result<Analysis>;

    fn profile(&self) -> &'static LanguageProfile;
}
