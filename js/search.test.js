import { beforeEach, describe, expect, test } from 'bun:test';

// Minimal STATIC_DATA.classes mock
if (!globalThis.STATIC_DATA) globalThis.STATIC_DATA = {};
if (!globalThis.STATIC_DATA.classes) globalThis.STATIC_DATA.classes = {};
Object.assign(globalThis.STATIC_DATA.classes, {
  crateNode: 'crate',
  module: 'module',
  searchMatch: 'search-match',
  searchMatchParent: 'search-match-parent',
  label: 'label',
  toolbarScopeActive: 'scope-active',
  depArc: 'dep-arc',
  cycleArc: 'cycle-arc',
  virtualArc: 'virtual-arc',
  depArrow: 'dep-arrow',
  upwardArrow: 'upward-arrow',
  cycleArrow: 'cycle-arrow',
  virtualArrow: 'virtual-arrow',
  arcCount: 'arc-count',
  arcCountBg: 'arc-count-bg',
  externalSection: 'external-section',
  externalCrate: 'external',
  externalTransitive: 'external-transitive',
  searchActive: 'search-active',
  treeLine: 'tree-line',
});

// Minimal DomAdapter mock
globalThis.DomAdapter = {
  querySelector: () => null,
  querySelectorAll: () => [],
  getElementById: () => null,
  getSvgRoot: () => null,
  getNode: () => null,
  getVisibleArc: () => null,
  getVisibleArrows: () => [],
  getLabelGroup: () => null,
};

// Minimal StaticData mock
globalThis.StaticData = {
  getAllNodeIds: () => [],
  getNode: () => null,
  getAllArcIds: () => [],
  getArc: () => null,
};

// Minimal AppState mock
globalThis.AppState = {
  isCollapsed: () => false,
};

const { SearchLogic } = require('./search.js');

