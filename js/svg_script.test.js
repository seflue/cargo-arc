import { afterEach, describe, expect, test } from 'bun:test';
import { AppState } from './app_state.js';
import { ArcLogic } from './arc_logic.js';
import { DerivedState } from './derived_state.js';
import { createFakeElement, createMockDomAdapter } from './dom_adapter.js';
import { Follow } from './follow.js';
import { HighlightRenderer } from './highlight_renderer.js';
import { Jump } from './jump.js';
import { JumpIcons } from './jump_icons.js';
import { JumpSymbol } from './jump_symbol.js';
import { LayerManager } from './layer_manager.js';
import { PageLink } from './page_link.js';
import { SearchLogic } from './search.js';
import { Selectors } from './selectors.js';
import { SidebarLogic } from './sidebar.js';
import { StaticData } from './static_data.js';
import { SwitchToggles } from './switch_toggles.js';
import { Theme } from './theme.js';
import { TreeLogic } from './tree_logic.js';
import { ViewSnapshot } from './view_snapshot.js';
import { VirtualEdgeLogic } from './virtual_edge_logic.js';

// svg_script.js's browser-only init only runs once `document` exists
// (below); a `document` another test file's own fixture left on `global`
// would make this one-time load of its pure exports run that init here,
// against whatever placeholders happen to be set - so `document` is cleared
// for this one require and put back the way it was found.
const hadDocument = 'document' in global;
const savedDocument = global.document;
delete global.document;
const { jumpIdFromClick, spanCenter } = require('./svg_script.js');
if (hadDocument) global.document = savedDocument;

describe('spanCenter', () => {
  test('is the middle between the topmost top and the lowest bottom', () => {
    const rects = [
      { y: 100, height: 20 },
      { y: 40, height: 20 },
      { y: 200, height: 30 },
    ];
    expect(spanCenter(rects)).toBe((40 + 230) / 2);
  });

  test('is the center of a single rect', () => {
    expect(spanCenter([{ y: 10, height: 30 }])).toBe(25);
  });

  test('is null without rects', () => {
    expect(spanCenter([])).toBeNull();
  });
});

describe('jumpIdFromClick', () => {
  test('returns the numeric jump id from the closest row with data-jump', () => {
    const target = {
      closest: (sel) =>
        sel.includes('.sidebar-location[data-jump]')
          ? { dataset: { jump: '5' } }
          : null,
    };
    expect(jumpIdFromClick(target)).toBe(5);
  });

  test('returns the jump id of a definition chip', () => {
    const target = {
      closest: (sel) =>
        sel.includes('.sidebar-definition[data-jump]')
          ? { dataset: { jump: '9' } }
          : null,
    };
    expect(jumpIdFromClick(target)).toBe(9);
  });

  test('returns null when no ancestor row has data-jump', () => {
    const target = { closest: () => null };
    expect(jumpIdFromClick(target)).toBeNull();
  });
});

