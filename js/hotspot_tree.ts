// hotspot_tree.ts - Derives the hotspot map's hierarchy from STATIC_DATA's
// flat node map (each node carries its own parent key, never a child list).
// Pure: reads only the `nodes` object it is given, never STATIC_DATA or the
// DOM directly, so it works the same for the real page and for a test
// fixture.

/**
 * The one node with no `parent` field: the synthetic workspace root, or a
 * single package's own root when the workspace has only one package.
 */
function rootKey(nodes: Record<string, { parent?: string }>): string {
  const found = Object.keys(nodes).find((key) => nodes[key].parent == null);
  if (found === undefined) throw new Error('hotspot tree has no root node');
  return found;
}

/**
 * Every node's direct children, keyed by parent. Mirrors
 * `hotspots::tree::sorted_children`: lines descending, ties by name, so a
 * container's children come out in the same order `pack()` packed them and
 * `render::hotspots` drew them.
 */
function childrenOf(
  nodes: Record<string, { parent?: string; name: string; lines: number }>,
): Map<string, string[]> {
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
 */
function ancestorPath(
  nodes: Record<string, { parent?: string }>,
  key: string,
): string[] {
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
 */
function preorderKeys(
  nodes: Record<string, { parent?: string; name: string; lines: number }>,
): string[] {
  const children = childrenOf(nodes);
  const order = [];
  const visit = (key) => {
    order.push(key);
    for (const child of children.get(key) ?? []) visit(child);
  };
  visit(rootKey(nodes));
  return order;
}

const HotspotTree = { rootKey, childrenOf, ancestorPath, preorderKeys };

export { rootKey, childrenOf, ancestorPath, preorderKeys, HotspotTree };
