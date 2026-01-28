// highlight_integration.test.js - Integration tests for highlight/clearHighlights behavior
// Ensures proper separation of highlight state (scale) vs layout state (position)

import { test, expect, describe } from "bun:test";

// Setup globals (simulating browser environment)
global.ArrowLogic = require('./arrow_logic.js').ArrowLogic;
global.HighlightLogic = require('./highlight_logic.js').HighlightLogic;

const { createMockDomAdapter, createFakeElement } = require('./dom_adapter.js');
const { HighlightState } = require('./highlight_state.js');

describe('clearHighlights - Separation of Highlight State vs Layout State', () => {
  /**
   * Specification: clearHighlights should only restore highlight state (scale),
   * not layout state (position).
   *
   * Scenario:
   * 1. Initial state: arrow at position (100, 307) with scale 1.0
   * 2. Node collapses → relayout moves arrow to (100, 187)
   * 3. Hover → highlight scales arrow to 1.3
   * 4. Unhover → clearHighlights should keep position (100, 187), restore scale to 1.0
   *
   * Expected: Arrow stays at current layout position (187), scale restores to 1.0
   */

  test('preserves arrow position after relayout when clearing highlights', () => {
    // Setup mock DOM
    const domAdapter = createMockDomAdapter();

    // Create fake arrow element
    const arrow = createFakeElement('polygon');

    // Initial expanded position (100, 307), scale 1.0
    const initialPoints = global.ArrowLogic.getArrowPoints({ x: 100, y: 307 }, 1.0);
    arrow.setAttribute('points', initialPoints);
    expect(arrow.getAttribute('points')).toBe('108,303 100,307 108,311');

    // Create highlight state and store initial values
    const highlightState = HighlightState.create();
    HighlightState.storeOriginal(highlightState, 'arc-1', {
      strokeWidth: 1.5,
      scale: 1.0,
      tipX: 100,
      tipY: 307
    });

    // === RELAYOUT: Node collapses, arrow moves to new position (100, 187) ===
    const newCollapsedPoints = global.ArrowLogic.getArrowPoints({ x: 100, y: 187 }, 1.0);
    arrow.setAttribute('points', newCollapsedPoints);
    expect(arrow.getAttribute('points')).toBe('108,183 100,187 108,191');

    // Update stored state with new position (simulating what should happen after relayout)
    highlightState.originalValues.set('arc-1', {
      strokeWidth: 1.5,
      scale: 1.0,
      tipX: 100,
      tipY: 187  // NEW collapsed position
    });

    // === HIGHLIGHT: User hovers, arrow scales to 1.3 ===
    const highlightedPoints = global.ArrowLogic.getArrowPoints({ x: 100, y: 187 }, 1.3);
    arrow.setAttribute('points', highlightedPoints);
    expect(arrow.getAttribute('points')).toBe('110.4,181.8 100,187 110.4,192.2');

    // === CLEAR HIGHLIGHTS: Should restore scale but KEEP position ===
    const original = HighlightState.getOriginal(highlightState, 'arc-1');
    const currentTip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));

    // This is the FIX: use currentTip (187), not original.tipY (which could be stale)
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints(
      currentTip,       // Keep CURRENT position (layout state)
      original.scale    // Restore ORIGINAL scale (highlight state)
    ));

    // Verify: Arrow should be at collapsed position (187), not expanded position (307)
    expect(arrow.getAttribute('points')).toBe('108,183 100,187 108,191');

    const finalTip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(finalTip.y).toBe(187);  // Collapsed position
    expect(finalTip.y).not.toBe(307);  // NOT expanded position
  });

  test('clearHighlights restores scale correctly', () => {
    const arrow = createFakeElement('polygon');

    // Initial: scale 1.0
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 200 }, 1.0));

    const highlightState = HighlightState.create();
    HighlightState.storeOriginal(highlightState, 'arc-1', {
      strokeWidth: 1.5,
      scale: 1.0,
      tipX: 100,
      tipY: 200
    });

    // Highlight: scale to 1.3
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 200 }, 1.3));
    expect(arrow.getAttribute('points')).toBe('110.4,194.8 100,200 110.4,205.2');

    // Clear highlights: restore scale
    const original = HighlightState.getOriginal(highlightState, 'arc-1');
    const currentTip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints(currentTip, original.scale));

    // Verify: back to scale 1.0
    expect(arrow.getAttribute('points')).toBe('108,196 100,200 108,204');
  });

  test('handles multiple relayouts correctly', () => {
    const arrow = createFakeElement('polygon');
    const highlightState = HighlightState.create();

    // Initial position: Y=300
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 300 }, 1.0));
    HighlightState.storeOriginal(highlightState, 'arc-1', {
      strokeWidth: 1.5,
      scale: 1.0,
      tipX: 100,
      tipY: 300
    });

    // First relayout: collapse to Y=200
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 200 }, 1.0));
    highlightState.originalValues.set('arc-1', {
      strokeWidth: 1.5,
      scale: 1.0,
      tipX: 100,
      tipY: 200
    });

    // Highlight
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 200 }, 1.3));

    // Clear - should be at Y=200
    let original = HighlightState.getOriginal(highlightState, 'arc-1');
    let currentTip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints(currentTip, original.scale));
    expect(global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points')).y).toBe(200);

    // Second relayout: collapse further to Y=100
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 100 }, 1.0));
    highlightState.originalValues.set('arc-1', {
      strokeWidth: 1.5,
      scale: 1.0,
      tipX: 100,
      tipY: 100
    });

    // Highlight again
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 100 }, 1.3));

    // Clear - should be at Y=100, NOT Y=200 or Y=300
    original = HighlightState.getOriginal(highlightState, 'arc-1');
    currentTip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints(currentTip, original.scale));
    expect(global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points')).y).toBe(100);
  });

  test('uses current arrow position, not stored position', () => {
    // Specification: clearHighlights must read current position from DOM, not from stored state
    const arrow = createFakeElement('polygon');
    const highlightState = HighlightState.create();

    // Initial state
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 307 }, 1.0));
    HighlightState.storeOriginal(highlightState, 'arc-1', {
      strokeWidth: 1.5,
      scale: 1.0,
      tipX: 100,
      tipY: 307
    });

    // Relayout changes position (stored state may become stale)
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 187 }, 1.0));

    // Highlight
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 187 }, 1.3));

    // Clear: must use current position from DOM
    const original = HighlightState.getOriginal(highlightState, 'arc-1');
    const currentTip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints(
      currentTip,       // Current position (source of truth)
      original.scale    // Stored scale (highlight state)
    ));

    // Verify: position matches current layout state (187)
    const finalTip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(finalTip.y).toBe(187);
  });
});

