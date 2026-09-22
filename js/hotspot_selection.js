// @module HotspotSelection
// @deps HotspotZoom
// @config
// hotspot_selection.js - What a click on the map does, and the leaf node key
// for a workspace-relative file (an editor follow event, or `?select`).

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

// The browser global exposing this module's API.
const HotspotSelection = { leafKeyForFile, clickAction };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = {
    leafKeyForFile,
    clickAction,
    HotspotSelection,
  };
}
