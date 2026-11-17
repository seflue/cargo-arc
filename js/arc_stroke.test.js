import { afterAll, beforeEach, describe, expect, test } from 'bun:test';

if (!globalThis.STATIC_DATA) globalThis.STATIC_DATA = {};
if (!globalThis.STATIC_DATA.classes) globalThis.STATIC_DATA.classes = {};
globalThis.STATIC_DATA.classes.direction = {
  ...globalThis.STATIC_DATA.classes.direction,
  depArc: 'dep-arc',
  cycleArc: 'cycle-arc',
  virtualArc: 'virtual-arc',
  hairOuter: 'hair-outer',
};
globalThis.STATIC_DATA.classes.relation = {
  ...globalThis.STATIC_DATA.classes.relation,
  shadowPath: 'shadow-path',
};

import { ArcStroke } from './arc_stroke.js';
import { DomAdapter } from './dom_adapter.js';
import { createFakeElement, createMockDomAdapter } from './dom_mock.js';
import { restoreAll, substitute } from './test_support.js';

afterAll(restoreAll);

// A downward arc over about 200 rows, long enough for several pieces.
const longArc = 'M 100,0 Q 300,0 300,2500 Q 300,5000 100,5000';
const shortArc = 'M 100,0 Q 130,0 130,20 Q 130,40 100,40';

let dom;
beforeEach(() => {
  dom = createMockDomAdapter();
  substitute(DomAdapter, dom);
});

describe('ArcStroke.draw', () => {
  test('a thin arc becomes one path with a subpath per piece that takes the width of the group', () => {
    const group = createFakeElement('g');

    ArcStroke.draw(group, longArc, 0.5, 1);

    expect(group.getAttribute('data-path')).toBe(longArc);
    expect(group.style.strokeWidth).toBe('0.5px');
    expect(group.children).toHaveLength(1);
    const [line] = group.children;
    expect(line.tagName).toBe('path');
    expect(line.style.strokeWidth).toBeUndefined();
    expect(line.classList.contains('hair-outer')).toBe(false);
    expect(line.getAttribute('d')).toStartWith('M 100,0 L ');
    expect(line.getAttribute('d').match(/M/g).length).toBeGreaterThan(1);
  });

  test('a long wide arc draws the middle hairlines of all pieces in one path and the outer ones in another', () => {
    const group = createFakeElement('g');

    ArcStroke.draw(group, longArc, 2.5, 1.6);

    expect(group.children).toHaveLength(2);
    const [middle, outer] = group.children;
    const pieces = middle.getAttribute('d').match(/M/g).length;
    expect(pieces).toBeGreaterThan(1);
    // Four hairlines: the middle one and three outer ones per piece.
    expect(outer.getAttribute('d').match(/M/g)).toHaveLength(3 * pieces);
    expect(outer.classList.contains('hair-outer')).toBe(true);
    expect(middle.style.strokeWidth).toBe('0.625px');
    expect(outer.style.strokeWidth).toBe('0.625px');
  });

  test('a wide arc draws a middle and an outer hairline per piece, one device pixel each', () => {
    const group = createFakeElement('g');

    ArcStroke.draw(group, shortArc, 2.5, 1.6);

    expect(group.style.strokeWidth).toBe('2.5px');
    expect(group.children).toHaveLength(2);
    const [middle, outer] = group.children;
    expect(middle.classList.contains('hair-outer')).toBe(false);
    expect(outer.classList.contains('hair-outer')).toBe(true);
    expect(middle.style.strokeWidth).toBe('0.625px');
    expect(outer.style.strokeWidth).toBe('0.625px');
    // Three outer hairlines, one subpath each.
    expect(outer.getAttribute('d').match(/M/g)).toHaveLength(3);
  });

  test('a browser that antialiases whole curves gets the curve as one path', () => {
    DomAdapter.antialiasesWholeCurves = () => true;
    const group = createFakeElement('g');

    ArcStroke.draw(group, longArc, 2.5, 1.6);

    expect(group.getAttribute('data-path')).toBe(longArc);
    expect(group.style.strokeWidth).toBe('2.5px');
    expect(group.children).toHaveLength(1);
    expect(group.children[0].getAttribute('d')).toBe(longArc);
    expect(group.children[0].style.strokeWidth).toBeUndefined();
  });

  test('drawing again replaces the earlier pieces', () => {
    const group = createFakeElement('g');
    group.appendChild(createFakeElement('path'));

    ArcStroke.draw(group, shortArc, 0.5, 1);
    ArcStroke.draw(group, shortArc, 0.5, 1);

    expect(group.children).toHaveLength(1);
    expect(group.children[0].getAttribute('d')).toStartWith('M 100,0 L ');
  });
});

describe('ArcStroke.redraw', () => {
  test('draws the curve the group holds at the new width', () => {
    const group = createFakeElement('g');
    ArcStroke.draw(group, shortArc, 0.5, 1);

    ArcStroke.redraw(group, 2, 1);

    expect(group.style.strokeWidth).toBe('2px');
    expect(group.children).toHaveLength(2);
  });
});

describe('ArcStroke.redrawAll', () => {
  test('draws every arc and virtual arc at its own width for the new density', () => {
    const thin = createFakeElement('g');
    const wide = createFakeElement('g');
    ArcStroke.draw(thin, shortArc, 0.5, 1);
    ArcStroke.draw(wide, shortArc, 2, 1);
    dom._registerSelector('.dep-arc, .cycle-arc, .virtual-arc', [thin, wide]);

    ArcStroke.redrawAll(2);

    // 0.5 px at density 2 is one line, 2 px are four.
    expect(thin.children).toHaveLength(1);
    expect(thin.style.strokeWidth).toBe('0.5px');
    const outer = wide.children[1];
    expect(outer.getAttribute('d').match(/M/g)).toHaveLength(3);
    expect(outer.style.strokeWidth).toBe('0.5px');
  });

  test('leaves the shadow of a highlighted arc as it is', () => {
    const shadow = createFakeElement('path');
    shadow.classList.add('dep-arc', 'shadow-path');
    shadow.setAttribute('d', shortArc);
    shadow.style.strokeWidth = '6px';
    dom._registerSelector('.dep-arc, .cycle-arc, .virtual-arc', [shadow]);

    ArcStroke.redrawAll(2);

    expect(shadow.children).toHaveLength(0);
    expect(shadow.getAttribute('data-path')).toBeNull();
  });
});
