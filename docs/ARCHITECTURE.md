# Architecture

## Project Structure

```
src/
├── analyze/
│   ├── mod.rs         # Coordination + public API
│   ├── use_parser.rs  # Use statement extraction (syn)
│   ├── hir.rs         # HIR module analysis (ra_ap_hir)
│   ├── filtering.rs   # Dependency classification + filtering
│   ├── syn_walker.rs  # Module discovery (syn + filesystem)
│   ├── backend.rs     # Analysis backend abstraction
│   └── workspace.rs   # Workspace analysis (cargo_metadata)
├── model.rs     # Shared data structures
├── graph.rs     # Dependency graph building (petgraph)
├── layout.rs    # Tree layout algorithm
├── render.rs     # SVG generation
├── volatility.rs # Git history volatility analysis
├── cli.rs        # CLI interface (clap)
├── lib.rs       # Public API exports
└── main.rs      # Entry point
```
