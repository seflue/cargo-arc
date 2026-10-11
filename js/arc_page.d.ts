// Global declarations for the arc page's tsc project. STATIC_DATA and its
// parts come from the Rust structs `render::static_data` serializes; the
// types in `generated/` are written by `cargo test`.

declare const STATIC_DATA: import('./generated/StaticData').StaticData;

type NodeData = import('./generated/NodeData').NodeData;
type ArcData = import('./generated/ArcData').ArcData;
type CycleArcData = import('./generated/CycleArcData').CycleArcData;
