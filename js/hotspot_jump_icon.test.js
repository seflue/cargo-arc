import { describe, expect, test } from 'bun:test';
import { createFakeElement } from './dom_adapter.js';
import { HotspotLabels } from './hotspot_labels.js';
import { JumpSymbol } from './jump_symbol.js';

global.HotspotLabels = HotspotLabels;
global.JumpSymbol = JumpSymbol;

import { createHotspotJumpIcon, glyphPosition } from './hotspot_jump_icon.js';

// Fake elements support the subset of the DOM createHotspotJumpIcon touches,
// plus addEventListener/_fire so tests can trigger the click handler it
// attaches (pattern: createFakeSvgElement in jump_icons.test.js).
function createFakeSvgElement(tag) {
  const el = createFakeElement(tag);
  const listeners = new Map();
  el.addEventListener = (evt, fn) => {
    if (!listeners.has(evt)) listeners.set(evt, []);
    listeners.get(evt).push(fn);
  };
  el._fire = (evt, event) => {
    for (const fn of listeners.get(evt) || []) fn(event);
  };
  return el;
}

global.DomAdapter = {
  createSvgElement: (tag) => createFakeSvgElement(tag),
};

function makeHotspotJumpIcon(overrides = {}) {
  const layer = createFakeSvgElement('g');
  const defsHost = createFakeSvgElement('svg');
  const jumpIcon = createHotspotJumpIcon({
    layer,
    defsHost,
    onJump: () => {},
    iconClass: 'hotspot-jump-icon',
    ...overrides,
  });
  return { layer, defsHost, jumpIcon };
}

describe('glyphPosition', () => {
  const node = { cx: 100, cy: 50, r: 20 };

  test("sits past the label's own measured right edge, on its baseline", () => {
    const placement = { fontSize: 10, y: 55, text: 'hot.rs' };
    const scale = 2;
    const tx = 5;
    const ty = 7;

    const at = glyphPosition(node, placement, scale, tx, ty);

    const box = HotspotLabels.labelBox(
      node,
      placement.text,
      placement.fontSize,
      placement.y,
    );
    const rightEdgeOnScreen = tx + (box.x + box.width) * scale;
    expect(at.x).toBeGreaterThan(rightEdgeOnScreen);
    // The label's own y is its baseline; the glyph centres on it.
    expect(at.y + at.size / 2).toBeCloseTo(ty + placement.y * scale, 5);
  });

  test("sizes the glyph from the label's own font size, proportionally", () => {
    const base = { fontSize: 10, y: 0, text: 'a.rs' };
    const doubled = { ...base, fontSize: 20 };
    const scale = 1;

    const at = glyphPosition(node, base, scale, 0, 0);
    const bigger = glyphPosition(node, doubled, scale, 0, 0);

    expect(bigger.size).toBeCloseTo(at.size * 2, 5);
  });

  test('the gap past a longer label tracks its own measured width, not a fixed pixel', () => {
    const short = { fontSize: 10, y: 0, text: 'a' };
    const long = { fontSize: 10, y: 0, text: 'a much longer name.rs' };
    const scale = 1;

    const shortAt = glyphPosition(node, short, scale, 0, 0);
    const longAt = glyphPosition(node, long, scale, 0, 0);

    const shortBox = HotspotLabels.labelBox(
      node,
      short.text,
      short.fontSize,
      short.y,
    );
    const longBox = HotspotLabels.labelBox(
      node,
      long.text,
      long.fontSize,
      long.y,
    );
    // The two right edges differ by exactly the measured width difference
    // (both labels are centred on the same node.cx); the glyph's own x
    // shifts by that same amount, not by some fixed pixel gap.
    const measuredShift = (longBox.width - shortBox.width) / 2;
    expect(longAt.x - shortAt.x).toBeCloseTo(measuredShift, 5);
  });

  test("scales with the zoom (tx/ty/scale), not just the label's own units", () => {
    const placement = { fontSize: 10, y: 0, text: 'a.rs' };

    const at1 = glyphPosition(node, placement, 1, 0, 0);
    const at2 = glyphPosition(node, placement, 2, 100, 50);

    expect(at2.size).toBeCloseTo(at1.size * 2, 5);
    expect(at2.x).not.toBeCloseTo(at1.x, 0);
  });

  test("falls back to the circle's own edge when the leaf has no visible label", () => {
    const at = glyphPosition(node, null, 1, 0, 0);

    expect(Number.isFinite(at.x)).toBe(true);
    expect(Number.isFinite(at.y)).toBe(true);
    expect(at.size).toBeGreaterThan(0);
    // The glyph's own centre sits on the circle's edge, so the fallback
    // stays reachable even with no label to hang off.
    const cx = at.x + at.size / 2;
    const cy = at.y + at.size / 2;
    const distanceFromCentre = Math.hypot(cx - node.cx, cy - node.cy);
    expect(distanceFromCentre).toBeCloseTo(node.r, 5);
  });
});

