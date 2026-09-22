import { describe, expect, test } from 'bun:test';
import { createFakeElement } from './dom_adapter.js';
import { createHotspotJumpIcon } from './hotspot_jump_icon.js';
import { JumpSymbol } from './jump_symbol.js';

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
global.JumpSymbol = JumpSymbol;

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

describe('createHotspotJumpIcon', () => {
  test('defines the #jump-icon symbol at construction, once', () => {
    const { defsHost, jumpIcon } = makeHotspotJumpIcon();

    expect(defsHost.children.length).toBe(1);
    const symbol = defsHost.children[0].children[0];
    expect(symbol.tagName).toBe('symbol');
    expect(symbol.getAttribute('id')).toBe('jump-icon');

    jumpIcon.show({ cx: 0, cy: 0, r: 10 }, { jump: 1, name: 'lib.rs' });
    expect(defsHost.children.length).toBe(1);
  });

  test('show places one <use> glyph at the circle edge, no popover or chip', () => {
    const { layer, jumpIcon } = makeHotspotJumpIcon();

    jumpIcon.show({ cx: 100, cy: 100, r: 20 }, { jump: 1, name: 'lib.rs' });

    expect(layer.children.length).toBe(1);
    const [icon] = layer.children;
    expect(icon.tagName).toBe('use');
    expect(icon.getAttribute('href')).toBe('#jump-icon');
    expect(icon.getAttribute('class')).toBe('hotspot-jump-icon');
    // Off the circle's centre, toward its edge - not centred on it.
    expect(Number(icon.getAttribute('x'))).not.toBe(100);
    expect(Number(icon.getAttribute('y'))).not.toBe(100);
  });

  test('the glyph carries the target name as its title, for hover', () => {
    const { layer, jumpIcon } = makeHotspotJumpIcon();

    jumpIcon.show({ cx: 0, cy: 0, r: 10 }, { jump: 4, name: 'src/hot.rs' });

    const [icon] = layer.children;
    const title = icon.children.find((c) => c.tagName === 'title');
    expect(title.textContent).toBe('src/hot.rs');
  });

  test('clicking the glyph calls onJump with the target id and stops propagation', () => {
    const jumped = [];
    const { layer, jumpIcon } = makeHotspotJumpIcon({
      onJump: (id) => jumped.push(id),
    });
    jumpIcon.show({ cx: 0, cy: 0, r: 10 }, { jump: 7, name: 'lib.rs' });

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

    jumpIcon.show({ cx: 0, cy: 0, r: 10 }, { jump: 1, name: 'a.rs' });
    const first = layer.children[0];
    jumpIcon.show({ cx: 50, cy: 50, r: 5 }, { jump: 2, name: 'b.rs' });

    expect(layer.children.length).toBe(1);
    expect(layer.children[0]).not.toBe(first);

    jumpIcon.hide();
    expect(layer.children.length).toBe(0);
  });
});
