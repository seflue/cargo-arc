// Global declarations for tsc --noEmit typecheck: names the page provides
// outside the ES modules (render-time placeholders and STATIC_DATA).

// Runtime placeholders (replaced by Rust at render time)
declare const __ROW_HEIGHT__: number;
declare const __MARGIN__: number;
declare const __TOOLBAR_HEIGHT__: number;
declare const __SIDEBAR_SHADOW_PAD__: number;
declare const __OVERLAYS__: 'svg' | 'page';

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
    sidebarMarginRight: number;
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
