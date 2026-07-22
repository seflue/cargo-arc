// highlight_renderer.test.js - Tests for HighlightRenderer (DOM application of highlight state)
import { beforeEach, describe, expect, test } from 'bun:test';

// Set up STATIC_DATA.classes (normally injected by render.rs)
if (!globalThis.STATIC_DATA) globalThis.STATIC_DATA = {};
if (!globalThis.STATIC_DATA.classes) globalThis.STATIC_DATA.classes = {};
Object.assign(globalThis.STATIC_DATA.classes, {
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
  hasHighlight: 'has-highlight',
  hasPinned: 'has-pinned',
  selectedCrate: 'selectedCrate',
  selectedModule: 'selectedModule',
  selectedExternal: 'selectedExternal',
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
});

import { ArcLogic } from './arc_logic.js';

global.ArcLogic = ArcLogic;

import { Selectors } from './selectors.js';

global.Selectors = Selectors;

import { LayerManager } from './layer_manager.js';

global.LayerManager = LayerManager;

import { createFakeElement, createMockDomAdapter } from './dom_adapter.js';
import { HighlightRenderer } from './highlight_renderer.js';

// Helper: create a minimal mock staticData for renderer tests
function createRendererStaticData(nodeIds, arcData) {
  return {
    getAllNodeIds: () => nodeIds,
    getAllArcIds: () => Object.keys(arcData),
    getArcStrokeWidth: (arcId) => arcData[arcId]?.strokeWidth ?? 0.5,
    getArc: (arcId) => arcData[arcId],
  };
}

// Helper: register SVG root element on mock dom
function registerSvgRoot(dom) {
  const svg = createFakeElement('svg');
  dom._registerSelector('svg', svg);
  return svg;
}

// Helper: register layer elements
function registerLayers(dom) {
  const layers = {};
  for (const id of Object.values(LayerManager.LAYERS)) {
    const el = createFakeElement('g');
    el.innerHTML = '';
    dom._registerElement(id, el);
    layers[id] = el;
  }
  return layers;
}

