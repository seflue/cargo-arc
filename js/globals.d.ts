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
declare const Follow: typeof import('./follow.js').Follow;
declare const Jump: typeof import('./jump.js').Jump;
declare const JumpIcons: typeof import('./jump_icons.js').JumpIcons;
declare const JumpSymbol: typeof import('./jump_symbol.js').JumpSymbol;
declare const LayerManager: typeof import('./layer_manager.js').LayerManager;
declare const SearchLogic: typeof import('./search.js').SearchLogic;
declare const Selectors: typeof import('./selectors.js').Selectors;
declare const SidebarLogic: typeof import('./sidebar.js').SidebarLogic;
declare const StaticData: typeof import('./static_data.js').StaticData;
declare const Theme: typeof import('./theme.js').Theme;
declare const SwitchToggles: typeof import('./switch_toggles.js').SwitchToggles;
declare const TreeLogic: typeof import('./tree_logic.js').TreeLogic;
declare const VirtualEdgeLogic: typeof import('./virtual_edge_logic.js').VirtualEdgeLogic;
declare const ViewSnapshot: typeof import('./view_snapshot.js').ViewSnapshot;
declare const TextMeasure: typeof import('./text_metrics.js').TextMeasure;
declare const PageLink: typeof import('./page_link.js').PageLink;
declare const HotspotTree: typeof import('./hotspot_tree.js').HotspotTree;
declare const HotspotZoom: typeof import('./hotspot_zoom.js').HotspotZoom;
declare const HotspotLabels: typeof import('./hotspot_labels.js').HotspotLabels;
declare const HotspotHover: typeof import('./hotspot_hover.js').HotspotHover;
declare const HotspotSelection: typeof import('./hotspot_selection.js').HotspotSelection;
declare const HotspotBars: typeof import('./hotspot_bars.js').HotspotBars;
declare const HotspotJumpIcon: typeof import('./hotspot_jump_icon.js').HotspotJumpIcon;
declare const HotspotLayout: typeof import('./hotspot_layout.js').HotspotLayout;

// Runtime placeholders (replaced by Rust at render time)
declare const __ROW_HEIGHT__: number;
declare const __MARGIN__: number;
declare const __TOOLBAR_HEIGHT__: number;
declare const __SIDEBAR_SHADOW_PAD__: number;

// A node's jump target, shared by both pages' node shapes below
// (`render::static_data::TargetData`).
interface StaticTargetData {
  kind: string;
  name: string;
  jump: number;
}

// Runtime global: pre-rendered static data from Rust, arc page
// (`render::static_data::NodeData`).
interface StaticNodeData {
  type: string;
  name: string;
  // Absent for a node with no module or manifest target (an external crate
  // or section).
  file?: string;
  parent: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
  hasChildren: boolean;
  nesting: number;
  version?: string;
  sccId?: number;
  targets?: StaticTargetData[];
}

// Runtime global: pre-rendered static data from Rust, hotspot map
// (`render::hotspots::HotspotCircleData`). A container's own declaring file
// for a `Module` or `Crate`, empty for the synthetic workspace root; unlike
// the arc page's `file` it is never absent.
interface HotspotCircleData {
  kind: string;
  name: string;
  file: string;
  parent?: string;
  cx: number;
  cy: number;
  r: number;
  lines: number;
  commits: number;
  rank?: number;
  fillPercent: number;
  targets?: StaticTargetData[];
}
interface StaticArcData {
  from: string;
  to: string;
  context: { kind: string; subKind?: string | null; features: string[] };
  usages: {
    symbol: string;
    modulePath?: string | null;
    viaReexport?: boolean;
    locations: { file: string; line: number; jump?: number }[];
    definition?: { file: string; line: number; jump: number };
  }[];
  cycleIds?: number[];
  sccId?: number;
}
interface StaticCycleData {
  nodes: string[];
  arcs: string[];
  sccId: number;
}
interface StaticCycleArcData {
  fromId: string;
  toId: string;
  symbols: number;
}
interface StaticClusterData {
  crate: string;
  moduleCount: number;
  cycleCount: number;
  cycles: StaticCycleArcData[][];
}
interface StaticSymbolLocality {
  locality: 'singleConsumer' | 'commonAncestor' | 'crateWide';
  module?: string;
  consumers: string[];
}
interface StaticThemeName {
  name: string;
  label: string;
}
declare const STATIC_DATA: {
  nodes: Record<string, StaticNodeData>;
  arcs: Record<string, StaticArcData>;
  classes: Record<string, string>;
  cycles?: Record<string, StaticCycleData>;
  clusters?: Record<string, StaticClusterData>;
  symbolLocalities?: Record<string, Record<string, StaticSymbolLocality>>;
  expandLevel?: number | null;
  // Hotspot map only: the workspace's top-N leaves, rank order, as keys into `nodes`.
  hotspots?: string[];
  // Hotspot map only: the furniture measurements `HotspotLayout` builds the
  // window-sized layout from (`render::hotspots::HotspotLayoutData`).
  layout?: {
    sidebarWidth: number;
    sidebarGap: number;
    mapMargin: number;
    toolbarHeight: number;
  };
  theme: {
    shadowOpacity: string;
    light: StaticThemeName[];
    dark: StaticThemeName[];
  };
};

// Window augmentation
interface Window {
  DEBUG_ARCS?: boolean;
}
