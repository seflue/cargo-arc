// view_snapshot.ts - Carries the page's view state across a reload of a
// recomputed diagram. Node ids are assigned per run, so the snapshot keys
// a node by the chain of names from its root down to it.

interface View {
  /** Ids of the collapsed nodes. */
  collapsed: Iterable<string>;
  /** The pinned selection; an arc id is `${from}-${to}`. */
  selection: { type: 'node' | 'arc' | null; id: string | null };
  /** The view menu's checkboxes by element id. */
  checks: Record<string, boolean>;
  search: { query: string; scope: string };
  follow: boolean;
  scroll: { x: number; y: number };
}

interface Snapshot {
  /** Node keys. */
  collapsed: string[];
  selection:
    | { type: 'node'; key: string }
    | { type: 'arc'; from: string; to: string }
    | null;
  checks: Record<string, boolean>;
  search: { query: string; scope: string };
  follow: boolean;
  scroll: { x: number; y: number };
}

type Nodes = Record<string, { name: string; parent?: string | null }>;

/**
 * The names from the root down to `id`, joined with `/`.
 */
function nodeKey(nodes: Nodes, id: string): string {
  const names = [];
  for (let node = nodes[id]; node; node = nodes[node.parent ?? '']) {
    names.unshift(node.name);
  }
  return names.join('/');
}

/**
 * Every node's id by its key.
 */
function idsByKey(nodes: Nodes): Map<string, string> {
  return new Map(Object.keys(nodes).map((id) => [nodeKey(nodes, id), id]));
}

/**
 * @param nodes - the nodes of the page the view belongs to
 */
function capture(view: View, nodes: Nodes): Snapshot {
  let selection: Snapshot['selection'] = null;
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
 * @param nodes - the nodes of the page being restored
 */
function restore(
  snapshot: Snapshot,
  nodes: Nodes,
): View & { collapsed: string[] } {
  const ids = idsByKey(nodes);
  let selection: { type: 'node' | 'arc' | null; id: string | null } = {
    type: null,
    id: null,
  };
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

const ViewSnapshot = { capture, restore };

export { capture, restore, ViewSnapshot };
