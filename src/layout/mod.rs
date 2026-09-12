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
// render and ui consume this re-export in later phases.
pub(crate) use jump::{JumpTable, JumpTarget, LocatedSource, Location, LocationId, TargetKind};
