//! The jump service: resolves jump ids to an `arc jump <line> <file>` line on
//! stdout for an editor plugin, and serves the diagram page over HTTP. Knows
//! nothing about CLI flags.

mod server;
mod service;

// nothing in the crate calls these yet.
#[allow(dead_code, unused_imports)]
pub(crate) use server::serve;
#[allow(dead_code, unused_imports)]
pub(crate) use service::JumpService;
