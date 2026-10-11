// Global declarations for the hotspot map's tsc project. STATIC_DATA and its
// parts come from the Rust structs `render::hotspots` serializes; the types
// in `generated/` are written by `cargo test`.

declare const STATIC_DATA: import('./generated/HotspotStaticData').HotspotStaticData;
