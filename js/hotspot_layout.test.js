import { describe, expect, test } from 'bun:test';
import { apply, computeLayout, MIN_MAP_AREA_SIZE } from './hotspot_layout.js';

const CONSTANTS = {
  sidebarWidth: 280,
  sidebarGap: 20,
  mapMargin: 20,
  toolbarHeight: 40,
};

describe('computeLayout', () => {
  test('viewBox matches the window exactly, one SVG unit per CSS pixel', () => {
    const layout = computeLayout({ width: 1600, height: 900 }, CONSTANTS);
    expect(layout.viewBox).toEqual({ width: 1600, height: 900 });
  });

  test('the toolbar spans the full window width at its fixed height', () => {
    const layout = computeLayout({ width: 1600, height: 900 }, CONSTANTS);
    expect(layout.toolbar).toEqual({ width: 1600, height: 40 });
  });

  test('the sidebar sits at its fixed width against the right edge, full window height', () => {
    const layout = computeLayout({ width: 1600, height: 900 }, CONSTANTS);
    expect(layout.sidebar).toEqual({ x: 1320, width: 280, height: 900 });
  });

  test('the map area is the largest square left over once the toolbar, the sidebar and its gap, and a margin on every side are taken out', () => {
    // free width = 1600 - 20 (gap) - 280 (sidebar) - 2*20 (margin) = 1260
    // free height = 900 - 40 (toolbar) - 2*20 (margin) = 820
    // the square is bounded by the smaller of the two
    const layout = computeLayout({ width: 1600, height: 900 }, CONSTANTS);
    expect(layout.mapAreaSize).toBe(820);
  });

  test('a window too small for the furniture is floored at the minimum map area, never zero', () => {
    const layout = computeLayout({ width: 100, height: 100 }, CONSTANTS);
    expect(layout.mapAreaSize).toBe(MIN_MAP_AREA_SIZE);
  });

  test('a window narrower than the minimum plus the furniture is still floored, not negative', () => {
    const layout = computeLayout({ width: 0, height: 0 }, CONSTANTS);
    expect(layout.mapAreaSize).toBe(MIN_MAP_AREA_SIZE);
  });
});

describe('apply', () => {
  function fakeLayoutTargets() {
    return {
      svg: { viewBox: { baseVal: { width: 0, height: 0 } } },
      toolbarFo: {
        attrs: {},
        setAttribute(k, v) {
          this.attrs[k] = v;
        },
      },
      sidebarFo: {
        attrs: {},
        setAttribute(k, v) {
          this.attrs[k] = v;
        },
      },
    };
  }

  test('writes the viewBox onto the svg root in place', () => {
    const targets = fakeLayoutTargets();
    apply(targets, computeLayout({ width: 1600, height: 900 }, CONSTANTS));
    expect(targets.svg.viewBox.baseVal).toEqual({ width: 1600, height: 900 });
  });

  test('writes the toolbar width and the sidebar x and height', () => {
    const targets = fakeLayoutTargets();
    apply(targets, computeLayout({ width: 1600, height: 900 }, CONSTANTS));
    expect(targets.toolbarFo.attrs.width).toBe('1600');
    expect(targets.sidebarFo.attrs.x).toBe('1320');
    expect(targets.sidebarFo.attrs.height).toBe('900');
  });

  test('tolerates a missing toolbar or sidebar element (a page rendered without jump ids, or mid-teardown)', () => {
    const targets = {
      svg: fakeLayoutTargets().svg,
      toolbarFo: null,
      sidebarFo: null,
    };
    expect(() =>
      apply(targets, computeLayout({ width: 1600, height: 900 }, CONSTANTS)),
    ).not.toThrow();
  });
});
