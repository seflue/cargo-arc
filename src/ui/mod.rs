//! The jump service: resolves jump ids to an `arc jump <line> <file>` line on
//! stdout for an editor plugin, and serves the diagram page over HTTP. Knows
//! nothing about CLI flags.

mod server;
mod service;

pub(crate) use server::serve;
pub(crate) use service::{Diagram, JumpService, Pages};
