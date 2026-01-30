// highlight_integration.test.js - Integration tests for highlight/clearHighlights behavior
// Ensures proper separation of highlight state (scale) vs layout state (position)

import { test, expect, describe } from "bun:test";

// Setup globals (simulating browser environment)
const { ArcLogic } = require('./arc_logic.js');
global.ArcLogic = ArcLogic;
global.HighlightLogic = require('./highlight_logic.js').HighlightLogic;

const { createMockDomAdapter, createFakeElement } = require('./dom_adapter.js');
const { AppState } = require('./app_state.js');

describe('clearHighlights - Separation of Highlight State vs Layout State', () => {
  /**
   * Specification: clearHighlights should only restore highlight state (scale),
   * not layout state (position).
   *
   * Key insight: Scale is now calculated from StaticData, not stored.
   * Position is always read from DOM (current layout state).
   */

  test('preserves arrow position after relayout when clearing highlights', () => {
    // Create fake arrow element
    const arrow = createFakeElement('polygon');

    // Initial expanded position (100, 307), scale 1.0
    const initialPoints = global.ArcLogic.getArrowPoints({ x: 100, y: 307 }, 1.0);
    arrow.setAttribute('points', initialPoints);
    expect(arrow.getAttribute('points')).toBe('108,303 100,307 108,311');

    // === RELAYOUT: Node collapses, arrow moves to new position (100, 187) ===
    const newCollapsedPoints = global.ArcLogic.getArrowPoints({ x: 100, y: 187 }, 1.0);
    arrow.setAttribute('points', newCollapsedPoints);
    expect(arrow.getAttribute('points')).toBe('108,183 100,187 108,191');

    // === HIGHLIGHT: User hovers, arrow scales to 1.3 ===
    const highlightedPoints = global.ArcLogic.getArrowPoints({ x: 100, y: 187 }, 1.3);
    arrow.setAttribute('points', highlightedPoints);
    expect(arrow.getAttribute('points')).toBe('110.4,181.8 100,187 110.4,192.2');

    // === CLEAR HIGHLIGHTS: Should restore scale but KEEP position ===
    // Scale comes from StaticData (simulated as 1.0 here)
    const baseScale = 1.0;
    const currentTip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));

    // This is the key: use currentTip (187), not some stored position
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints(
      currentTip,   // Keep CURRENT position (layout state from DOM)
      baseScale     // Restore base scale (from StaticData calculation)
    ));

    // Verify: Arrow should be at collapsed position (187), not expanded position (307)
    expect(arrow.getAttribute('points')).toBe('108,183 100,187 108,191');

    const finalTip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(finalTip.y).toBe(187);  // Collapsed position
    expect(finalTip.y).not.toBe(307);  // NOT expanded position
  });

  test('clearHighlights restores scale correctly', () => {
    const arrow = createFakeElement('polygon');

    // Initial: scale 1.0
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 200 }, 1.0));

    // Highlight: scale to 1.3
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 200 }, 1.3));
    expect(arrow.getAttribute('points')).toBe('110.4,194.8 100,200 110.4,205.2');

    // Clear highlights: restore scale (calculated from StaticData)
    const baseScale = 1.0;  // Would come from ArcLogic.scaleFromStrokeWidth(StaticData.getArcStrokeWidth(arcId))
    const currentTip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints(currentTip, baseScale));

    // Verify: back to scale 1.0
    expect(arrow.getAttribute('points')).toBe('108,196 100,200 108,204');
  });

  test('handles multiple relayouts correctly', () => {
    const arrow = createFakeElement('polygon');
    const baseScale = 1.0;  // From StaticData

    // Initial position: Y=300
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 300 }, 1.0));

    // First relayout: collapse to Y=200
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 200 }, 1.0));

    // Highlight
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 200 }, 1.3));

    // Clear - should be at Y=200
    let currentTip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints(currentTip, baseScale));
    expect(global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points')).y).toBe(200);

    // Second relayout: collapse further to Y=100
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 100 }, 1.0));

    // Highlight again
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 100 }, 1.3));

    // Clear - should be at Y=100, NOT Y=200 or Y=300
    currentTip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints(currentTip, baseScale));
    expect(global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points')).y).toBe(100);
  });

  test('uses current arrow position from DOM, not stored position', () => {
    // Specification: clearHighlights must read current position from DOM
    const arrow = createFakeElement('polygon');
    const baseScale = 1.0;

    // Initial state
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 307 }, 1.0));

    // Relayout changes position
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 187 }, 1.0));

    // Highlight
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 187 }, 1.3));

    // Clear: must use current position from DOM
    const currentTip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints(
      currentTip,   // Current position (source of truth)
      baseScale     // Base scale from StaticData
    ));

    // Verify: position matches current layout state (187)
    const finalTip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(finalTip.y).toBe(187);
  });
});

