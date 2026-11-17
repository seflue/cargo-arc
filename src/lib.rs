pub mod analyze;
pub mod cli;
pub mod diagnose;
pub mod graph;
pub(crate) mod hotspots;
mod js_bundle;
pub mod layout;
pub mod model;
pub(crate) mod query;
pub mod render;
pub mod rules;
mod ui;
pub mod volatility;

pub use cli::{ArcCommand, Cargo, Judgment, run};

/// The version of this build: the package version, plus the commit in a git
/// checkout (`<version>+<commit>`, `.dirty` with uncommitted sources).
pub(crate) const VERSION: &str = env!("CARGO_ARC_VERSION");

#[cfg(test)]
mod test_support;
