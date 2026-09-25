import { describe, expect, test } from 'bun:test';
import {
  apply,
  computeLayout,
  MIN_MAP_AREA_SIZE,
  sidebarWidthFor,
} from './hotspot_layout.js';

const CONSTANTS = {
  sidebarWidth: 280,
  sidebarGap: 20,
  sidebarMarginRight: 16,
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

  test('the sidebar sits at its fixed width, a margin away from the right edge, from below the toolbar to the bottom', () => {
    const layout = computeLayout({ width: 1600, height: 900 }, CONSTANTS);
    expect(layout.sidebar).toEqual({ x: 1304, y: 40, width: 280, height: 860 });
  });

  test('the map area is the largest square left over once the toolbar, the sidebar and its gap, and a margin on every side are taken out', () => {
    // free width = 1600 - 20 (gap) - 280 (sidebar) - 16 (right margin) - 2*20 (margin) = 1244
    // free height = 900 - 40 (toolbar) - 2*20 (margin) = 820
    // the square is bounded by the smaller of the two
    const layout = computeLayout({ width: 1600, height: 900 }, CONSTANTS);
    expect(layout.mapAreaSize).toBe(820);
  });

  test('a measured sidebar width replaces the constant, and the map square shrinks to match', () => {
    const layout = computeLayout({ width: 1600, height: 900 }, CONSTANTS, 700);
    expect(layout.sidebar).toEqual({ x: 884, y: 40, width: 700, height: 860 });
    // free width = 1600 - 20 - 700 - 16 - 2*20 = 824, free height = 820
    expect(layout.mapAreaSize).toBe(820);
    const narrower = computeLayout(
      { width: 1600, height: 900 },
      CONSTANTS,
      800,
    );
    // free width = 1600 - 20 - 800 - 16 - 2*20 = 724
    expect(narrower.mapAreaSize).toBe(724);
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

describe('sidebarWidthFor', () => {
  test('the sidebar takes its content width between the minimum and half the box', () => {
    expect(sidebarWidthFor(420, 1600, 280)).toBe(420);
    expect(sidebarWidthFor(200, 1600, 280)).toBe(280);
    expect(sidebarWidthFor(1000, 1600, 280)).toBe(800);
  });

  test('a content width that could not be measured keeps the minimum', () => {
    expect(sidebarWidthFor(undefined, 1600, 280)).toBe(280);
    expect(sidebarWidthFor(0, 1600, 280)).toBe(280);
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
    expect(targets.sidebarFo.attrs.x).toBe('1304');
    expect(targets.sidebarFo.attrs.width).toBe('280');
    expect(targets.sidebarFo.attrs.height).toBe('860');
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
