// @module ViewSnapshot
// @deps
// @config
// view_snapshot.js - Carries the page's view state across a reload of a
// recomputed diagram. Node ids are assigned per run, so the snapshot keys
// a node by the chain of names from its root down to it.

/**
 * @typedef {object} View
 * @property {Iterable<string>} collapsed - ids of the collapsed nodes
 * @property {{ type: 'node'|'arc'|null, id: string|null }} selection - the
 *   pinned selection; an arc id is `${from}-${to}`
 * @property {Record<string, boolean>} checks - the view menu's checkboxes
 *   by element id
 * @property {{ query: string, scope: string }} search
 * @property {boolean} follow
 * @property {{ x: number, y: number }} scroll
 */

/**
 * @typedef {object} Snapshot
 * @property {string[]} collapsed - node keys
 * @property {{ type: 'node', key: string } | { type: 'arc', from: string, to: string } | null} selection
 * @property {Record<string, boolean>} checks
 * @property {{ query: string, scope: string }} search
 * @property {boolean} follow
 * @property {{ x: number, y: number }} scroll
 */

/** @typedef {Record<string, { name: string, parent?: string | null }>} Nodes */

/**
 * The names from the root down to `id`, joined with `/`.
 * @param {Nodes} nodes
 * @param {string} id
 * @returns {string}
 */
function nodeKey(nodes, id) {
  const names = [];
  for (let node = nodes[id]; node; node = nodes[node.parent ?? '']) {
    names.unshift(node.name);
  }
  return names.join('/');
}

/**
 * Every node's id by its key.
 * @param {Nodes} nodes
 * @returns {Map<string, string>}
 */
function idsByKey(nodes) {
  return new Map(Object.keys(nodes).map((id) => [nodeKey(nodes, id), id]));
}

/**
 * @param {View} view
 * @param {Nodes} nodes - the nodes of the page the view belongs to
 * @returns {Snapshot}
 */
function capture(view, nodes) {
  /** @type {Snapshot['selection']} */
  let selection = null;
  if (view.selection.type === 'node' && view.selection.id !== null) {
    selection = { type: 'node', key: nodeKey(nodes, view.selection.id) };
  } else if (view.selection.type === 'arc' && view.selection.id !== null) {
    const [from, to] = view.selection.id.split('-');
    selection = {
      type: 'arc',
      from: nodeKey(nodes, from),
      to: nodeKey(nodes, to),
    };
  }
  return {
    collapsed: Array.from(view.collapsed, (id) => nodeKey(nodes, id)),
    selection,
    checks: { ...view.checks },
    search: { ...view.search },
    follow: view.follow,
    scroll: { ...view.scroll },
  };
}

/**
 * The parts of `snapshot` whose nodes `nodes` still has, with those nodes'
 * ids; the rest is dropped.
 * @param {Snapshot} snapshot
 * @param {Nodes} nodes - the nodes of the page being restored
 * @returns {View & { collapsed: string[] }}
 */
function restore(snapshot, nodes) {
  const ids = idsByKey(nodes);
  /** @type {{ type: 'node'|'arc'|null, id: string|null }} */
  let selection = { type: null, id: null };
  if (snapshot.selection?.type === 'node') {
    const id = ids.get(snapshot.selection.key);
    if (id !== undefined) selection = { type: 'node', id };
  } else if (snapshot.selection?.type === 'arc') {
    const from = ids.get(snapshot.selection.from);
    const to = ids.get(snapshot.selection.to);
    if (from !== undefined && to !== undefined) {
      selection = { type: 'arc', id: `${from}-${to}` };
    }
  }
  return {
    collapsed: snapshot.collapsed
      .map((key) => ids.get(key))
      .filter((id) => id !== undefined),
    selection,
    checks: { ...snapshot.checks },
    search: { ...snapshot.search },
    follow: snapshot.follow,
    scroll: { ...snapshot.scroll },
  };
}

// The browser global exposing this module's API.
const ViewSnapshot = { capture, restore };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { capture, restore, ViewSnapshot };
}