describe('State Separation: Highlight vs Layout', () => {
  test('highlight state: scale is calculated from StaticData', () => {
    // Scale is now calculated on demand from StaticData.getArcStrokeWidth()
    // No stored state needed

    // Simulate: arc with 1 usage -> strokeWidth 0.5 -> scale 0.33
    const strokeWidth = global.ArcLogic.calculateStrokeWidth(1);
    const scale = global.ArcLogic.scaleFromStrokeWidth(strokeWidth);

    expect(strokeWidth).toBe(0.5);  // MIN strokeWidth
    expect(scale).toBeCloseTo(0.333, 2);  // 0.5 / 1.5

    // During highlight, scale changes to HIGHLIGHT_SCALE (1.3)
    expect(global.HighlightLogic.HIGHLIGHT_SCALE).toBe(1.3);

    // When clearing, we recalculate from StaticData (same result)
    const clearedScale = global.ArcLogic.scaleFromStrokeWidth(strokeWidth);
    expect(clearedScale).toBeCloseTo(0.333, 2);
  });

  test('layout state: position (persistent, changes on relayout)', () => {
    const arrow = createFakeElement('polygon');

    // Initial layout: Y=300
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 300 }, 1.0));
    let tip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(tip.y).toBe(300);

    // Relayout changes position: Y=200
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 200 }, 1.0));
    tip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(tip.y).toBe(200);

    // Highlight should NOT affect position, only scale
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 200 }, 1.3));
    tip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(tip.y).toBe(200);  // Position unchanged

    // Clear highlight should NOT restore old position (300), only current scale
    arrow.setAttribute('points', global.ArcLogic.getArrowPoints({ x: 100, y: 200 }, 1.0));
    tip = global.ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(tip.y).toBe(200);  // Position still at current layout state
  });
});

