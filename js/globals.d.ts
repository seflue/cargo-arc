// Cross-file global declarations for tsc --noEmit typecheck.
// Each production JS file defines a top-level const (global-script pattern);
// tsc cannot see cross-file globals, so we declare them here.

// Module globals — typeof import preserves the inferred types from each file.
declare const ArcLogic: typeof import('./arc_logic.js').ArcLogic;
declare const AppState: typeof import('./app_state.js').AppState;
declare const DomAdapter: typeof import('./dom_adapter.js').DomAdapter;
declare const DerivedState: typeof import('./derived_state.js').DerivedState;
declare const HighlightLogic: typeof import('./highlight_logic.js').HighlightLogic;
declare const HighlightRenderer: typeof import('./highlight_renderer.js').HighlightRenderer;
declare const LayerManager: typeof import('./layer_manager.js').LayerManager;
declare const SearchLogic: typeof import('./search.js').SearchLogic;
declare const Selectors: typeof import('./selectors.js').Selectors;
declare const SidebarLogic: typeof import('./sidebar.js').SidebarLogic;
declare const StaticData: typeof import('./static_data.js').StaticData;
declare const TreeLogic: typeof import('./tree_logic.js').TreeLogic;
declare const VirtualEdgeLogic: typeof import('./virtual_edge_logic.js').VirtualEdgeLogic;
declare const TextMeasure: typeof import('./text_metrics.js').TextMeasure;

// Runtime placeholders (replaced by Rust at render time)
declare const __ROW_HEIGHT__: number;
declare const __MARGIN__: number;
declare const __TOOLBAR_HEIGHT__: number;
declare const __SIDEBAR_SHADOW_PAD__: number;

// Runtime global: pre-rendered static data from Rust
interface StaticNodeData {
  type: string;
  name: string;
  parent?: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
  hasChildren: boolean;
  nesting: number;
  version?: string;
  sccId?: number;
}
interface StaticArcData {
  from: string;
  to: string;
  context: { kind: string; subKind?: string | null; features: string[] };
  usages: {
    symbol: string;
    modulePath?: string | null;
    viaReexport?: boolean;
    locations: { file: string; line: number }[];
  }[];
  cycleIds?: number[];
  sccId?: number;
}
interface StaticCycleData {
  nodes: string[];
  arcs: string[];
  sccId: number;
}
interface StaticCutData {
  fromId: string;
  toId: string;
  breaks: number;
  refs: number;
}
interface StaticClusterData {
  crate: string;
  moduleCount: number;
  cycleCount: number;
  edges: StaticCutData[];
  toBreak: number;
}
interface StaticSymbolScope {
  scope: 'singleConsumer' | 'commonAncestor' | 'crateWide';
  module?: string;
  consumers: string[];
}
declare const STATIC_DATA: {
  nodes: Record<string, StaticNodeData>;
  arcs: Record<string, StaticArcData>;
  classes: Record<string, string>;
  cycles?: Record<string, StaticCycleData>;
  clusters?: Record<string, StaticClusterData>;
  symbolScopes?: Record<string, Record<string, StaticSymbolScope>>;
  expandLevel?: number | null;
};

// Window augmentation
interface Window {
  DEBUG_ARCS?: boolean;
}
