import { describe, expect, test } from 'bun:test';
import { capture, restore } from './view_snapshot.js';

/** A crate `app` with module `core` and its child `util`, plus crate `lib`. */
const nodes = {
  0: { name: 'app', parent: null },
  1: { name: 'core', parent: '0' },
  2: { name: 'util', parent: '1' },
  3: { name: 'lib', parent: null },
};

/** The same tree as the next run numbered it: `lib` first, `app` last. */
const renumbered = {
  0: { name: 'lib', parent: null },
  1: { name: 'app', parent: null },
  2: { name: 'core', parent: '1' },
  3: { name: 'util', parent: '2' },
};

const view = {
  collapsed: new Set(['1']),
  selection: { type: 'node', id: '2' },
  checks: { 'crate-dep-checkbox': true, 'reexport-dep-checkbox': false },
  search: { query: 'ut', scope: 'module' },
  follow: false,
  scroll: { x: 0, y: 120 },
};

describe('capture and restore', () => {
  test('round-trips a view on the same nodes', () => {
    const restored = restore(capture(view, nodes), nodes);
    expect(restored).toEqual({
      collapsed: ['1'],
      selection: { type: 'node', id: '2' },
      checks: { 'crate-dep-checkbox': true, 'reexport-dep-checkbox': false },
      search: { query: 'ut', scope: 'module' },
      follow: false,
      scroll: { x: 0, y: 120 },
    });
  });

  test('keys nodes by their names, not their ids', () => {
    const snapshot = capture(view, nodes);
    expect(snapshot.collapsed).toEqual(['app/core']);
    expect(snapshot.selection).toEqual({ type: 'node', key: 'app/core/util' });
    const restored = restore(snapshot, renumbered);
    expect(restored.collapsed).toEqual(['2']);
    expect(restored.selection).toEqual({ type: 'node', id: '3' });
  });

  test('drops a node that the new page no longer has', () => {
    const snapshot = capture(view, nodes);
    const without_util = {
      0: { name: 'app', parent: null },
      1: { name: 'core', parent: '0' },
    };
    const restored = restore(snapshot, without_util);
    expect(restored.collapsed).toEqual(['1']);
    expect(restored.selection).toEqual({ type: null, id: null });
  });

  test('carries an arc selection as its two node keys', () => {
    const snapshot = capture(
      { ...view, selection: { type: 'arc', id: '2-3' } },
      nodes,
    );
    expect(snapshot.selection).toEqual({
      type: 'arc',
      from: 'app/core/util',
      to: 'lib',
    });
    expect(restore(snapshot, renumbered).selection).toEqual({
      type: 'arc',
      id: '3-0',
    });
    expect(
      restore(snapshot, { 0: { name: 'lib', parent: null } }).selection,
    ).toEqual({ type: null, id: null });
  });

  test('an empty selection stays empty', () => {
    const snapshot = capture(
      { ...view, selection: { type: null, id: null } },
      nodes,
    );
    expect(snapshot.selection).toBeNull();
    expect(restore(snapshot, nodes).selection).toEqual({
      type: null,
      id: null,
    });
  });

  test('survives the trip through JSON', () => {
    const snapshot = JSON.parse(JSON.stringify(capture(view, nodes)));
    expect(restore(snapshot, nodes).collapsed).toEqual(['1']);
  });
});