describe('HighlightRenderer', () => {
  let dom, svg, layers;
  const C = STATIC_DATA.classes;

  beforeEach(() => {
    dom = createMockDomAdapter();
    svg = registerSvgRoot(dom);
    layers = registerLayers(dom);
    // Reset dirty-set state from previous tests (singleton)
    HighlightRenderer._prevNodeIds = new Set();
    HighlightRenderer._prevRegularArcIds = new Set();
    HighlightRenderer._prevVirtualArcIds = new Set();
  });

  describe('apply(null) — reset', () => {
    test('removes has-highlight class from SVG root', () => {
      svg.classList.add(C.hasHighlight);

      const staticData = createRendererStaticData([], {});
      HighlightRenderer.apply(dom, staticData, new Map(), null);

      expect(svg.classList.contains(C.hasHighlight)).toBe(false);
    });

    test('resets node classes via dirty-set', () => {
      const nodeEl = createFakeElement('g');
      dom._registerElement(Selectors.nodeId('n1'), nodeEl);

      const staticData = createRendererStaticData(['n1'], {});
      // Apply state to populate dirty set
      HighlightRenderer.apply(dom, staticData, new Map(), {
        nodeHighlights: new Map([
          ['n1', { role: 'current', cssClass: 'selectedCrate' }],
        ]),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      });
      expect(nodeEl.classList.contains(C.selectedCrate)).toBe(true);

      // Reset clears only dirty-set nodes
      HighlightRenderer.apply(dom, staticData, new Map(), null);
      expect(nodeEl.classList.contains(C.selectedCrate)).toBe(false);
    });

    test('resets regular arc strokeWidth to base from staticData', () => {
      const arcEl = createFakeElement('path');
      dom._registerSelector(Selectors.baseArc('a-b'), arcEl);

      const arrowEl = createFakeElement('polygon');
      arrowEl.setAttribute(
        'points',
        ArcLogic.getArrowPoints({ x: 100, y: 200 }, 0.33),
      );
      dom._registerSelector(Selectors.arrows('a-b'), [arrowEl]);

      const staticData = createRendererStaticData([], {
        'a-b': { from: 'a', to: 'b', strokeWidth: 0.5 },
      });
      // Apply highlight to populate dirty set
      HighlightRenderer.apply(dom, staticData, new Map(), {
        nodeHighlights: new Map(),
        arcHighlights: new Map([
          [
            'a-b',
            {
              highlightWidth: 2.0,
              arrowScale: 1.3,
              relationType: 'dep',
              isVirtual: false,
            },
          ],
        ]),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      });
      expect(arcEl.classList.contains(C.highlightedArc)).toBe(true);

      // Reset
      HighlightRenderer.apply(dom, staticData, new Map(), null);
      expect(arcEl.classList.contains(C.highlightedArc)).toBe(false);
      expect(parseFloat(arcEl.style.strokeWidth)).toBeCloseTo(0.5, 2);
      expect(arrowEl.classList.contains(C.highlightedArrow)).toBe(false);
    });

    test('resets virtual arc styles via dirty-set and cache', () => {
      const vArcEl = createFakeElement('path');
      vArcEl.classList.add(C.virtualArc);

      const vArrowEl = createFakeElement('polygon');
      vArrowEl.classList.add(C.virtualArrow);
      vArrowEl.setAttribute(
        'points',
        ArcLogic.getArrowPoints({ x: 50, y: 100 }, 1.0),
      );

      const vLabelEl = createFakeElement('text');
      vLabelEl.classList.add(C.arcCount);
      const labelGroup = createFakeElement('g');
      labelGroup.appendChild(vLabelEl);

      // Register in cache (used by reset via dirty-set + cache)
      dom.cacheArcElements('x-y', vArcEl, [vArrowEl], labelGroup);

      // Register querySelectorAll results (used by apply phase for virtual arcs)
      dom._registerSelector(`.${C.virtualArc}[data-arc-id="x-y"]`, [vArcEl]);
      dom._registerSelector(`.${C.virtualArrow}[data-vedge="x-y"]`, [vArrowEl]);
      dom._registerSelector(`.${C.arcCount}[data-vedge="x-y"]`, [vLabelEl]);

      const virtualArcUsages = new Map([
        [
          'x-y',
          [
            {
              symbol: 'f',
              modulePath: null,
              locations: [{ file: 'a.rs', line: 1 }],
            },
          ],
        ],
      ]);

      const staticData = createRendererStaticData([], {});
      // Apply highlight with virtual arc to populate dirty set
      HighlightRenderer.apply(dom, staticData, virtualArcUsages, {
        nodeHighlights: new Map(),
        arcHighlights: new Map([
          [
            'v:x-y',
            {
              highlightWidth: 1.5,
              arrowScale: 1.0,
              relationType: 'dep',
              isVirtual: true,
            },
          ],
        ]),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      });
      expect(vArcEl.classList.contains(C.highlightedArc)).toBe(true);

      // Reset
      HighlightRenderer.apply(dom, staticData, virtualArcUsages, null);
      expect(vArcEl.classList.contains(C.highlightedArc)).toBe(false);
      expect(vArrowEl.classList.contains(C.highlightedArrow)).toBe(false);
      expect(vLabelEl.classList.contains(C.highlightedLabel)).toBe(false);
    });
  });

  describe('apply(state) — highlight', () => {
    test('adds has-highlight to SVG root (dimming)', () => {
      const staticData = createRendererStaticData([], {});
      const state = {
        nodeHighlights: new Map(),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      expect(svg.classList.contains(C.hasHighlight)).toBe(true);
    });

    test('sets CSS class on highlighted node', () => {
      const nodeEl = createFakeElement('g');
      dom._registerElement(Selectors.nodeId('n1'), nodeEl);

      const staticData = createRendererStaticData(['n1'], {});
      const state = {
        nodeHighlights: new Map([
          ['n1', { role: 'current', cssClass: 'selectedModule' }],
        ]),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      expect(nodeEl.classList.contains(C.selectedModule)).toBe(true);
    });

    test('sets highlightedArc class and strokeWidth on arc', () => {
      const arcEl = createFakeElement('path');
      arcEl.style.strokeWidth = '0.5px';
      dom._registerSelector(Selectors.baseArc('a-b'), arcEl);
      dom._registerSelector(Selectors.arrows('a-b'), []);
      dom._registerSelector(
        Selectors.labelGroup ? `.${C.arcCountGroup}[data-vedge="a-b"]` : '',
        null,
      );

      const staticData = createRendererStaticData([], {
        'a-b': { from: 'a', to: 'b', strokeWidth: 0.5 },
      });
      const state = {
        nodeHighlights: new Map(),
        arcHighlights: new Map([
          [
            'a-b',
            {
              highlightWidth: 0.65,
              arrowScale: 0.43,
              relationType: 'dep',
              isVirtual: false,
            },
          ],
        ]),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      expect(arcEl.classList.contains(C.highlightedArc)).toBe(true);
      expect(parseFloat(arcEl.style.strokeWidth)).toBeCloseTo(0.65, 2);
    });

    test('scales arrows to match highlight', () => {
      const arrowEl = createFakeElement('polygon');
      const originalTip = { x: 100, y: 200 };
      arrowEl.setAttribute(
        'points',
        ArcLogic.getArrowPoints(originalTip, 0.33),
      );
      dom._registerSelector(
        Selectors.baseArc('a-b'),
        createFakeElement('path'),
      );
      dom._registerSelector(Selectors.arrows('a-b'), [arrowEl]);

      const staticData = createRendererStaticData([], {
        'a-b': { from: 'a', to: 'b', strokeWidth: 0.5 },
      });
      const highlightScale = 0.43;
      const state = {
        nodeHighlights: new Map(),
        arcHighlights: new Map([
          [
            'a-b',
            {
              highlightWidth: 0.65,
              arrowScale: highlightScale,
              relationType: 'dep',
              isVirtual: false,
            },
          ],
        ]),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      expect(arrowEl.classList.contains(C.highlightedArrow)).toBe(true);
      // Arrow tip should be preserved
      const tip = ArcLogic.parseTipFromPoints(arrowEl.getAttribute('points'));
      expect(tip.x).toBe(originalTip.x);
      expect(tip.y).toBe(originalTip.y);
    });

    test('shadow clones strip data-arc-id to prevent escape from shadow layer', () => {
      // Regression: cloneNode(false) copies data-arc-id from virtual arcs.
      // _promoteToHighlightLayers queries '.virtual-arc[data-arc-id="..."]' and
      // matches shadow clones too, moving them from SHADOWS → HIGHLIGHT_ARCS.
      // On next reset, _resetLayers moves them to BASE_ARCS (never cleaned).
      // _createShadows then clones the zombies → exponential DOM growth.
      const arcEl = createFakeElement('path');
      arcEl.classList.add(C.virtualArc);
      arcEl.setAttribute('data-arc-id', 'x-y');
      arcEl.setAttribute('d', 'M 0 0 C 50 0 50 100 0 100');

      dom._registerSelector(`.${C.virtualArc}[data-arc-id="x-y"]`, [arcEl]);

      const shadowLayer = layers[LayerManager.LAYERS.SHADOWS];

      const state = {
        nodeHighlights: new Map(),
        arcHighlights: new Map([
          [
            'v:x-y',
            {
              highlightWidth: 0.65,
              arrowScale: 0.43,
              relationType: 'dep',
              isVirtual: true,
            },
          ],
        ]),
        shadowData: new Map([
          [
            'v:x-y',
            {
              shadowWidth: 2.0,
              visibleLength: 50,
              dashOffset: 0,
              glowClass: 'glowIncoming',
            },
          ],
        ]),
        promotedHitareas: new Set(),
      };

      const staticData = createRendererStaticData([], {});
      HighlightRenderer.apply(dom, staticData, new Map(), state);

      // Shadow layer should have exactly 1 child (the shadow clone)
      expect(shadowLayer.children.length).toBe(1);
      const shadow = shadowLayer.children[0];

      // Critical: shadow must NOT have data-arc-id (prevents querySelectorAll match)
      expect(shadow.getAttribute('data-arc-id')).toBeNull();

      // Shadow should have shadowPath class
      expect(shadow.classList.contains(C.shadowPath)).toBe(true);
    });

    test('dimming order: resetDimming before classes, activateDimming after', () => {
      // Verify that svg starts without has-highlight after reset,
      // and ends with has-highlight after apply
      svg.classList.add(C.hasHighlight); // pre-existing

      const staticData = createRendererStaticData([], {});
      const state = {
        nodeHighlights: new Map(),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      // After full apply: has-highlight should be present (activateDimming ran last)
      expect(svg.classList.contains(C.hasHighlight)).toBe(true);
    });

    test('cycle-member: node gets cycle-member CSS class', () => {
      const nodeEl = createFakeElement('g');
      dom._registerElement(Selectors.nodeId('n1'), nodeEl);

      const staticData = createRendererStaticData(['n1'], {});
      const state = {
        nodeHighlights: new Map([
          ['n1', { role: 'cycle-member', cssClass: 'cycleMember' }],
        ]),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      expect(nodeEl.classList.contains(C.cycleMember)).toBe(true);
    });

    test('cycle-member: resetToBase removes cycle-member class', () => {
      const nodeEl = createFakeElement('g');
      dom._registerElement(Selectors.nodeId('n1'), nodeEl);

      const staticData = createRendererStaticData(['n1'], {});
      // Apply cycle-member state to populate dirty set
      HighlightRenderer.apply(dom, staticData, new Map(), {
        nodeHighlights: new Map([
          ['n1', { role: 'cycle-member', cssClass: 'cycleMember' }],
        ]),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      });
      expect(nodeEl.classList.contains(C.cycleMember)).toBe(true);

      // Reset
      HighlightRenderer.apply(dom, staticData, new Map(), null);
      expect(nodeEl.classList.contains(C.cycleMember)).toBe(false);
    });

    test('cycle-member: multiple nodes get cycle-member simultaneously', () => {
      const nodeA = createFakeElement('g');
      const nodeB = createFakeElement('g');
      dom._registerElement(Selectors.nodeId('a'), nodeA);
      dom._registerElement(Selectors.nodeId('b'), nodeB);

      const staticData = createRendererStaticData(['a', 'b'], {});
      const state = {
        nodeHighlights: new Map([
          ['a', { role: 'cycle-member', cssClass: 'cycleMember' }],
          ['b', { role: 'cycle-member', cssClass: 'cycleMember' }],
        ]),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      expect(nodeA.classList.contains(C.cycleMember)).toBe(true);
      expect(nodeB.classList.contains(C.cycleMember)).toBe(true);
    });

    test('isPinned true: adds has-pinned class to SVG root', () => {
      const staticData = createRendererStaticData([], {});
      const state = {
        nodeHighlights: new Map(),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
        isPinned: true,
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      expect(svg.classList.contains(C.hasPinned)).toBe(true);
      expect(svg.classList.contains(C.hasHighlight)).toBe(true);
    });

    test('isPinned false: does not add has-pinned class to SVG root', () => {
      const staticData = createRendererStaticData([], {});
      const state = {
        nodeHighlights: new Map(),
        arcHighlights: new Map(),
        shadowData: new Map(),
        promotedHitareas: new Set(),
        isPinned: false,
      };

      HighlightRenderer.apply(dom, staticData, new Map(), state);

      expect(svg.classList.contains(C.hasPinned)).toBe(false);
      expect(svg.classList.contains(C.hasHighlight)).toBe(true);
    });

    test('apply(null) removes has-pinned class from SVG root', () => {
      svg.classList.add(C.hasPinned);

      const staticData = createRendererStaticData([], {});
      HighlightRenderer.apply(dom, staticData, new Map(), null);

      expect(svg.classList.contains(C.hasPinned)).toBe(false);
    });
  });
});
