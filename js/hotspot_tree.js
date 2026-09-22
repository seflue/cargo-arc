// @module HotspotTree
// @deps
// @config
// hotspot_tree.js - Derives the hotspot map's hierarchy from STATIC_DATA's
// flat node map (each node carries its own parent key, never a child list).
// Pure: reads only the `nodes` object it is given, never STATIC_DATA or the
// DOM directly, so it works the same for the real page and for a test
// fixture.

/**
 * The one node with no `parent` field: the synthetic workspace root, or a
 * single crate's own root when the workspace has only one crate.
 * @param {Record<string, {parent?: string}>} nodes
 * @returns {string}
 */
function rootKey(nodes) {
  const found = Object.keys(nodes).find((key) => nodes[key].parent == null);
  if (found === undefined) throw new Error('hotspot tree has no root node');
  return found;
}

/**
 * Every node's direct children, keyed by parent. Mirrors
 * `hotspots::tree::sorted_children`: lines descending, ties by name, so a
 * container's children come out in the same order `pack()` packed them and
 * `render::hotspots` drew them.
 * @param {Record<string, {parent?: string, name: string, lines: number}>} nodes
 * @returns {Map<string, string[]>}
 */
function childrenOf(nodes) {
  const byParent = new Map();
  for (const [key, node] of Object.entries(nodes)) {
    if (node.parent == null) continue;
    if (!byParent.has(node.parent)) byParent.set(node.parent, []);
    byParent.get(node.parent).push(key);
  }
  for (const children of byParent.values()) {
    children.sort((a, b) => {
      const byLines = nodes[b].lines - nodes[a].lines;
      if (byLines !== 0) return byLines;
      // Byte order, not `localeCompare`: locale-aware collation and byte
      // order disagree on case, and `hotspots::tree::sorted_children`'s own
      // tiebreak (`str::cmp`) is byte order.
      const nameA = nodes[a].name;
      const nameB = nodes[b].name;
      return nameA < nameB ? -1 : nameA > nameB ? 1 : 0;
    });
  }
  return byParent;
}

/**
 * The node's ancestors from the root down to the node itself, inclusive.
 * @param {Record<string, {parent?: string}>} nodes
 * @param {string} key
 * @returns {string[]}
 */
function ancestorPath(nodes, key) {
  const path = [];
  for (let step = key; step !== undefined; step = nodes[step]?.parent) {
    path.unshift(step);
  }
  return path;
}

/**
 * Every node's key, in preorder from the root: a container before its own
 * children, children in `childrenOf`'s order. The order `render::hotspots`
 * drew its circles in, recovered from data alone.
 * @param {Record<string, {parent?: string, name: string, lines: number}>} nodes
 * @returns {string[]}
 */
function preorderKeys(nodes) {
  const children = childrenOf(nodes);
  const order = [];
  const visit = (key) => {
    order.push(key);
    for (const child of children.get(key) ?? []) visit(child);
  };
  visit(rootKey(nodes));
  return order;
}

// The browser global exposing this module's API.
const HotspotTree = { rootKey, childrenOf, ancestorPath, preorderKeys };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = {
    rootKey,
    childrenOf,
    ancestorPath,
    preorderKeys,
    HotspotTree,
  };
}