describe('ArcLogic', () => {
  describe('getArcOffset', () => {
    test('calculates correct offset for 1 row', () => {
      expect(ArcLogic.getArcOffset(1)).toBe(35); // 20 + 1*15
    });

    test('calculates correct offset for 3 rows', () => {
      expect(ArcLogic.getArcOffset(3)).toBe(65); // 20 + 3*15
    });

    test('calculates correct offset for 0 rows', () => {
      expect(ArcLogic.getArcOffset(0)).toBe(20); // 20 + 0*15
    });

    test('calculates correct offset for 10 rows', () => {
      expect(ArcLogic.getArcOffset(10)).toBe(170); // 20 + 10*15
    });
  });

  describe('calculateArcPath', () => {
    test('returns valid SVG path with M and Q commands', () => {
      const result = ArcLogic.calculateArcPath(100, 50, 100, 150, 200, 24);
      expect(result.path).toContain('M ');
      expect(result.path).toContain('Q ');
    });

    test('returns correct toX and toY coordinates', () => {
      const result = ArcLogic.calculateArcPath(100, 50, 150, 200, 200, 24);
      expect(result.toX).toBe(150);
      expect(result.toY).toBe(200);
    });

    test('calculates midY as average of fromY and toY', () => {
      const result = ArcLogic.calculateArcPath(100, 50, 100, 150, 200, 24);
      expect(result.midY).toBe(100); // (50 + 150) / 2
    });

    test('calculates ctrlX based on maxRight and arc offset', () => {
      // 150 distance, rowHeight 24 => rowsSpanned = max(1, round(150/24)) = 6
      // arcOffset = 20 + 6*15 = 110
      // ctrlX = 200 + 110 = 310
      const result = ArcLogic.calculateArcPath(100, 0, 100, 150, 200, 24);
      expect(result.ctrlX).toBe(310);
    });

    test('minimum rowsSpanned is 1', () => {
      // Even with small distance, rowsSpanned should be at least 1
      const result = ArcLogic.calculateArcPath(100, 50, 100, 55, 200, 24);
      // rowsSpanned = max(1, round(5/24)) = max(1, 0) = 1
      // arcOffset = 20 + 1*15 = 35
      // ctrlX = 200 + 35 = 235
      expect(result.ctrlX).toBe(235);
    });

    test('path format is correct quadratic bezier', () => {
      const result = ArcLogic.calculateArcPath(100, 50, 150, 200, 200, 24);
      // Path should be: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
      const pathRegex = /^M \d+,\d+ Q \d+,\d+ \d+,\d+ Q \d+,\d+ \d+,\d+$/;
      expect(result.path).toMatch(pathRegex);
    });
  });

  describe('estimatePathLength', () => {
    test('returns 100 for empty/null path', () => {
      expect(ArcLogic.estimatePathLength('')).toBe(100);
      expect(ArcLogic.estimatePathLength(null)).toBe(100);
      expect(ArcLogic.estimatePathLength(undefined)).toBe(100);
    });

    test('returns 100 for invalid path with insufficient coordinates', () => {
      expect(ArcLogic.estimatePathLength('M 0,0')).toBe(100);
      expect(ArcLogic.estimatePathLength('invalid')).toBe(100);
    });

    test('estimates length from valid S-curve path using bezier approximation', () => {
      // Path: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
      // Uses quadratic bezier approximation for accurate S-curve length
      const path = 'M 100,50 Q 300,50 300,100 Q 300,150 100,150';
      // Horizontal extent 200, vertical extent 100 → bezier length ≈ 456
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(456, 0);
    });

    test('handles large vertical distance', () => {
      // Horizontal extent 300, vertical extent 500 → bezier length ≈ 941
      const path = 'M 100,0 Q 400,0 400,250 Q 400,500 100,500';
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(941, 0);
    });

    test('handles negative coordinates', () => {
      // Same geometry as first test, just shifted → ≈ 456
      const path = 'M 100,-50 Q 300,-50 300,0 Q 300,50 100,50';
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(456, 0);
    });
  });
});

// === The page's own init: `?select=` on arrival, and its precedence over a
// restored view ===
//
// The rest of svg_script.js lives inside a browser-only IIFE
// (`if (typeof document !== 'undefined')`), never exported, so exercising it
// means building the same kind of DOM/global fixture
// hotspot_script.test.js's `buildHotspotMap` tests already use for the
// hotspot map - StaticData, AppState and the rest are the real modules, only
// the DOM and window/location globals are faked. Since the IIFE runs once at
// require() time rather than through a callable export, each test forces a
// fresh module evaluation (`require.cache` cleared first) against its own
// fixture instead of calling an exported function repeatedly.