describe('SearchLogic', () => {
  beforeEach(() => {
    // Reset internal state between tests
    SearchLogic.clearSearch();
  });

  describe('refresh', () => {
    test('does nothing when search is not active', () => {
      let executeCalled = false;
      const origExecute = SearchLogic.executeSearch;
      SearchLogic.executeSearch = () => {
        executeCalled = true;
        return 0;
      };

      SearchLogic.refresh();
      expect(executeCalled).toBe(false);

      SearchLogic.executeSearch = origExecute;
    });

    test('re-executes search when active with query', () => {
      let capturedArgs = null;
      const origExecute = SearchLogic.executeSearch;

      // First call sets up state, then we intercept subsequent calls
      SearchLogic.executeSearch = function (query, scope) {
        // Use original to set up active state
        return origExecute.call(this, query, scope);
      };

      // Set up nodes for a match
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      StaticData.getAllNodeIds = () => ['crate-1'];
      StaticData.getNode = (id) =>
        id === 'crate-1'
          ? { name: 'my-crate', type: 'crate', parent: null }
          : null;

      // Execute a real search to set active state
      SearchLogic.executeSearch('my-crate', 'all');
      expect(SearchLogic.isActive()).toBe(true);

      // Now intercept to verify refresh calls executeSearch
      SearchLogic.executeSearch = (query, scope) => {
        capturedArgs = { query, scope };
        return 0;
      };

      SearchLogic.refresh();
      expect(capturedArgs).toEqual({ query: 'my-crate', scope: 'all' });

      // Restore
      SearchLogic.executeSearch = origExecute;
      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
    });
  });

  describe('isActive', () => {
    test('returns false initially', () => {
      expect(SearchLogic.isActive()).toBe(false);
    });

    test('returns true after executeSearch', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      StaticData.getAllNodeIds = () => ['n1'];
      const origGetNode = StaticData.getNode;
      StaticData.getNode = () => ({ name: 'foo', type: 'crate', parent: null });

      SearchLogic.executeSearch('foo', 'all');
      expect(SearchLogic.isActive()).toBe(true);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
    });

    test('returns false after clearSearch', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      StaticData.getAllNodeIds = () => ['n1'];
      const origGetNode = StaticData.getNode;
      StaticData.getNode = () => ({ name: 'foo', type: 'crate', parent: null });

      SearchLogic.executeSearch('foo', 'all');
      SearchLogic.clearSearch();
      expect(SearchLogic.isActive()).toBe(false);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
    });
  });

  describe('executeSearch', () => {
    test('clears and returns 0 for empty query', () => {
      const result = SearchLogic.executeSearch('', 'all');
      expect(result).toBe(0);
      expect(SearchLogic.isActive()).toBe(false);
    });

    test('clears and returns 0 for whitespace-only query', () => {
      const result = SearchLogic.executeSearch('   ', 'all');
      expect(result).toBe(0);
    });

    test('matches nodes by name substring', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      StaticData.getAllNodeIds = () => ['a', 'b', 'c'];
      StaticData.getNode = (id) =>
        ({
          a: { name: 'auth-service', type: 'crate', parent: null },
          b: { name: 'core', type: 'module', parent: null },
          c: { name: 'auth-utils', type: 'crate', parent: null },
        })[id];

      const result = SearchLogic.executeSearch('auth', 'all');
      expect(result).toBe(2);
      expect(SearchLogic.getMatchedNodeIds().has('a')).toBe(true);
      expect(SearchLogic.getMatchedNodeIds().has('c')).toBe(true);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
    });

    test('diff-based update skips stable matches during incremental typing', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      const origDomGetNode = DomAdapter.getNode;

      StaticData.getAllNodeIds = () => ['a', 'b', 'c'];
      StaticData.getNode = (id) =>
        ({
          a: { name: 'auth-service', type: 'crate', parent: null },
          b: { name: 'auth-utils', type: 'crate', parent: null },
          c: { name: 'database', type: 'crate', parent: null },
        })[id];

      // Tracking fake elements: count classList.add/remove calls
      function trackingElement() {
        const classes = new Set();
        const counts = { add: 0, remove: 0 };
        const label = {
          classList: {
            contains: () => true,
            add: () => counts.add++,
            remove: () => counts.remove++,
            toggle(_c, force) {
              if (force) counts.add++;
              else counts.remove++;
            },
          },
        };
        return {
          classList: {
            add(c) {
              counts.add++;
              classes.add(c);
            },
            remove(c) {
              counts.remove++;
              classes.delete(c);
            },
            toggle(c, force) {
              if (force) {
                counts.add++;
                classes.add(c);
              } else {
                counts.remove++;
                classes.delete(c);
              }
            },
            contains(c) {
              return classes.has(c);
            },
          },
          nextElementSibling: label,
          _counts: counts,
          _resetCounts() {
            counts.add = 0;
            counts.remove = 0;
          },
        };
      }

      const elements = {
        a: trackingElement(),
        b: trackingElement(),
        c: trackingElement(),
      };
      DomAdapter.getNode = (id) => elements[id] ?? null;

      // First search: "a" matches all three (auth-service, auth-utils, database)
      SearchLogic.executeSearch('a', 'all');
      expect(SearchLogic.getMatchedNodeIds().size).toBe(3);

      // Reset counters after initial apply
      elements.a._resetCounts();
      elements.b._resetCounts();
      elements.c._resetCounts();

      // Incremental search: "auth" narrows to a + b, drops c
      SearchLogic.executeSearch('auth', 'all');
      expect(SearchLogic.getMatchedNodeIds().size).toBe(2);

      // Stable matches (a, b) should have zero DOM operations
      expect(elements.a._counts.add).toBe(0);
      expect(elements.a._counts.remove).toBe(0);
      expect(elements.b._counts.add).toBe(0);
      expect(elements.b._counts.remove).toBe(0);

      // Dropped match (c): search-match removed from rect + label
      expect(elements.c._counts.remove).toBeGreaterThan(0);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
      DomAdapter.getNode = origDomGetNode;
    });

    test('respects scope filter for crate', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      StaticData.getAllNodeIds = () => ['a', 'b'];
      StaticData.getNode = (id) =>
        ({
          a: { name: 'foo', type: 'crate', parent: null },
          b: { name: 'foo-mod', type: 'module', parent: null },
        })[id];

      const result = SearchLogic.executeSearch('foo', 'crate');
      expect(result).toBe(1);
      expect(SearchLogic.getMatchedNodeIds().has('a')).toBe(true);
      expect(SearchLogic.getMatchedNodeIds().has('b')).toBe(false);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
    });

    test('external crates match in scope all', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      StaticData.getAllNodeIds = () => ['int-1', 'ext-1', 'ext-2'];
      StaticData.getNode = (id) =>
        ({
          'int-1': { name: 'my-app', type: 'crate', parent: null },
          'ext-1': { name: 'serde', type: 'external', parent: null },
          'ext-2': { name: 'tokio', type: 'external-transitive', parent: null },
        })[id];

      const result = SearchLogic.executeSearch('serde', 'all');
      expect(result).toBe(1);
      expect(SearchLogic.getMatchedNodeIds().has('ext-1')).toBe(true);
      expect(SearchLogic.getMatchedNodeIds().has('int-1')).toBe(false);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
    });

    test('search-active class is set on SVG root when search is active', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      const origGetSvgRoot = DomAdapter.getSvgRoot;
      const origDomGetNode = DomAdapter.getNode;

      StaticData.getAllNodeIds = () => ['int-1', 'ext-1'];
      StaticData.getNode = (id) =>
        ({
          'int-1': { name: 'my-app', type: 'crate', parent: null },
          'ext-1': { name: 'serde', type: 'external', parent: null },
        })[id];

      const svgClasses = new Set();
      DomAdapter.getSvgRoot = () => ({
        classList: {
          add: (c) => svgClasses.add(c),
          remove: (c) => svgClasses.delete(c),
        },
      });

      const matchLabel = {
        classList: {
          contains: () => true,
          add: () => {},
          remove: () => {},
          toggle: () => {},
        },
      };
      DomAdapter.getNode = (id) =>
        id === 'int-1'
          ? {
              classList: {
                add: () => {},
                remove: () => {},
                toggle: () => {},
                contains: () => false,
              },
              nextElementSibling: matchLabel,
            }
          : null;

      SearchLogic.executeSearch('my-app', 'all');
      expect(svgClasses.has('search-active')).toBe(true);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
      DomAdapter.getSvgRoot = origGetSvgRoot;
      DomAdapter.getNode = origDomGetNode;
    });

    test('search-active class removed from SVG root on clearSearch', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      const origGetSvgRoot = DomAdapter.getSvgRoot;
      const origDomGetNode = DomAdapter.getNode;

      StaticData.getAllNodeIds = () => ['n1'];
      StaticData.getNode = (id) =>
        id === 'n1' ? { name: 'my-crate', type: 'crate', parent: null } : null;

      const svgClasses = new Set();
      DomAdapter.getSvgRoot = () => ({
        classList: {
          add: (c) => svgClasses.add(c),
          remove: (c) => svgClasses.delete(c),
        },
      });

      const matchLabel = {
        classList: {
          contains: () => true,
          add: () => {},
          remove: () => {},
          toggle: () => {},
        },
      };
      DomAdapter.getNode = (id) =>
        id === 'n1'
          ? {
              classList: {
                add: () => {},
                remove: () => {},
                toggle: () => {},
                contains: () => false,
              },
              nextElementSibling: matchLabel,
            }
          : null;

      SearchLogic.executeSearch('my-crate', 'all');
      expect(svgClasses.has('search-active')).toBe(true);

      SearchLogic.clearSearch();
      expect(svgClasses.has('search-active')).toBe(false);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
      DomAdapter.getSvgRoot = origGetSvgRoot;
      DomAdapter.getNode = origDomGetNode;
    });

    test('matching external crate gets search-match class', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      const origGetSvgRoot = DomAdapter.getSvgRoot;
      const origDomGetNode = DomAdapter.getNode;

      StaticData.getAllNodeIds = () => ['ext-1'];
      StaticData.getNode = (id) =>
        id === 'ext-1'
          ? { name: 'serde', type: 'external', parent: null }
          : null;

      const classes = new Set();
      const extRect = {
        classList: {
          add: (c) => classes.add(c),
          remove: (c) => classes.delete(c),
          toggle: (c, force) => {
            if (force) classes.add(c);
            else classes.delete(c);
          },
          contains: (c) => classes.has(c),
        },
      };

      DomAdapter.getSvgRoot = () => ({
        classList: { add: () => {}, remove: () => {}, toggle: () => {} },
      });

      const matchLabel = {
        classList: {
          contains: () => true,
          add: () => {},
          remove: () => {},
          toggle: () => {},
        },
      };
      DomAdapter.getNode = (id) =>
        id === 'ext-1'
          ? { classList: extRect.classList, nextElementSibling: matchLabel }
          : null;

      SearchLogic.executeSearch('serde', 'all');
      expect(classes.has('search-match')).toBe(true);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
      DomAdapter.getSvgRoot = origGetSvgRoot;
      DomAdapter.getNode = origDomGetNode;
    });
  });

  describe('arc symbol dimming', () => {
    test('symbol match adds search-match to arc, arrows, and label children', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      const origGetAllArcIds = StaticData.getAllArcIds;
      const origGetArc = StaticData.getArc;
      const origGetSvgRoot = DomAdapter.getSvgRoot;
      const origDomGetNode = DomAdapter.getNode;
      const origGetVisibleArc = DomAdapter.getVisibleArc;
      const origGetVisibleArrows = DomAdapter.getVisibleArrows;
      const origGetLabelGroup = DomAdapter.getLabelGroup;

      StaticData.getAllNodeIds = () => ['n1', 'n2'];
      StaticData.getNode = (id) =>
        ({
          n1: { name: 'my-crate', type: 'crate', parent: null },
          n2: { name: 'other', type: 'crate', parent: null },
        })[id];
      StaticData.getAllArcIds = () => ['arc-1'];
      StaticData.getArc = (id) =>
        id === 'arc-1'
          ? { from: 'n1', to: 'n2', usages: [{ symbol: 'HashMap' }] }
          : null;

      const arcClasses = new Set();
      const arrowClasses = new Set();
      const labelChildClasses = new Set();

      const arcEl = {
        classList: {
          add: (c) => arcClasses.add(c),
          remove: (c) => arcClasses.delete(c),
        },
      };
      const arrowEl = {
        classList: {
          add: (c) => arrowClasses.add(c),
          remove: (c) => arrowClasses.delete(c),
        },
      };
      const labelChild = {
        classList: {
          add: (c) => labelChildClasses.add(c),
          remove: (c) => labelChildClasses.delete(c),
        },
      };

      DomAdapter.getSvgRoot = () => ({
        classList: { add: () => {}, remove: () => {}, toggle: () => {} },
      });
      DomAdapter.getNode = (id) =>
        ['n1', 'n2'].includes(id)
          ? {
              classList: { add: () => {}, remove: () => {}, toggle: () => {} },
              nextElementSibling: {
                classList: {
                  contains: () => true,
                  add: () => {},
                  remove: () => {},
                  toggle: () => {},
                },
              },
            }
          : null;
      DomAdapter.getVisibleArc = (id) => (id === 'arc-1' ? arcEl : null);
      DomAdapter.getVisibleArrows = (id) => (id === 'arc-1' ? [arrowEl] : []);
      DomAdapter.getLabelGroup = (id) =>
        id === 'arc-1' ? { children: [labelChild] } : null;

      SearchLogic.executeSearch('hashmap', 'all');

      expect(arcClasses.has('search-match')).toBe(true);
      expect(arrowClasses.has('search-match')).toBe(true);
      expect(labelChildClasses.has('search-match')).toBe(true);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
      StaticData.getAllArcIds = origGetAllArcIds;
      StaticData.getArc = origGetArc;
      DomAdapter.getSvgRoot = origGetSvgRoot;
      DomAdapter.getNode = origDomGetNode;
      DomAdapter.getVisibleArc = origGetVisibleArc;
      DomAdapter.getVisibleArrows = origGetVisibleArrows;
      DomAdapter.getLabelGroup = origGetLabelGroup;
    });

    test('arc connected to matched node gets search-match', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      const origGetAllArcIds = StaticData.getAllArcIds;
      const origGetArc = StaticData.getArc;
      const origGetSvgRoot = DomAdapter.getSvgRoot;
      const origDomGetNode = DomAdapter.getNode;
      const origGetVisibleArc = DomAdapter.getVisibleArc;
      const origGetVisibleArrows = DomAdapter.getVisibleArrows;
      const origGetLabelGroup = DomAdapter.getLabelGroup;

      StaticData.getAllNodeIds = () => ['n1', 'n2'];
      StaticData.getNode = (id) =>
        ({
          n1: { name: 'my-crate', type: 'crate', parent: null },
          n2: { name: 'other', type: 'crate', parent: null },
        })[id];
      StaticData.getAllArcIds = () => ['arc-1'];
      StaticData.getArc = (id) =>
        id === 'arc-1'
          ? { from: 'n1', to: 'n2', usages: [{ symbol: 'HashMap' }] }
          : null;

      const arcClasses = new Set();
      const arcEl = {
        classList: {
          add: (c) => arcClasses.add(c),
          remove: (c) => arcClasses.delete(c),
        },
      };

      DomAdapter.getSvgRoot = () => ({
        classList: { add: () => {}, remove: () => {}, toggle: () => {} },
      });
      const matchLabel = {
        classList: {
          contains: () => true,
          add: () => {},
          remove: () => {},
          toggle: () => {},
        },
      };
      DomAdapter.getNode = (id) =>
        id === 'n1'
          ? {
              classList: { add: () => {}, remove: () => {}, toggle: () => {} },
              nextElementSibling: matchLabel,
            }
          : null;
      DomAdapter.getVisibleArc = (id) => (id === 'arc-1' ? arcEl : null);
      DomAdapter.getVisibleArrows = () => [];
      DomAdapter.getLabelGroup = () => null;

      // Search by node name — arc endpoint matches
      SearchLogic.executeSearch('my-crate', 'all');

      expect(arcClasses.has('search-match')).toBe(true);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
      StaticData.getAllArcIds = origGetAllArcIds;
      StaticData.getArc = origGetArc;
      DomAdapter.getSvgRoot = origGetSvgRoot;
      DomAdapter.getNode = origDomGetNode;
      DomAdapter.getVisibleArc = origGetVisibleArc;
      DomAdapter.getVisibleArrows = origGetVisibleArrows;
      DomAdapter.getLabelGroup = origGetLabelGroup;
    });

    test('arc between non-matching nodes has no search-match', () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      const origGetAllArcIds = StaticData.getAllArcIds;
      const origGetArc = StaticData.getArc;
      const origGetSvgRoot = DomAdapter.getSvgRoot;
      const origDomGetNode = DomAdapter.getNode;
      const origGetVisibleArc = DomAdapter.getVisibleArc;
      const origGetVisibleArrows = DomAdapter.getVisibleArrows;
      const origGetLabelGroup = DomAdapter.getLabelGroup;

      StaticData.getAllNodeIds = () => ['n1', 'n2', 'n3'];
      StaticData.getNode = (id) =>
        ({
          n1: { name: 'alpha', type: 'crate', parent: null },
          n2: { name: 'beta', type: 'crate', parent: null },
          n3: { name: 'target-crate', type: 'crate', parent: null },
        })[id];
      StaticData.getAllArcIds = () => ['arc-1'];
      StaticData.getArc = (id) =>
        id === 'arc-1'
          ? { from: 'n1', to: 'n2', usages: [{ symbol: 'Foo' }] }
          : null;

      const arcClasses = new Set();

      DomAdapter.getSvgRoot = () => ({
        classList: { add: () => {}, remove: () => {}, toggle: () => {} },
      });
      const matchLabel = {
        classList: {
          contains: () => true,
          add: () => {},
          remove: () => {},
          toggle: () => {},
        },
      };
      DomAdapter.getNode = (id) =>
        id === 'n3'
          ? {
              classList: { add: () => {}, remove: () => {}, toggle: () => {} },
              nextElementSibling: matchLabel,
            }
          : null;
      DomAdapter.getVisibleArc = () => null;
      DomAdapter.getVisibleArrows = () => [];
      DomAdapter.getLabelGroup = () => null;

      // Search matches n3, but arc-1 connects n1↔n2 — no search-match
      SearchLogic.executeSearch('target', 'all');

      expect(arcClasses.has('search-match')).toBe(false);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
      StaticData.getAllArcIds = origGetAllArcIds;
      StaticData.getArc = origGetArc;
      DomAdapter.getSvgRoot = origGetSvgRoot;
      DomAdapter.getNode = origDomGetNode;
      DomAdapter.getVisibleArc = origGetVisibleArc;
      DomAdapter.getVisibleArrows = origGetVisibleArrows;
      DomAdapter.getLabelGroup = origGetLabelGroup;
    });

    test("scope='symbol' skips node names", () => {
      const origGetAllNodeIds = StaticData.getAllNodeIds;
      const origGetNode = StaticData.getNode;
      const origGetAllArcIds = StaticData.getAllArcIds;
      const origGetArc = StaticData.getArc;

      StaticData.getAllNodeIds = () => ['n1'];
      StaticData.getNode = (id) =>
        id === 'n1' ? { name: 'HashMap', type: 'crate', parent: null } : null;
      StaticData.getAllArcIds = () => [];
      StaticData.getArc = () => null;

      const result = SearchLogic.executeSearch('HashMap', 'symbol');
      expect(result).toBe(0);

      StaticData.getAllNodeIds = origGetAllNodeIds;
      StaticData.getNode = origGetNode;
      StaticData.getAllArcIds = origGetAllArcIds;
      StaticData.getArc = origGetArc;
    });
  });
});
