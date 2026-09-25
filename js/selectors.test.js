import { describe, expect, test } from 'bun:test';

// Provide STATIC_DATA.classes for selectors (normally injected by render.rs)
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
  upwardArrow: 'upward-arrow',
  cycleArrow: 'cycle-arrow',
});

import { Selectors } from './selectors.js';

describe('Selectors', () => {
  describe('IDs', () => {
    test('nodeId generates node-prefixed ID', () => {
      expect(Selectors.nodeId('foo')).toBe('node-foo');
      expect(Selectors.nodeId('crate::module')).toBe('node-crate::module');
    });

    test('countId generates count-prefixed ID', () => {
      expect(Selectors.countId('bar')).toBe('count-bar');
    });
  });

  describe('CSS Selectors', () => {
    test('baseArc selects dep-arc or cycle-arc by arc-id', () => {
      expect(Selectors.baseArc('a-b')).toBe(
        '.dep-arc[data-arc-id="a-b"], .cycle-arc[data-arc-id="a-b"]',
      );
    });

    test('hitarea selects arc-hitarea by arc-id', () => {
      expect(Selectors.hitarea('x-y')).toBe('.arc-hitarea[data-arc-id="x-y"]');
    });

    test('arrows selects by data-edge attribute', () => {
      expect(Selectors.arrows('from-to')).toBe('polygon[data-edge="from-to"]');
    });

    test('virtualArrows selects data-vedge excluding arc-count', () => {
      expect(Selectors.virtualArrows('v-edge')).toBe(
        '[data-vedge="v-edge"]:not(.arc-count)',
      );
    });

    test('virtualArc selects by from and to attributes', () => {
      expect(Selectors.virtualArc('nodeA', 'nodeB')).toBe(
        '.virtual-arc[data-from="nodeA"][data-to="nodeB"]',
      );
    });

    test('labelGroup selects arc-count-group by vedge', () => {
      expect(Selectors.labelGroup('edge-id')).toBe(
        '.arc-count-group[data-vedge="edge-id"]',
      );
    });
  });

  describe('Identity Selectors', () => {
    test('collapseToggle selects by data-target', () => {
      expect(Selectors.collapseToggle('myNode')).toBe(
        '.collapse-toggle[data-target="myNode"]',
      );
    });

    test('treeLineChild selects lines by data-child', () => {
      expect(Selectors.treeLineChild('child1')).toBe(
        'line[data-child="child1"]',
      );
    });

    test('treeLineParent selects lines by data-parent', () => {
      expect(Selectors.treeLineParent('parent1')).toBe(
        'line[data-parent="parent1"]',
      );
    });
  });

  describe('Category Selectors', () => {
    test('allHitareas returns .arc-hitarea', () => {
      expect(Selectors.allHitareas()).toBe('.arc-hitarea');
    });

    test('allVirtualElements returns all virtual element classes', () => {
      expect(Selectors.allVirtualElements()).toBe(
        '.virtual-arc, .virtual-hitarea, .virtual-arrow, .arc-count, .arc-count-group, .arc-count-bg',
      );
    });

    test('allBaseEdges returns hitarea, dep-arc, cycle-arc', () => {
      expect(Selectors.allBaseEdges()).toBe(
        '.arc-hitarea, .dep-arc, .cycle-arc',
      );
    });

    test('allBaseArrows returns dep-arrow, upward-arrow, cycle-arrow', () => {
      expect(Selectors.allBaseArrows()).toBe(
        '.dep-arrow, .upward-arrow, .cycle-arrow',
      );
    });

    test('allArcPaths returns dep-arc, cycle-arc, virtual-arc', () => {
      expect(Selectors.allArcPaths()).toBe(
        '.dep-arc, .cycle-arc, .virtual-arc',
      );
    });
  });

  describe('Edge cases', () => {
    test('handles empty string IDs', () => {
      expect(Selectors.nodeId('')).toBe('node-');
      expect(Selectors.baseArc('')).toBe(
        '.dep-arc[data-arc-id=""], .cycle-arc[data-arc-id=""]',
      );
    });

    test('handles IDs with special characters', () => {
      expect(Selectors.nodeId('crate::mod::sub')).toBe('node-crate::mod::sub');
      expect(Selectors.virtualArc('a::b', 'c::d')).toBe(
        '.virtual-arc[data-from="a::b"][data-to="c::d"]',
      );
    });
  });
});
