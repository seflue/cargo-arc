// @module HotspotSelection
// @deps HotspotZoom
// @config
// hotspot_selection.js - What a click on the map does, the leaf node key for
// a workspace-relative file (an editor follow event), and where such a file
// resolves to when it arrives from the arc page's own toolbar link.

/**
 * The leaf node key for a workspace-relative file, or `null` when no leaf
 * carries it. Never a container: a container and its own self-leaf can
 * share a file, and only the leaf is `kind: 'file'`.
 * @param {Record<string, {kind: string, file: string}>} nodes
 * @param {string} file
 * @returns {string | null}
 */
function leafKeyForFile(nodes, file) {
  const found = Object.entries(nodes).find(
    ([, node]) => node.kind === 'file' && node.file === file,
  );
  return found ? found[0] : null;
}

/**
 * What a click on the map should do: a leaf selects itself, a container or
 * empty space zooms, following `HotspotZoom.nextZoomTarget`'s targeting
 * rules.
 * @param {Record<string, {kind: string, parent?: string}>} nodes
 * @param {string} targetKey - the current zoom target
 * @param {string | null | undefined} clickedKey - the node under the pointer
 * @returns {{ type: 'select', key: string } | { type: 'zoom', key: string } | { type: 'none' }}
 */
function clickAction(nodes, targetKey, clickedKey) {
  if (clickedKey != null && nodes[clickedKey]?.kind === 'file') {
    return { type: 'select', key: clickedKey };
  }
  const next = HotspotZoom.nextZoomTarget(nodes, targetKey, clickedKey);
  return next === targetKey ? { type: 'none' } : { type: 'zoom', key: next };
}

/**
 * Resolve a file arriving via `?select=`: zoom into a container, focus a
 * leaf. A container wins over its own leaf, since the arc node is the module.
 * @param {Record<string, {kind: string, file: string}>} nodes
 * @param {string} file
 * @returns {{ type: 'zoom', key: string } | { type: 'focus', key: string } | null}
 */
function placeForFile(nodes, file) {
  let leafKey = null;
  for (const [key, node] of Object.entries(nodes)) {
    if (node.file !== file) continue;
    if (node.kind !== 'file') return { type: 'zoom', key };
    leafKey = key;
  }
  return leafKey === null ? null : { type: 'focus', key: leafKey };
}

// The browser global exposing this module's API.
const HotspotSelection = { leafKeyForFile, clickAction, placeForFile };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = {
    leafKeyForFile,
    clickAction,
    placeForFile,
    HotspotSelection,
  };
}
