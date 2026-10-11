import { HotspotZoom } from './hotspot_zoom.js';

// hotspot_selection.ts - What a click on the map does, the leaf node key for
// a workspace-relative file (an editor follow event), and where such a file
// resolves to when it arrives from the arc page's own toolbar link.

/**
 * The leaf node key for a workspace-relative file, or `null` when no leaf
 * carries it. Never a container: a container and its own self-leaf can
 * share a file, and only the leaf is `kind: 'file'`.
 */
function leafKeyForFile(
  nodes: Record<string, { kind: string; file: string }>,
  file: string,
): string | null {
  const found = Object.entries(nodes).find(
    ([, node]) => node.kind === 'file' && node.file === file,
  );
  return found ? found[0] : null;
}

/**
 * What a click on the map should do: a leaf selects itself (or deselects
 * itself when already selected), a container or empty space zooms,
 * following `HotspotZoom.nextZoomTarget`'s targeting rules.
 * @param targetKey - the current zoom target
 * @param clickedKey - the node under the pointer
 * @param [selectedKey] - the currently selected leaf
 */
function clickAction(
  nodes: Record<string, { kind: string; parent?: string }>,
  targetKey: string,
  clickedKey: string | null | undefined,
  selectedKey: string | null = null,
):
  | { type: 'select'; key: string | null }
  | { type: 'zoom'; key: string }
  | { type: 'none' } {
  if (clickedKey != null && nodes[clickedKey]?.kind === 'file') {
    return {
      type: 'select',
      key: clickedKey === selectedKey ? null : clickedKey,
    };
  }
  const next = HotspotZoom.nextZoomTarget(nodes, targetKey, clickedKey);
  return next === targetKey ? { type: 'none' } : { type: 'zoom', key: next };
}

/**
 * Resolve a file arriving via `?select=`: zoom into a container, focus a
 * leaf. A container wins over its own leaf, since the arc node is the module.
 */
function placeForFile(
  nodes: Record<string, { kind: string; file: string }>,
  file: string,
): { type: 'zoom'; key: string } | { type: 'focus'; key: string } | null {
  let leafKey = null;
  for (const [key, node] of Object.entries(nodes)) {
    if (node.file !== file) continue;
    if (node.kind !== 'file') return { type: 'zoom', key };
    leafKey = key;
  }
  return leafKey === null ? null : { type: 'focus', key: leafKey };
}

const HotspotSelection = { leafKeyForFile, clickAction, placeForFile };

export { leafKeyForFile, clickAction, placeForFile, HotspotSelection };
