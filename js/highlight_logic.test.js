// highlight_logic.test.js - Tests for pure highlight calculation functions
const { expect, test, describe } = require('bun:test');

// Make ArcLogic globally available (simulating browser environment where it's loaded first)
const { ArcLogic } = require('./arc_logic.js');
global.ArcLogic = ArcLogic;

const { HighlightLogic } = require('./highlight_logic.js');

describe('HighlightLogic', () => {
  describe('constants', () => {
    test('HIGHLIGHT_SCALE is 1.3', () => {
      expect(HighlightLogic.HIGHLIGHT_SCALE).toBe(1.3);
    });

    test('SHADOW_MULTIPLIER is 4', () => {
      expect(HighlightLogic.SHADOW_MULTIPLIER).toBe(4);
    });
  });

  describe('calculateHighlightWidth', () => {
    test('scales base width by 1.3', () => {
      expect(HighlightLogic.calculateHighlightWidth(1.0)).toBe(1.3);
      expect(HighlightLogic.calculateHighlightWidth(2.0)).toBe(2.6);
      expect(HighlightLogic.calculateHighlightWidth(0.5)).toBe(0.65);
    });

    test('handles zero', () => {
      expect(HighlightLogic.calculateHighlightWidth(0)).toBe(0);
    });
  });

  describe('calculateShadowWidth', () => {
    test('multiplies arc width by 4', () => {
      expect(HighlightLogic.calculateShadowWidth(1.0)).toBe(4.0);
      expect(HighlightLogic.calculateShadowWidth(0.5)).toBe(2.0);
      expect(HighlightLogic.calculateShadowWidth(2.5)).toBe(10.0);
    });
  });

  describe('calculateShadowOverhang', () => {
    test('calculates overhang as (shadow - arc) / 2', () => {
      expect(HighlightLogic.calculateShadowOverhang(4, 1)).toBe(1.5);
      expect(HighlightLogic.calculateShadowOverhang(2, 0.5)).toBe(0.75);
      expect(HighlightLogic.calculateShadowOverhang(10, 2)).toBe(4);
    });
  });

  describe('calculateVisibleLength', () => {
    test('subtracts 2x overhang from path length', () => {
      expect(HighlightLogic.calculateVisibleLength(100, 10)).toBe(80);
      expect(HighlightLogic.calculateVisibleLength(50, 5)).toBe(40);
    });

    test('returns 0 when overhang exceeds path', () => {
      expect(HighlightLogic.calculateVisibleLength(10, 10)).toBe(0);
      expect(HighlightLogic.calculateVisibleLength(5, 10)).toBe(0);
    });
  });

  describe('calculateDashOffset', () => {
    test('returns negative overhang', () => {
      expect(HighlightLogic.calculateDashOffset(10)).toBe(-10);
      expect(HighlightLogic.calculateDashOffset(1.5)).toBe(-1.5);
    });

    test('handles zero overhang', () => {
      // Note: -0 === 0 in JS, but toBe uses Object.is which distinguishes them
      expect(HighlightLogic.calculateDashOffset(0)).toEqual(0);
    });
  });

  describe('determineRelationType', () => {
    test('returns dep when from matches selected (outgoing)', () => {
      expect(HighlightLogic.determineRelationType('nodeA', 'nodeB', 'nodeA')).toBe('dep');
    });

    test('returns reverse when to matches selected (incoming)', () => {
      expect(HighlightLogic.determineRelationType('nodeA', 'nodeB', 'nodeB')).toBe('reverse');
    });

    test('returns reverse when selected is unrelated', () => {
      expect(HighlightLogic.determineRelationType('nodeA', 'nodeB', 'nodeC')).toBe('reverse');
    });
  });

  describe('calculateVirtualArrowScale', () => {
    test('divides stroke width by 1.5', () => {
      expect(HighlightLogic.calculateVirtualArrowScale(1.5)).toBe(1.0);
      expect(HighlightLogic.calculateVirtualArrowScale(3.0)).toBe(2.0);
      expect(HighlightLogic.calculateVirtualArrowScale(0.75)).toBe(0.5);
    });
  });

  describe('calculateShadowData', () => {
    test('combines all shadow calculations', () => {
      const result = HighlightLogic.calculateShadowData(1.0, 100);

      expect(result.shadowWidth).toBe(4.0);  // 1.0 * 4
      expect(result.overhang).toBe(1.5);     // (4 - 1) / 2
      expect(result.visibleLength).toBe(97); // 100 - 1.5*2
      expect(result.dashOffset).toBe(-1.5);  // -overhang
    });

    test('handles edge case where overhang exceeds path', () => {
      const result = HighlightLogic.calculateShadowData(10, 10);

      expect(result.shadowWidth).toBe(40);
      expect(result.overhang).toBe(15);      // (40 - 10) / 2
      expect(result.visibleLength).toBe(0);  // max(0, 10 - 30)
      expect(result.dashOffset).toBe(-15);
    });
  });
});
