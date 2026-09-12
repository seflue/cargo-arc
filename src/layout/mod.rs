//! Layout IR & Algorithms

mod build;
mod jump;
mod toposort;

pub(crate) use build::build_layout;
pub use build::{
    ClusterInfo, CycleKind, CyclicEdgeInfo, EdgeDirection, ItemKind, LayoutEdge, LayoutIR,
    LayoutItem, NodeId, SymbolLocality,
};
#[allow(dead_code, unused_imports)]
// ui resolves ids through these once it exists, Phase 5.
pub(crate) use jump::{JumpTable, JumpTarget, Location};
pub(crate) use jump::{LocatedSource, LocationId, TargetKind};
