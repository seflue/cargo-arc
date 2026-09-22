import { describe, expect, test } from 'bun:test';
import { HotspotTree } from './hotspot_tree.js';

global.HotspotTree = HotspotTree;

import {
  easeInOut,
  lerpView,
  nextZoomTarget,
  viewBoxAt,
  viewFor,
  ZOOM_MARGIN,
  ZOOM_MS,
} from './hotspot_zoom.js';

describe('easeInOut', () => {
  test('is the identity at the ends', () => {
    expect(easeInOut(0)).toBe(0);
    expect(easeInOut(1)).toBe(1);
  });

  test('passes through the midpoint', () => {
    expect(easeInOut(0.5)).toBe(0.5);
  });
});

describe('viewFor', () => {
  test('centres on the node and pads its radius by ZOOM_MARGIN', () => {
    const view = viewFor({ cx: 12, cy: -4, r: 10 });
    expect(view).toEqual({ x: 12, y: -4, radius: 10 * ZOOM_MARGIN });
  });
});

describe('lerpView', () => {
  const from = { x: 0, y: 0, radius: 10 };
  const to = { x: 10, y: 20, radius: 30 };

  test('t=0 is the start view', () => {
    expect(lerpView(from, to, 0)).toEqual(from);
  });

  test('t=1 is the end view', () => {
    expect(lerpView(from, to, 1)).toEqual(to);
  });

  test('t=0.5 is the midpoint', () => {
    expect(lerpView(from, to, 0.5)).toEqual({ x: 5, y: 10, radius: 20 });
  });
});

describe('viewBoxAt', () => {
  const from = { x: 0, y: 0, radius: 10 };
  const to = { x: 100, y: 0, radius: 50 };

  test('at elapsed 0 the view has not moved and the animation is not done', () => {
    const { view, done } = viewBoxAt(from, to, 0);
    expect(view).toEqual(from);
    expect(done).toBe(false);
  });

  test('at ZOOM_MS the view reaches the target and the animation is done', () => {
    const { view, done } = viewBoxAt(from, to, ZOOM_MS);
    expect(view).toEqual(to);
    expect(done).toBe(true);
  });

  test('past ZOOM_MS stays clamped to the target', () => {
    const { view, done } = viewBoxAt(from, to, ZOOM_MS + 1000);
    expect(view).toEqual(to);
    expect(done).toBe(true);
  });

  test('midway, done is still false', () => {
    const { done } = viewBoxAt(from, to, ZOOM_MS / 2);
    expect(done).toBe(false);
  });
});

describe('nextZoomTarget', () => {
  // root -> a (module, lines 5) -> a1 (file, lines 3)
  //      -> b (file, lines 8)
  function nodes() {
    return {
      root: { name: 'root', kind: 'crate', lines: 10 },
      a: { name: 'a', kind: 'module', parent: 'root', lines: 5 },
      b: { name: 'b', kind: 'file', parent: 'root', lines: 8 },
      a1: { name: 'a1', kind: 'file', parent: 'a', lines: 3 },
    };
  }

  test('clicking a different container zooms to it', () => {
    expect(nextZoomTarget(nodes(), 'root', 'a')).toBe('a');
  });

  test('re-clicking the current container zooms out to its parent', () => {
    expect(nextZoomTarget(nodes(), 'a', 'a')).toBe('root');
  });

  test('re-clicking the root, which has no parent, stays at the root', () => {
    expect(nextZoomTarget(nodes(), 'root', 'root')).toBe('root');
  });

  test('clicking a leaf has no effect on the zoom target', () => {
    expect(nextZoomTarget(nodes(), 'a', 'a1')).toBe('a');
  });

  test('clicking outside any node zooms out to the current target’s parent', () => {
    expect(nextZoomTarget(nodes(), 'a', null)).toBe('root');
  });

  test('clicking outside any node while at the root stays at the root', () => {
    expect(nextZoomTarget(nodes(), 'root', null)).toBe('root');
  });
});
