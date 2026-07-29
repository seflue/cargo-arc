//! Layout IR & Algorithms

mod build;
mod toposort;

pub use build::{
    ClusterInfo, CycleEdgeInfo, CycleKind, EdgeDirection, ItemKind, LayoutEdge, LayoutIR,
    LayoutItem, NodeId, SymbolScope, build_layout,
};