describe('createHotspotJumpIcon', () => {
  test('defines the #jump-icon symbol at construction, once', () => {
    const { defsHost, jumpIcon } = makeHotspotJumpIcon();

    expect(defsHost.children.length).toBe(1);
    const symbol = defsHost.children[0].children[0];
    expect(symbol.tagName).toBe('symbol');
    expect(symbol.getAttribute('id')).toBe('jump-icon');

    jumpIcon.show({ x: 0, y: 0, size: 14 }, { jump: 1, name: 'lib.rs' });
    expect(defsHost.children.length).toBe(1);
  });

  test('show places one <use> glyph at the given position and size, no popover or chip', () => {
    const { layer, jumpIcon } = makeHotspotJumpIcon();

    jumpIcon.show({ x: 12, y: 34, size: 16 }, { jump: 1, name: 'lib.rs' });

    expect(layer.children.length).toBe(1);
    const [icon] = layer.children;
    expect(icon.tagName).toBe('use');
    expect(icon.getAttribute('href')).toBe('#jump-icon');
    expect(icon.getAttribute('class')).toBe('hotspot-jump-icon');
    expect(icon.getAttribute('x')).toBe('12');
    expect(icon.getAttribute('y')).toBe('34');
    expect(icon.getAttribute('width')).toBe('16');
    expect(icon.getAttribute('height')).toBe('16');
  });

  test('the glyph carries the target name as its title, for hover', () => {
    const { layer, jumpIcon } = makeHotspotJumpIcon();

    jumpIcon.show({ x: 0, y: 0, size: 14 }, { jump: 4, name: 'src/hot.rs' });

    const [icon] = layer.children;
    const title = icon.children.find((c) => c.tagName === 'title');
    expect(title.textContent).toBe('src/hot.rs');
  });

  test('clicking the glyph calls onJump with the target id and stops propagation', () => {
    const jumped = [];
    const { layer, jumpIcon } = makeHotspotJumpIcon({
      onJump: (id) => jumped.push(id),
    });
    jumpIcon.show({ x: 0, y: 0, size: 14 }, { jump: 7, name: 'lib.rs' });

    let stopped = false;
    layer.children[0]._fire('click', {
      stopPropagation: () => {
        stopped = true;
      },
    });

    expect(jumped).toEqual([7]);
    expect(stopped).toBe(true);
  });

  test('a second show replaces the first glyph; hide removes it', () => {
    const { layer, jumpIcon } = makeHotspotJumpIcon();

    jumpIcon.show({ x: 0, y: 0, size: 14 }, { jump: 1, name: 'a.rs' });
    const first = layer.children[0];
    jumpIcon.show({ x: 50, y: 50, size: 5 }, { jump: 2, name: 'b.rs' });

    expect(layer.children.length).toBe(1);
    expect(layer.children[0]).not.toBe(first);

    jumpIcon.hide();
    expect(layer.children.length).toBe(0);
  });
});