// A crate root with one container module (`mod_a`, has children) and one
// leaf inside it (`leaf_b`) - enough to tell "a module with children" apart
// from a plain leaf. No arcs: the relations panel and highlight pipeline
// still run, just over an empty set.
function makeSvgPageStaticData() {
  return {
    theme: {
      shadowOpacity: '0.42',
      light: [{ name: 'latte', label: 'Latte' }],
      dark: [{ name: 'mocha', label: 'Mocha' }],
    },
    // The class names svg_script.js's own `C` reads; values are arbitrary
    // but must be present; several are dereferenced unconditionally at
    // module scope regardless of what the fixture below wires up.
    classes: {
      depArc: 'dep-arc',
      cycleArc: 'cycle-arc',
      virtualArc: 'virtual-arc',
      arcHitarea: 'arc-hitarea',
      arcCount: 'arc-count',
      arcCountGroup: 'arc-count-group',
      arcCountBg: 'arc-count-bg',
      collapseToggle: 'collapse-toggle',
      virtualHitarea: 'virtual-hitarea',
      virtualArrow: 'virtual-arrow',
      depArrow: 'dep-arrow',
      cycleArrow: 'cycle-arrow',
      upwardArrow: 'upward-arrow',
      hasHighlight: 'has-highlight',
      hasPinned: 'has-pinned',
      selectedCrate: 'selectedCrate',
      selectedModule: 'selectedModule',
      selectedExternal: 'selectedExternal',
      selectedExternalTransitive: 'selectedExternalTransitive',
      depNode: 'depNode',
      dependentNode: 'dependentNode',
      highlightedArc: 'highlightedArc',
      highlightedArrow: 'highlightedArrow',
      highlightedLabel: 'highlightedLabel',
      shadowPath: 'shadow-path',
      glowIncoming: 'glowIncoming',
      glowOutgoing: 'glowOutgoing',
      downward: 'downward',
      upward: 'upward',
      groupMember: 'group-member',
      cycleMember: 'cycle-member',
      checked: 'checked',
      clusterModeOn: 'cluster-mode-on',
      collapsed: 'collapsed',
      crateDepArc: 'crate-dep-arc',
      moduleDepArc: 'module-dep-arc',
      reexportArc: 'reexport-arc',
      hiddenByFilter: 'hidden-by-filter',
      hidesCycle: 'hides-cycle',
      label: 'label',
      toolbarDropdownPanel: 'toolbar-dropdown-panel',
      toolbarRoot: 'toolbar-root',
      toolbarScopeActive: 'toolbar-scope-active',
    },
    expandLevel: null,
    nodes: {
      root: {
        type: 'crate',
        name: 'root',
        file: 'Cargo.toml',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 24,
        hasChildren: true,
      },
      mod_a: {
        type: 'module',
        name: 'mod_a',
        file: 'src/mod_a/mod.rs',
        parent: 'root',
        x: 20,
        y: 50,
        width: 100,
        height: 20,
        hasChildren: true,
      },
      leaf_b: {
        type: 'module',
        name: 'leaf_b',
        file: 'src/mod_a/leaf_b.rs',
        parent: 'mod_a',
        x: 40,
        y: 80,
        width: 100,
        height: 20,
        hasChildren: false,
      },
    },
    arcs: {},
  };
}

/**
 * Builds a fresh DOM/global fixture and requires svg_script.js against it
 * (a cache-cleared, fresh module evaluation, so each test gets its own
 * closures and its own AppState). Returns the pieces a test asserts on:
 * the node rects, the sidebar's own content div, and every `window.scrollTo`
 * call the init made (`scrollToSpan`'s object-argument call is how to tell
 * apart from `applyRestoredView`'s own two-argument one).
 * @param {{ search?: string, sessionStorageView?: object }} [options]
 */
