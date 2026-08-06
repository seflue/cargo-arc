//! Layout IR & Algorithms

mod build;
mod toposort;

pub use build::{
    ClusterInfo, CycleKind, CyclicEdgeInfo, EdgeDirection, ItemKind, LayoutEdge, LayoutIR,
    LayoutItem, NodeId, SymbolLocality, build_layout,
};
