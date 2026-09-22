pub mod analyze;
pub mod cli;
pub mod diagnose;
pub mod graph;
pub(crate) mod hotspots;
mod js_registry;
pub mod layout;
pub mod model;
pub mod render;
pub mod rules;
mod ui;
pub mod volatility;

pub use cli::{ArcCommand, Cargo, Judgment, run};

#[cfg(test)]
mod test_support;