function loadSvgPage({ search = '', sessionStorageView } = {}) {
  const dom = createMockDomAdapter();

  const svg = createFakeElement('svg');
  svg.viewBox = { baseVal: { width: 1000, height: 800 } };
  svg.getBoundingClientRect = () => ({
    width: 1000,
    height: 800,
    top: 0,
    left: 0,
  });
  svg.addEventListener = () => {};
  dom._registerSelector('svg', svg);

  const nodeRects = {};
  for (const [id, node] of Object.entries(makeSvgPageStaticData().nodes)) {
    const rect = createFakeElement('rect');
    rect.setAttribute('x', String(node.x));
    rect.setAttribute('y', String(node.y));
    rect.setAttribute('width', String(node.width));
    rect.setAttribute('height', String(node.height));
    rect.addEventListener = () => {};
    nodeRects[id] = rect;
    dom._registerElement(`node-${id}`, rect);
  }

  const sidebarContent = createFakeElement('div');
  sidebarContent.classList.add('sidebar-root');
  // A real element starts with an empty innerHTML; the fake one leaves the
  // property unset until first written.
  sidebarContent.innerHTML = '';
  const sidebarEl = createFakeElement('div');
  sidebarEl.addEventListener = () => {};
  sidebarEl.querySelector = (sel) =>
    sel === '.sidebar-root' ? sidebarContent : null;
  dom._registerElement('relation-sidebar', sidebarEl);

  const scrollCalls = [];

  global.DomAdapter = dom;
  global.document = {
    documentElement: { dataset: {}, scrollHeight: 2000 },
    createElement: (tag) => createFakeElement(tag),
  };
  global.window = {
    matchMedia: () => ({ matches: false, addEventListener() {} }),
    addEventListener: () => {},
    innerWidth: 1000,
    innerHeight: 800,
    scrollX: 0,
    scrollY: 0,
    scrollTo: (...args) => scrollCalls.push(args),
  };
  global.location = { search };
  global.sessionStorage = sessionStorageView
    ? {
        getItem: (key) =>
          key === 'cargo-arc-view' ? JSON.stringify(sessionStorageView) : null,
        removeItem: () => {},
        setItem: () => {},
      }
    : { getItem: () => null, removeItem: () => {}, setItem: () => {} };
  global.requestAnimationFrame = () => 0;
  global.cancelAnimationFrame = () => {};
  global.fetch = () => Promise.resolve({ ok: true });

  global.STATIC_DATA = makeSvgPageStaticData();
  global.ArcLogic = ArcLogic;
  global.StaticData = StaticData;
  global.AppState = AppState;
  global.Selectors = Selectors;
  global.LayerManager = LayerManager;
  global.TreeLogic = TreeLogic;
  global.DerivedState = DerivedState;
  global.HighlightRenderer = HighlightRenderer;
  global.VirtualEdgeLogic = VirtualEdgeLogic;
  global.SidebarLogic = SidebarLogic;
  global.SearchLogic = SearchLogic;
  global.Jump = Jump;
  global.JumpIcons = JumpIcons;
  global.JumpSymbol = JumpSymbol;
  global.Follow = Follow;
  global.Theme = Theme;
  global.SwitchToggles = SwitchToggles;
  global.ViewSnapshot = ViewSnapshot;
  global.PageLink = PageLink;

  // svg_script.js's own runtime placeholders, normally substituted by
  // render.rs before the page ships.
  global.__ROW_HEIGHT__ = 24;
  global.__MARGIN__ = 20;
  global.__TOOLBAR_HEIGHT__ = 40;

  delete require.cache[require.resolve('./svg_script.js')];
  require('./svg_script.js');

  return { nodeRects, sidebarContent, scrollCalls };
}

describe("`?select=` on arrival (svg_script.js's init)", () => {
  afterEach(() => {
    // svg_script.js's own init assigns this on the real, shared SidebarLogic
    // module object, so sidebar.js's own default has to be put back for
    // whichever test file runs next.
    SidebarLogic._onBadgeClick = null;
  });

  test('selects a module node that has children, not only a leaf, and scrolls to it', () => {
    const { sidebarContent, scrollCalls } = loadSvgPage({
      search: `?select=${encodeURIComponent('src/mod_a/mod.rs')}`,
    });

    expect(sidebarContent.innerHTML).toContain('data-node-id="mod_a"');
    // scrollToSpan's own call shape - an object with `top`/`behavior`, not
    // the two plain arguments `applyRestoredView` (or a real scrollTo) uses.
    expect(
      scrollCalls.some(
        (args) => args.length === 1 && typeof args[0] === 'object',
      ),
    ).toBe(true);
  });

  test('applies on top of a restored view left over from an abandoned recompute, not only without one', () => {
    // A view stored before a recompute's reload, selecting `mod_a` - still
    // in sessionStorage because the user left for the hotspot map instead
    // of waiting for the reload (svg_script.js's own comment on this).
    const storedView = {
      collapsed: [],
      selection: { type: 'node', key: 'root/mod_a' },
      checks: {},
      search: { query: '', scope: 'all' },
      follow: true,
      scroll: { x: 0, y: 123 },
    };
    const { sidebarContent, scrollCalls } = loadSvgPage({
      search: `?select=${encodeURIComponent('src/mod_a/leaf_b.rs')}`,
      sessionStorageView: storedView,
    });

    // The restored view's own selection did apply (proof it wasn't just
    // skipped outright)...
    expect(
      scrollCalls.some(
        (args) => args.length === 2 && args[0] === 0 && args[1] === 123,
      ),
    ).toBe(true);
    // ...but `?select=`, arriving after it, is the newer and more specific
    // signal and wins: the sidebar's final content is the leaf's, not the
    // restored view's own `mod_a`.
    expect(sidebarContent.innerHTML).toContain('data-node-id="leaf_b"');
    expect(sidebarContent.innerHTML).not.toContain('data-node-id="mod_a"');
  });

  test('without a `?select=` param, no selection or scroll happens on arrival', () => {
    const { sidebarContent, scrollCalls } = loadSvgPage({ search: '' });

    expect(sidebarContent.innerHTML).toBe('');
    expect(scrollCalls.length).toBe(0);
  });
});
