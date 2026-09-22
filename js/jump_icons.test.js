import { describe, expect, test } from 'bun:test';
import { createFakeElement } from './dom_adapter.js';
import { createJumpIcons } from './jump_icons.js';
import { JumpSymbol } from './jump_symbol.js';

// Fake elements support the subset of the DOM createJumpIcons touches, plus
// addEventListener/_fire so tests can trigger the handlers it attaches
// (pattern: makeBadgeMock in sidebar.test.js).
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
global.JumpSymbol = JumpSymbol;

function makeRect(x, y, width, height) {
  const el = createFakeElement('rect');
  el.setAttribute('x', x);
  el.setAttribute('y', y);
  el.setAttribute('width', width);
  el.setAttribute('height', height);
  return el;
}

const TARGETS = [
  { kind: 'lib', name: 'lib.rs', jump: 1 },
  { kind: 'bin', name: 'tool.rs', jump: 2 },
  { kind: 'manifest', name: 'Cargo.toml', jump: 3 },
];

function makeJumpIcons(overrides = {}) {
  const layer = createFakeSvgElement('g');
  const defsHost = createFakeSvgElement('svg');
  const jumpIcons = createJumpIcons({
    layer,
    defsHost,
    onJump: () => {},
    onEnter: () => {},
    onLeave: () => {},
    setTimeout: () => 0,
    clearTimeout: () => {},
    grace: 90,
    ...overrides,
  });
  return { layer, defsHost, jumpIcons };
}

function chipsOf(layer) {
  return layer.children[0].children.filter(
    (c) => c.getAttribute('class') === 'jump-chip',
  );
}

describe('createJumpIcons', () => {
  test('defines the #jump-icon symbol at construction, once', () => {
    const { defsHost, jumpIcons } = makeJumpIcons();

    expect(defsHost.children.length).toBe(1);
    const symbol = defsHost.children[0].children[0];
    expect(symbol.tagName).toBe('symbol');
    expect(symbol.getAttribute('id')).toBe('jump-icon');

    jumpIcons.show('node-1', makeRect(0, 0, 100, 30), TARGETS);
    expect(defsHost.children.length).toBe(1);
  });

  test('show builds a popover with one chip per target, labelled by kind', () => {
    const { layer, jumpIcons } = makeJumpIcons();

    jumpIcons.show('node-1', makeRect(0, 0, 100, 30), TARGETS);

    expect(layer.children.length).toBe(1);
    expect(layer.children[0].getAttribute('class')).toBe('jump-popover');
    const chips = chipsOf(layer);
    expect(chips.length).toBe(3);
    const labels = chips.map(
      (c) => c.children.find((e) => e.tagName === 'text').textContent,
    );
    expect(labels).toEqual(['lib', 'bin', 'toml']);
    const titles = chips.map(
      (c) => c.children.find((e) => e.tagName === 'title').textContent,
    );
    expect(titles).toEqual(['lib.rs', 'tool.rs', 'Cargo.toml']);
  });

  test('a module target reads mod', () => {
    const { layer, jumpIcons } = makeJumpIcons();

    jumpIcons.show('node-1', makeRect(0, 0, 100, 30), [
      { kind: 'module', name: 'foo.rs', jump: 7 },
    ]);

    const [chip] = chipsOf(layer);
    expect(chip.children.find((e) => e.tagName === 'text').textContent).toBe(
      'mod',
    );
  });

  test('the popover starts at the rect edge, bridged so the pointer crosses no gap', () => {
    const { layer, jumpIcons } = makeJumpIcons();

    jumpIcons.show('node-1', makeRect(10, 20, 100, 24), TARGETS);

    const group = layer.children[0];
    const bridge = group.children.find(
      (c) => c.getAttribute('class') === 'jump-popover jump-popover-bridge',
    );
    const bg = group.children.find(
      (c) => c.getAttribute('class') === 'jump-popover jump-popover-bg',
    );
    expect(Number(bridge.getAttribute('x'))).toBe(110);
    expect(Number(bridge.getAttribute('y'))).toBe(20);
    expect(Number(bridge.getAttribute('height'))).toBe(24);
    expect(Number(bg.getAttribute('x'))).toBe(
      110 + Number(bridge.getAttribute('width')),
    );
    expect(Number(bg.getAttribute('y'))).toBe(20);
    expect(Number(bg.getAttribute('height'))).toBe(24);
    // chips sit inside the background, left to right, in target order
    const chipRects = chipsOf(layer).map((c) =>
      c.children.find((e) => e.tagName === 'rect'),
    );
    const xs = chipRects.map((r) => Number(r.getAttribute('x')));
    expect(xs[0]).toBeGreaterThan(Number(bg.getAttribute('x')));
    expect(xs[1]).toBeGreaterThan(xs[0]);
    expect(xs[2]).toBeGreaterThan(xs[1]);
    const last = chipRects[2];
    expect(
      Number(last.getAttribute('x')) + Number(last.getAttribute('width')),
    ).toBeLessThan(
      Number(bg.getAttribute('x')) + Number(bg.getAttribute('width')),
    );
  });

  test('clicking a chip calls onJump with its jump id and stops propagation', () => {
    const jumped = [];
    const { layer, jumpIcons } = makeJumpIcons({
      onJump: (id) => jumped.push(id),
    });
    jumpIcons.show('node-1', makeRect(0, 0, 100, 30), TARGETS);

    let stopped = false;
    chipsOf(layer)[1]._fire('click', {
      stopPropagation: () => {
        stopped = true;
      },
    });

    expect(jumped).toEqual([2]);
    expect(stopped).toBe(true);
  });

  test('entering the popover cancels the hide and re-enters the node hover', () => {
    const entered = [];
    let scheduled = null;
    const { layer, jumpIcons } = makeJumpIcons({
      onEnter: (id) => entered.push(id),
      setTimeout: (fn) => {
        scheduled = fn;
        return 1;
      },
      clearTimeout: () => {
        scheduled = null;
      },
    });
    jumpIcons.show('node-1', makeRect(0, 0, 100, 30), TARGETS);
    jumpIcons.scheduleHide();

    layer.children[0]._fire('mouseenter');

    expect(scheduled).toBeNull();
    expect(entered).toEqual(['node-1']);
  });

  test('scheduleHide removes the group only once the timer elapses; cancelHide prevents it', () => {
    let scheduled = null;
    const { layer, jumpIcons } = makeJumpIcons({
      setTimeout: (fn) => {
        scheduled = fn;
        return 1;
      },
      clearTimeout: () => {
        scheduled = null;
      },
    });
    jumpIcons.show('node-1', makeRect(0, 0, 100, 30), TARGETS);

    jumpIcons.scheduleHide();
    expect(layer.children.length).toBe(1);
    scheduled();
    expect(layer.children.length).toBe(0);

    jumpIcons.show('node-1', makeRect(0, 0, 100, 30), TARGETS);
    jumpIcons.scheduleHide();
    jumpIcons.cancelHide();
    expect(scheduled).toBeNull();
    expect(layer.children.length).toBe(1);
  });

  test('a second show replaces the first group', () => {
    const { layer, jumpIcons } = makeJumpIcons();

    jumpIcons.show('node-1', makeRect(0, 0, 100, 30), TARGETS);
    const firstGroup = layer.children[0];
    jumpIcons.show('node-2', makeRect(0, 40, 100, 30), TARGETS.slice(0, 1));

    expect(layer.children.length).toBe(1);
    expect(layer.children[0]).not.toBe(firstGroup);
  });
});
