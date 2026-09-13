import { describe, expect, test } from 'bun:test';

// Provide STATIC_DATA.classes for selectors (normally injected by render.rs)
globalThis.STATIC_DATA = globalThis.STATIC_DATA || {};
globalThis.STATIC_DATA.classes = globalThis.STATIC_DATA.classes || {};
Object.assign(globalThis.STATIC_DATA.classes, {
  depArc: 'dep-arc',
  cycleArc: 'cycle-arc',
  virtualArc: 'virtual-arc',
  arcHitarea: 'arc-hitarea',
  arcCount: 'arc-count',
  arcCountGroup: 'arc-count-group',
  arcCountBg: 'arc-count-bg',
  collapseToggle: 'collapse-toggle',
  virtualHitarea: 'virtual-hitarea',
  virtualArrow: 'virtual-arrow',
  depArrow: 'dep-arrow',
  cycleArrow: 'cycle-arrow',
});

import { Selectors } from './selectors.js';

// Set Selectors globally (simulating browser environment where it's loaded before dom_adapter.js)
global.Selectors = Selectors;

import { createFakeElement, createMockDomAdapter } from './dom_adapter.js';

describe('createFakeElement', () => {
  test('setAttribute/getAttribute roundtrip', () => {
    const el = createFakeElement('rect');
    el.setAttribute('id', 'my-rect');
    el.setAttribute('width', '100');
    expect(el.getAttribute('id')).toBe('my-rect');
    expect(el.getAttribute('width')).toBe('100');
  });

  test('classList.add/contains/remove', () => {
    const el = createFakeElement('g');
    expect(el.classList.contains('active')).toBe(false);
    el.classList.add('active');
    expect(el.classList.contains('active')).toBe(true);
    el.classList.remove('active');
    expect(el.classList.contains('active')).toBe(false);
  });

  test('style property get/set', () => {
    const el = createFakeElement('path');
    el.style.strokeWidth = '5px';
    el.style.fill = 'red';
    expect(el.style.strokeWidth).toBe('5px');
    expect(el.style.fill).toBe('red');
  });

  test('appendChild/removeChild', () => {
    const parent = createFakeElement('g');
    const child = createFakeElement('rect');
    parent.appendChild(child);
    expect(parent.children).toContain(child);
    parent.removeChild(child);
    expect(parent.children).not.toContain(child);
  });
});

describe('createMockDomAdapter', () => {
  test('getElementById tracks calls', () => {
    const mock = createMockDomAdapter();
    mock.getElementById('foo');
    mock.getElementById('bar');
    expect(mock._getCalls('getElementById')).toEqual([['foo'], ['bar']]);
  });

  test('_registerElement makes getElementById return element', () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement('g');
    mock._registerElement('my-id', el);
    expect(mock.getElementById('my-id')).toBe(el);
  });

  test('querySelector, querySelectorAll, createSvgElement track calls', () => {
    const mock = createMockDomAdapter();
    mock.querySelector('.node');
    mock.querySelectorAll('rect');
    const svgEl = mock.createSvgElement('path');
    expect(mock._getCalls('querySelector')).toEqual([['.node']]);
    expect(mock._getCalls('querySelectorAll')).toEqual([['rect']]);
    expect(mock._getCalls('createSvgElement')).toEqual([['path']]);
    expect(svgEl.tagName).toBe('path');
  });
});

