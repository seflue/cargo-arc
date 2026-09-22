pub mod analyze;
pub mod cli;
pub mod diagnose;
pub mod graph;
pub mod hotspots;
pub mod layout;
pub mod model;
pub mod render;
pub mod rules;
mod ui;
pub mod volatility;

pub use cli::{ArcCommand, Cargo, Judgment, run};

#[cfg(test)]
mod js_registry;
#[cfg(test)]
mod test_support;
