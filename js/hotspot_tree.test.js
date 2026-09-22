import { describe, expect, test } from 'bun:test';
import {
  ancestorPath,
  childrenOf,
  preorderKeys,
  rootKey,
} from './hotspot_tree.js';

// A tiny STATIC_DATA.nodes fixture: one root, two children (a file with more
// lines than the module, so it must sort first), one grandchild.
function fixture() {
  return {
    root: { name: 'root', kind: 'crate', lines: 10 },
    a: { name: 'a', kind: 'module', parent: 'root', lines: 5 },
    b: { name: 'b', kind: 'file', parent: 'root', lines: 8 },
    a1: { name: 'a1', kind: 'file', parent: 'a', lines: 3 },
  };
}

describe('rootKey', () => {
  test('finds the one node without a parent', () => {
    expect(rootKey(fixture())).toBe('root');
  });
});

describe('childrenOf', () => {
  test('sorts siblings by lines descending', () => {
    const children = childrenOf(fixture());
    expect(children.get('root')).toEqual(['b', 'a']);
  });

  test('breaks a lines tie by name', () => {
    const nodes = {
      root: { name: 'root', lines: 10 },
      y: { name: 'y', parent: 'root', lines: 5 },
      x: { name: 'x', parent: 'root', lines: 5 },
    };
    expect(childrenOf(nodes).get('root')).toEqual(['x', 'y']);
  });

  test('a leaf with no children of its own is absent from the map', () => {
    const children = childrenOf(fixture());
    expect(children.has('a1')).toBe(false);
  });

  test('breaks a lines tie by byte order, matching Rust, not locale-aware case folding', () => {
    const nodes = {
      root: { name: 'root', lines: 10 },
      alpha: { name: 'alpha', parent: 'root', lines: 5 },
      Beta: { name: 'Beta', parent: 'root', lines: 5 },
    };
    // 'B' (0x42) sorts before 'a' (0x61) by byte order, the same order
    // `hotspots::tree::sorted_children`'s `str::cmp` gives; locale-aware
    // `localeCompare` would put 'alpha' first instead.
    expect(childrenOf(nodes).get('root')).toEqual(['Beta', 'alpha']);
  });
});

describe('ancestorPath', () => {
  test('runs from the root down to the node, inclusive', () => {
    expect(ancestorPath(fixture(), 'a1')).toEqual(['root', 'a', 'a1']);
  });

  test('a root node is its own one-element path', () => {
    expect(ancestorPath(fixture(), 'root')).toEqual(['root']);
  });
});

describe('preorderKeys', () => {
  test('visits a container before its children, children by childrenOf order', () => {
    expect(preorderKeys(fixture())).toEqual(['root', 'b', 'a', 'a1']);
  });
});
