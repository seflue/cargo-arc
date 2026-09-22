//! Hotspot map: size and volatility per file, packed for rendering.

mod lines;
mod pack;
mod tree;

pub(crate) use lines::code_lines;
pub(crate) use pack::{PackedCircle, pack};
pub(crate) use tree::{GreyCause, HotspotKind, HotspotNode, HotspotTree, build, sorted_children};