describe('Virtual Arc Hover Bug Regression Tests', () => {
  /**
   * Bug: When hovering over modules repeatedly, virtual arc arrows and shadows
   * grow with each hover instead of resetting to original size.
   *
   * This tests the exact flow:
   * 1. Virtual arc exists with original strokeWidth
   * 2. Hover → highlightArcElement reads strokeWidth, scales by 1.3
   * 3. Leave → resetArcToOriginal should restore original strokeWidth
   * 4. Hover again → should read ORIGINAL strokeWidth, not highlighted
   */

  test('virtual arc strokeWidth resets correctly after highlight cycle', () => {
    // Simulate virtual arc with 3 usages
    const arc = createFakeElement('path');
    arc.classList.add('virtual-arc');

    // Initial strokeWidth calculated from 3 usages
    const usages = "file1:10|file2:20|file3:30";
    const usageCount = ArcLogic.countLocations(usages);
    expect(usageCount).toBe(3);

    const originalStrokeWidth = ArcLogic.calculateStrokeWidth(usageCount);
    arc.style.strokeWidth = originalStrokeWidth + 'px';

    // Verify initial state
    expect(parseFloat(arc.style.strokeWidth)).toBeCloseTo(originalStrokeWidth, 5);

    // === HIGHLIGHT (simulates highlightArcElement) ===
    const currentWidth = parseFloat(arc.style.strokeWidth);
    const highlightWidth = HighlightLogic.calculateHighlightWidth(currentWidth);
    arc.style.strokeWidth = highlightWidth + 'px';

    expect(parseFloat(arc.style.strokeWidth)).toBeCloseTo(highlightWidth, 5);
    expect(highlightWidth).toBeCloseTo(originalStrokeWidth * 1.3, 5);

    // === CLEAR (simulates resetArcToOriginal for virtual arc) ===
    // Key: reset calculates from usages, not from DOM
    const resetCount = ArcLogic.countLocations(usages);
    const resetStrokeWidth = ArcLogic.calculateStrokeWidth(resetCount);
    arc.style.strokeWidth = resetStrokeWidth + 'px';

    // Verify reset to ORIGINAL, not highlighted
    expect(parseFloat(arc.style.strokeWidth)).toBeCloseTo(originalStrokeWidth, 5);

    // === HOVER AGAIN ===
    const secondCurrentWidth = parseFloat(arc.style.strokeWidth);
    const secondHighlightWidth = HighlightLogic.calculateHighlightWidth(secondCurrentWidth);

    // BUG CHECK: secondHighlightWidth should equal first highlightWidth
    expect(secondHighlightWidth).toBeCloseTo(highlightWidth, 5);
    expect(secondHighlightWidth).not.toBeCloseTo(highlightWidth * 1.3, 5);  // Would be 1.69x if bug exists
  });

  test('virtual arrow scale resets correctly after highlight cycle', () => {
    const arrow = createFakeElement('polygon');
    arrow.classList.add('virtual-arrow');

    // Initial: 2 usages → strokeWidth → scale
    const usages = "file1:10|file2:20";
    const usageCount = ArcLogic.countLocations(usages);
    const strokeWidth = ArcLogic.calculateStrokeWidth(usageCount);
    const originalScale = strokeWidth / 1.5;

    // Set initial arrow points
    const tip = { x: 100, y: 200 };
    arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, originalScale));

    const initialPoints = arrow.getAttribute('points');

    // === HIGHLIGHT ===
    const highlightWidth = HighlightLogic.calculateHighlightWidth(strokeWidth);
    const highlightScale = HighlightLogic.calculateVirtualArrowScale(highlightWidth);
    const currentTip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', ArcLogic.getArrowPoints(currentTip, highlightScale));

    const highlightedPoints = arrow.getAttribute('points');
    expect(highlightedPoints).not.toBe(initialPoints);  // Should be different (scaled up)

    // === CLEAR ===
    const resetStrokeWidth = ArcLogic.calculateStrokeWidth(usageCount);
    const resetScale = resetStrokeWidth / 1.5;
    const tipAfterHighlight = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', ArcLogic.getArrowPoints(tipAfterHighlight, resetScale));

    const resetPoints = arrow.getAttribute('points');

    // Tip should be preserved
    const resetTip = ArcLogic.parseTipFromPoints(resetPoints);
    expect(resetTip.x).toBe(tip.x);
    expect(resetTip.y).toBe(tip.y);

    // Points should match initial (same scale)
    expect(resetPoints).toBe(initialPoints);

    // === HOVER AGAIN ===
    const secondTip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
    const secondHighlightScale = HighlightLogic.calculateVirtualArrowScale(
      HighlightLogic.calculateHighlightWidth(resetStrokeWidth)
    );
    arrow.setAttribute('points', ArcLogic.getArrowPoints(secondTip, secondHighlightScale));

    const secondHighlightedPoints = arrow.getAttribute('points');

    // BUG CHECK: second highlight should produce same points as first highlight
    expect(secondHighlightedPoints).toBe(highlightedPoints);
  });

  test('multiple hover cycles do not accumulate scale', () => {
    const arrow = createFakeElement('polygon');
    const usages = "file:1";
    const strokeWidth = ArcLogic.calculateStrokeWidth(ArcLogic.countLocations(usages));
    const baseScale = strokeWidth / 1.5;

    const tip = { x: 50, y: 100 };
    arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, baseScale));

    const originalPoints = arrow.getAttribute('points');

    // Simulate 5 hover/unhover cycles
    for (let i = 0; i < 5; i++) {
      // Highlight
      const highlightScale = HighlightLogic.calculateVirtualArrowScale(
        HighlightLogic.calculateHighlightWidth(strokeWidth)
      );
      const currentTip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
      arrow.setAttribute('points', ArcLogic.getArrowPoints(currentTip, highlightScale));

      // Clear
      const resetScale = strokeWidth / 1.5;
      const tipAfterHighlight = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
      arrow.setAttribute('points', ArcLogic.getArrowPoints(tipAfterHighlight, resetScale));
    }

    // After 5 cycles, should be back to original
    expect(arrow.getAttribute('points')).toBe(originalPoints);
  });

  test('BUG SIMULATION: what happens if strokeWidth is read from arc during highlight', () => {
    /**
     * This simulates the ACTUAL bug scenario:
     * 1. Virtual arc exists with original strokeWidth
     * 2. Hover → highlightArcElement reads arc.style.strokeWidth (should be original)
     * 3. Highlight is applied
     * 4. Leave → resetArcToOriginal should reset arc.style.strokeWidth
     * 5. Hover again → highlightArcElement reads arc.style.strokeWidth
     *
     * Bug occurs if step 4 doesn't reset properly, causing step 5 to read highlighted value.
     */
    const arc = createFakeElement('path');
    arc.classList.add('virtual-arc');

    const usages = "file1:10|file2:20|file3:30";
    const originalStrokeWidth = ArcLogic.calculateStrokeWidth(ArcLogic.countLocations(usages));
    arc.style.strokeWidth = originalStrokeWidth + 'px';

    const highlightedWidths = [];

    for (let cycle = 0; cycle < 5; cycle++) {
      // === HIGHLIGHT (simulates highlightArcElement reading from DOM) ===
      const currentWidth = parseFloat(arc.style.strokeWidth) || 0.5;
      const highlightWidth = HighlightLogic.calculateHighlightWidth(currentWidth);
      arc.style.strokeWidth = highlightWidth + 'px';
      highlightedWidths.push(highlightWidth);

      // === CLEAR (simulates resetArcToOriginal - calculates from usages, NOT from DOM) ===
      const resetCount = ArcLogic.countLocations(usages);
      const resetStrokeWidth = ArcLogic.calculateStrokeWidth(resetCount);
      arc.style.strokeWidth = resetStrokeWidth + 'px';
    }

    // All highlighted widths should be the same (no accumulation)
    for (let i = 1; i < highlightedWidths.length; i++) {
      expect(highlightedWidths[i]).toBeCloseTo(highlightedWidths[0], 5);
    }

    // Final state should be original
    expect(parseFloat(arc.style.strokeWidth)).toBeCloseTo(originalStrokeWidth, 5);
  });

  test('BUG CASE: empty sourceLocations causes wrong reset', () => {
    /**
     * Bug scenario: if hitarea.dataset.sourceLocations is empty/undefined,
     * resetArcToOriginal calculates wrong strokeWidth.
     */
    const arc = createFakeElement('path');
    arc.classList.add('virtual-arc');

    // Arc created with 5 usages but hitarea has empty sourceLocations (bug condition)
    const actualUsages = "a|b|c|d|e";  // 5 usages
    const originalStrokeWidth = ArcLogic.calculateStrokeWidth(5);
    arc.style.strokeWidth = originalStrokeWidth + 'px';

    // Simulate highlight
    const highlightWidth = HighlightLogic.calculateHighlightWidth(originalStrokeWidth);
    arc.style.strokeWidth = highlightWidth + 'px';

    // Simulate reset with EMPTY sourceLocations (bug condition)
    const emptySourceLocations = undefined;
    const resetCount = ArcLogic.countLocations(emptySourceLocations);  // Returns 0
    const resetStrokeWidth = ArcLogic.calculateStrokeWidth(resetCount);  // Returns 0.5

    // BUG: resetStrokeWidth (0.5) != originalStrokeWidth (~1.0)
    expect(resetStrokeWidth).not.toBeCloseTo(originalStrokeWidth, 1);
    expect(resetStrokeWidth).toBe(0.5);  // MIN value

    // This mismatch causes the bug: next highlight reads 0.5 instead of original
  });
});

describe('AppState without originalValues', () => {
  test('AppState.create() returns state without originalValues', () => {
    const appState = AppState.create();
    expect(appState.collapsed).toBeDefined();
    expect(appState.selection).toBeDefined();
    expect(appState.originalValues).toBeUndefined();
  });

  test('selection operations still work', () => {
    const appState = AppState.create();

    // Toggle pinned
    expect(AppState.togglePinned(appState, 'node', 'node-1')).toBe(true);
    expect(AppState.getPinned(appState)).toEqual({ type: 'node', id: 'node-1' });

    // Toggle again to deselect
    expect(AppState.togglePinned(appState, 'node', 'node-1')).toBe(false);
    expect(AppState.getPinned(appState)).toBeNull();
  });
});
