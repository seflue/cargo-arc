//! Layout IR & Algorithms

mod build;
mod toposort;

pub use build::{
    ClusterInfo, CycleKind, CyclicEdgeInfo, EdgeDirection, ItemKind, LayoutEdge, LayoutIR,
    LayoutItem, NodeId, SymbolScope, build_layout,
};
