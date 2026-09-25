import { describe, expect, test } from 'bun:test';
import { HotspotTree } from './hotspot_tree.js';

global.HotspotTree = HotspotTree;

import { HotspotZoom } from './hotspot_zoom.js';

global.HotspotZoom = HotspotZoom;

import {
  clickAction,
  leafKeyForFile,
  placeForFile,
} from './hotspot_selection.js';

// root (crate) -> a (module, own leaf 'a/mod.rs' + child a1) -> a1 (file)
//              -> b (file)
function nodes() {
  return {
    root: { name: 'root', kind: 'crate', file: 'Cargo.toml', lines: 20 },
    a: {
      name: 'a',
      kind: 'module',
      file: 'src/a/mod.rs',
      parent: 'root',
      lines: 10,
    },
    'src/a/mod.rs': {
      name: 'mod.rs',
      kind: 'file',
      file: 'src/a/mod.rs',
      parent: 'a',
      lines: 2,
    },
    a1: {
      name: 'a1.rs',
      kind: 'file',
      file: 'src/a/a1.rs',
      parent: 'a',
      lines: 8,
    },
    b: {
      name: 'b.rs',
      kind: 'file',
      file: 'src/b.rs',
      parent: 'root',
      lines: 8,
    },
  };
}

describe('leafKeyForFile', () => {
  test('finds the leaf carrying the file', () => {
    expect(leafKeyForFile(nodes(), 'src/a/a1.rs')).toBe('a1');
  });

  test('resolves a container’s own file to its self-leaf, not the container', () => {
    expect(leafKeyForFile(nodes(), 'src/a/mod.rs')).toBe('src/a/mod.rs');
  });

  test('is null for a file matching no leaf', () => {
    expect(leafKeyForFile(nodes(), 'src/missing.rs')).toBeNull();
  });
});

describe('clickAction', () => {
  test('a leaf click selects it, without zooming', () => {
    expect(clickAction(nodes(), 'root', 'a1')).toEqual({
      type: 'select',
      key: 'a1',
    });
  });

  test('re-clicking the selected leaf deselects it', () => {
    expect(clickAction(nodes(), 'root', 'a1', 'a1')).toEqual({
      type: 'select',
      key: null,
    });
  });

  test('a leaf click while another leaf is selected selects the clicked one', () => {
    expect(clickAction(nodes(), 'root', 'a1', 'b')).toEqual({
      type: 'select',
      key: 'a1',
    });
  });

  test('a container click zooms to it', () => {
    expect(clickAction(nodes(), 'root', 'a')).toEqual({
      type: 'zoom',
      key: 'a',
    });
  });

  test('re-clicking the current container zooms out to its parent', () => {
    expect(clickAction(nodes(), 'a', 'a')).toEqual({
      type: 'zoom',
      key: 'root',
    });
  });

  test('clicking outside any node zooms to the target’s parent', () => {
    expect(clickAction(nodes(), 'a', null)).toEqual({
      type: 'zoom',
      key: 'root',
    });
  });

  test('clicking outside any node at the root does nothing', () => {
    expect(clickAction(nodes(), 'root', null)).toEqual({ type: 'none' });
  });
});

describe('placeForFile', () => {
  test('a leaf’s file focuses it', () => {
    expect(placeForFile(nodes(), 'src/a/a1.rs')).toEqual({
      type: 'focus',
      key: 'a1',
    });
  });

  test('a container’s own file zooms into it, even though a same-named self-leaf shares it', () => {
    expect(placeForFile(nodes(), 'src/a/mod.rs')).toEqual({
      type: 'zoom',
      key: 'a',
    });
  });

  test('a crate’s own file zooms into it', () => {
    expect(placeForFile(nodes(), 'Cargo.toml')).toEqual({
      type: 'zoom',
      key: 'root',
    });
  });

  test('is null for a file matching no node', () => {
    expect(placeForFile(nodes(), 'src/missing.rs')).toBeNull();
  });
});
