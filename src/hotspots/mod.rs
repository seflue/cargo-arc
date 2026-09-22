//! Hotspot map: size and volatility per file, packed for rendering.

mod lines;
mod pack;
mod tree;

pub use lines::code_lines;
pub use pack::{PackedCircle, pack};
pub use tree::{HotspotKind, HotspotNode, HotspotTree, build};