describe('Convenience methods', () => {
  test('getNode uses Selectors.nodeId', () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement('rect');
    mock._registerElement('node-foo', el);
    expect(mock.getNode('foo')).toBe(el);
    expect(mock._getCalls('getElementById')).toContainEqual(['node-foo']);
  });

  test('getVisibleArc uses Selectors.baseArc', () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement('path');
    mock._registerSelector(Selectors.baseArc('a-b'), el);
    expect(mock.getVisibleArc('a-b')).toBe(el);
    expect(mock._getCalls('querySelector')).toContainEqual([
      Selectors.baseArc('a-b'),
    ]);
  });

  test('getHitarea uses Selectors.hitarea', () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement('path');
    mock._registerSelector(Selectors.hitarea('x-y'), el);
    expect(mock.getHitarea('x-y')).toBe(el);
    expect(mock._getCalls('querySelector')).toContainEqual([
      Selectors.hitarea('x-y'),
    ]);
  });

  test('getArrows uses Selectors.arrows', () => {
    const mock = createMockDomAdapter();
    const arrows = [createFakeElement('polygon'), createFakeElement('polygon')];
    mock._registerSelector(Selectors.arrows('e-id'), arrows);
    expect(mock.getArrows('e-id')).toEqual(arrows);
    expect(mock._getCalls('querySelectorAll')).toContainEqual([
      Selectors.arrows('e-id'),
    ]);
  });

  test('getVirtualArrows uses Selectors.virtualArrows', () => {
    const mock = createMockDomAdapter();
    const arrows = [createFakeElement('polygon')];
    mock._registerSelector(Selectors.virtualArrows('v-id'), arrows);
    expect(mock.getVirtualArrows('v-id')).toEqual(arrows);
  });

  test('getLabelGroup uses Selectors.labelGroup', () => {
    const mock = createMockDomAdapter();
    const group = createFakeElement('g');
    mock._registerSelector(Selectors.labelGroup('arc-1'), group);
    expect(mock.getLabelGroup('arc-1')).toBe(group);
  });

  test('getCollapseToggle uses Selectors.collapseToggle', () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement('text');
    mock._registerSelector(Selectors.collapseToggle('myNode'), el);
    expect(mock.getCollapseToggle('myNode')).toBe(el);
    expect(mock._getCalls('querySelector')).toContainEqual([
      Selectors.collapseToggle('myNode'),
    ]);
  });

  test('getCountLabel uses Selectors.countId', () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement('tspan');
    mock._registerElement(Selectors.countId('node1'), el);
    expect(mock.getCountLabel('node1')).toBe(el);
    expect(mock._getCalls('getElementById')).toContainEqual([
      Selectors.countId('node1'),
    ]);
  });

  test('getCycleMarker uses Selectors.cycleMarkerId', () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement('tspan');
    mock._registerElement(Selectors.cycleMarkerId('node1'), el);
    expect(mock.getCycleMarker('node1')).toBe(el);
    expect(mock._getCalls('getElementById')).toContainEqual([
      Selectors.cycleMarkerId('node1'),
    ]);
  });

  test('getTreeLines with role=child uses Selectors.treeLineChild', () => {
    const mock = createMockDomAdapter();
    const lines = [createFakeElement('line'), createFakeElement('line')];
    mock._registerSelector(Selectors.treeLineChild('c1'), lines);
    expect(mock.getTreeLines('c1', 'child')).toEqual(lines);
    expect(mock._getCalls('querySelectorAll')).toContainEqual([
      Selectors.treeLineChild('c1'),
    ]);
  });

  test('getTreeLines with role=parent uses Selectors.treeLineParent', () => {
    const mock = createMockDomAdapter();
    const lines = [createFakeElement('line')];
    mock._registerSelector(Selectors.treeLineParent('p1'), lines);
    expect(mock.getTreeLines('p1', 'parent')).toEqual(lines);
    expect(mock._getCalls('querySelectorAll')).toContainEqual([
      Selectors.treeLineParent('p1'),
    ]);
  });

  test("getSvgRoot uses querySelector('svg')", () => {
    const mock = createMockDomAdapter();
    const svg = createFakeElement('svg');
    mock._registerSelector('svg', svg);
    expect(mock.getSvgRoot()).toBe(svg);
    expect(mock._getCalls('querySelector')).toContainEqual(['svg']);
  });

  test('getAllHitareas uses Selectors.allHitareas', () => {
    const mock = createMockDomAdapter();
    const hitareas = [createFakeElement('path'), createFakeElement('path')];
    mock._registerSelector(Selectors.allHitareas(), hitareas);
    expect(mock.getAllHitareas()).toEqual(hitareas);
    expect(mock._getCalls('querySelectorAll')).toContainEqual([
      Selectors.allHitareas(),
    ]);
  });
});

describe('Arc element cache', () => {
  test('cacheArcElements stores and retrieves arc via getArc', () => {
    const mock = createMockDomAdapter();
    const arcEl = createFakeElement('path');
    mock.cacheArcElements('a-b', arcEl, [], null);
    expect(mock.getArc('a-b')).toBe(arcEl);
    // Should NOT call querySelector — served from cache
    expect(mock._getCalls('querySelector')).toEqual([]);
  });

  test('cacheArcElements stores and retrieves arrows via getArrows', () => {
    const mock = createMockDomAdapter();
    const arr1 = createFakeElement('polygon');
    const arr2 = createFakeElement('polygon');
    mock.cacheArcElements('a-b', null, [arr1, arr2], null);
    expect(mock.getArrows('a-b')).toEqual([arr1, arr2]);
    expect(mock._getCalls('querySelectorAll')).toEqual([]);
  });

  test('cacheArcElements stores and retrieves labelGroup via getLabelGroup', () => {
    const mock = createMockDomAdapter();
    const labelGroup = createFakeElement('g');
    mock.cacheArcElements('a-b', null, [], labelGroup);
    expect(mock.getLabelGroup('a-b')).toBe(labelGroup);
    expect(mock._getCalls('querySelector')).toEqual([]);
  });

  test('getArc falls back to querySelector when arcId not cached', () => {
    const mock = createMockDomAdapter();
    const arcEl = createFakeElement('path');
    mock._registerSelector(Selectors.baseArc('x-y'), arcEl);
    expect(mock.getArc('x-y')).toBe(arcEl);
    expect(mock._getCalls('querySelector')).toContainEqual([
      Selectors.baseArc('x-y'),
    ]);
  });

  test('clearArcCache removes all cached entries', () => {
    const mock = createMockDomAdapter();
    const arcEl = createFakeElement('path');
    mock.cacheArcElements('a-b', arcEl, [], null);
    mock.clearArcCache();
    // After clearing, getArc falls back to querySelector (returns null for unregistered selector)
    expect(mock.getArc('a-b')).toBe(null);
    expect(mock._getCalls('querySelector')).toContainEqual([
      Selectors.baseArc('a-b'),
    ]);
  });

  test('evictArcCache removes specific entry', () => {
    const mock = createMockDomAdapter();
    const arcA = createFakeElement('path');
    const arcB = createFakeElement('path');
    mock.cacheArcElements('a-b', arcA, [], null);
    mock.cacheArcElements('c-d', arcB, [], null);
    mock.evictArcCache('a-b');
    // a-b evicted — falls back to querySelector
    expect(mock.getArc('a-b')).toBe(null);
    // c-d still cached
    expect(mock.getArc('c-d')).toBe(arcB);
  });

  test('cached null arc is returned without querySelector fallback', () => {
    const mock = createMockDomAdapter();
    // Cache entry with arc: null (happens for multi-segment virtual arcs)
    mock.cacheArcElements('v-arc', null, [], null);
    expect(mock.getArc('v-arc')).toBe(null);
    // Must NOT fall back to querySelector — null is a valid cached value
    expect(mock._getCalls('querySelector')).toEqual([]);
  });
});