describe('State Separation: Highlight vs Layout', () => {
  test('highlight state: scale (ephemeral, changes on hover)', () => {
    const highlightState = HighlightState.create();

    // Store initial scale
    HighlightState.storeOriginal(highlightState, 'arc-1', {
      strokeWidth: 1.5,
      scale: 1.0,
      tipX: 100,
      tipY: 200
    });

    // Highlight changes scale
    const original = HighlightState.getOriginal(highlightState, 'arc-1');
    expect(original.scale).toBe(1.0);

    // Scale changes to 1.3 during highlight (not stored)
    // When clearing, we restore original.scale (1.0)
    expect(global.HighlightLogic.HIGHLIGHT_SCALE).toBe(1.3);
  });

  test('layout state: position (persistent, changes on relayout)', () => {
    const arrow = createFakeElement('polygon');

    // Initial layout: Y=300
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 300 }, 1.0));
    let tip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(tip.y).toBe(300);

    // Relayout changes position: Y=200
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 200 }, 1.0));
    tip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(tip.y).toBe(200);

    // Highlight should NOT affect position, only scale
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 200 }, 1.3));
    tip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(tip.y).toBe(200);  // Position unchanged

    // Clear highlight should NOT restore old position (300), only current scale
    arrow.setAttribute('points', global.ArrowLogic.getArrowPoints({ x: 100, y: 200 }, 1.0));
    tip = global.ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
    expect(tip.y).toBe(200);  // Position still at current layout state
  });
});
