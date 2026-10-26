//! Layout IR & Algorithms

mod build;
mod jump;
mod toposort;

pub use build::{
    ClusterInfo, CycleKind, CyclicEdgeInfo, EdgeDirection, ItemKind, LayoutEdge, LayoutIR,
    LayoutItem, NodeId, SymbolLocality,
};
pub(crate) use build::{build_layout, symbol_groups};
#[allow(dead_code, unused_imports)]
// render's tests build these directly, nothing else reads them.
pub(crate) use jump::JumpTarget;
#[cfg(test)]
pub(crate) use jump::LocatedDefinition;
pub(crate) use jump::{JumpTable, Location};
pub(crate) use jump::{LocatedSource, LocationId, TargetKind};
