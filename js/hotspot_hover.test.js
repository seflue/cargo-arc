import { describe, expect, test } from 'bun:test';
import { createFakeElement, createMockDomAdapter } from './dom_adapter.js';
import { HotspotTree } from './hotspot_tree.js';
import { TextMeasure } from './text_metrics.js';

global.HotspotTree = HotspotTree;
global.DomAdapter = createMockDomAdapter();
global.TextMeasure = TextMeasure;

import { createHoverTooltip, tooltipRows } from './hotspot_hover.js';

// root (crate) -> lib.rs (file, hotspot, ranked), src (module) -> a.rs, b.rs
function nodes() {
  return {
    root: {
      name: 'app',
      kind: 'crate',
      file: 'app/Cargo.toml',
      lines: 40,
      commits: 9,
    },
    'app/src/lib.rs': {
      name: 'lib.rs',
      kind: 'file',
      file: 'app/src/lib.rs',
      parent: 'root',
      lines: 12,
      commits: 6,
      rank: 1,
    },
    'app/src': {
      name: 'src',
      kind: 'module',
      file: 'app/src/mod.rs',
      parent: 'root',
      lines: 28,
      commits: 7,
    },
    a: {
      name: 'a.rs',
      kind: 'file',
      file: 'app/src/a.rs',
      parent: 'app/src',
      lines: 10,
      commits: 4,
    },
    b: {
      name: 'b.rs',
      kind: 'file',
      file: 'app/src/b.rs',
      parent: 'app/src',
      lines: 8,
      commits: 3,
    },
  };
}

describe('tooltipRows', () => {
  test('a ranked file shows its path, kind, lines, commits and rank', () => {
    const { title, rows } = tooltipRows(nodes(), 'app/src/lib.rs');
    expect(title).toBe('app/src/lib.rs');
    expect(rows).toEqual([
      ['kind', 'file'],
      ['lines', 12],
      ['commits', 6],
      ['rank', '#1'],
    ]);
  });

  test('an unranked file has no rank row', () => {
    const { rows } = tooltipRows(nodes(), 'a');
    expect(rows.some(([label]) => label === 'rank')).toBe(false);
  });

  test('a container shows its child count instead of a rank', () => {
    const { title, rows } = tooltipRows(nodes(), 'app/src');
    expect(title).toBe('app/src/mod.rs');
    expect(rows).toEqual([
      ['kind', 'module'],
      ['lines', 28],
      ['commits', 7],
      ['children', 2],
    ]);
  });

  test('the root falls back to its name when its file is empty', () => {
    const withEmptyRoot = { ...nodes(), root: { ...nodes().root, file: '' } };
    const { title } = tooltipRows(withEmptyRoot, 'root');
    expect(title).toBe('app');
  });
});

describe('createHoverTooltip', () => {
  test('positions the tooltip beside the given pointer point, not a circle', () => {
    const layer = createFakeElement('g');
    const tooltip = createHoverTooltip({ layer, canvasWidth: 1000 });

    tooltip.show({ x: 300, y: 200 }, { title: 'x', rows: [] });

    const [bg] = layer.children[0].children;
    expect(Number(bg.getAttribute('x'))).toBeGreaterThan(300);
  });

  test('flips to the pointer’s left when the right side would cross the canvas edge', () => {
    const layer = createFakeElement('g');
    const tooltip = createHoverTooltip({ layer, canvasWidth: 320 });

    tooltip.show(
      { x: 310, y: 50 },
      { title: 'a-fairly-long-title.rs', rows: [] },
    );

    const [bg] = layer.children[0].children;
    const x = Number(bg.getAttribute('x'));
    const width = Number(bg.getAttribute('width'));
    expect(x).toBeLessThan(310);
    expect(x + width).toBeLessThanOrEqual(320);
  });

  test('a long path fits inside the emitted box width, at the tooltip’s real monospace advance', () => {
    const layer = createFakeElement('g');
    const tooltip = createHoverTooltip({ layer, canvasWidth: 2000 });
    const title = 'app/src/some/deeply/nested/module/path/file.rs';

    tooltip.show({ x: 0, y: 0 }, { title, rows: [] });

    const [bg] = layer.children[0].children;
    const width = Number(bg.getAttribute('width'));
    const measuredWidth = TextMeasure.estimateWidth(title, 11);
    expect(measuredWidth).toBeLessThanOrEqual(width);
  });

  test('setCanvasWidth reclamps a tooltip shown after a resize, not the width captured at creation', () => {
    const layer = createFakeElement('g');
    const tooltip = createHoverTooltip({ layer, canvasWidth: 1000 });

    tooltip.setCanvasWidth(320);
    tooltip.show(
      { x: 310, y: 50 },
      { title: 'a-fairly-long-title.rs', rows: [] },
    );

    const [bg] = layer.children[0].children;
    const x = Number(bg.getAttribute('x'));
    const width = Number(bg.getAttribute('width'));
    expect(x).toBeLessThan(310);
    expect(x + width).toBeLessThanOrEqual(320);
  });

  test('hide removes the tooltip group', () => {
    const layer = createFakeElement('g');
    const tooltip = createHoverTooltip({ layer, canvasWidth: 1000 });

    tooltip.show({ x: 0, y: 0 }, { title: 'x', rows: [] });
    expect(layer.children.length).toBe(1);

    tooltip.hide();
    expect(layer.children.length).toBe(0);
  });
});
