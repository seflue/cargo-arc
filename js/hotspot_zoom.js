// @module HotspotZoom
// @deps HotspotTree
// @config
// hotspot_zoom.js - The zoom: which circle is the target, and the animated
// view (x, y, radius) that fills the canvas with it. Names and values kept
// from the prototype (`proto/hotspot-map` @
// `b4a3557d2f463a1f1c26da79fbb5d019ce55f7ea`, `proto/hotspots/page.html`,
// PURE-BEGIN..PURE-END).

const ZOOM_MS = 300; // duration of the zoom animation
const ZOOM_MARGIN = 1.04; // the zoom target fills the canvas up to this factor of its radius

/** @param {number} t - progress in [0, 1] */
function easeInOut(t) {
  return t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
}

/**
 * The view (in the SVG's own units) that fills the canvas with `node`.
 * @param {{ cx: number, cy: number, r: number }} node
 */
function viewFor(node) {
  return { x: node.cx, y: node.cy, radius: node.r * ZOOM_MARGIN };
}

/**
 * @param {{ x: number, y: number, radius: number }} from
 * @param {{ x: number, y: number, radius: number }} to
 * @param {number} t - eased progress in [0, 1]
 */
function lerpView(from, to, t) {
  return {
    x: from.x + (to.x - from.x) * t,
    y: from.y + (to.y - from.y) * t,
    radius: from.radius + (to.radius - from.radius) * t,
  };
}

/**
 * The view at `elapsedMs` into a `from` -> `to` zoom, and whether the
 * animation has reached its end.
 * @param {{ x: number, y: number, radius: number }} from
 * @param {{ x: number, y: number, radius: number }} to
 * @param {number} elapsedMs
 */
function viewBoxAt(from, to, elapsedMs) {
  const raw = Math.min(1, Math.max(0, elapsedMs / ZOOM_MS));
  return {
    view: lerpView(from, to, easeInOut(raw)),
    done: elapsedMs >= ZOOM_MS,
  };
}

/**
 * Where a click should zoom to. `clickedKey` is the node under the pointer,
 * or `null`/`undefined` for a click that hit no node. A container zooms in;
 * re-clicking the current target zooms out to its parent (or stays at the
 * root); a leaf returns the unchanged target - HotspotSelection.clickAction
 * turns a leaf click into a selection instead of calling this.
 * @param {Record<string, {kind: string, parent?: string}>} nodes
 * @param {string} targetKey
 * @param {string | null | undefined} clickedKey
 * @returns {string}
 */
function nextZoomTarget(nodes, targetKey, clickedKey) {
  const parentOrRoot = (key) =>
    nodes[key]?.parent ?? HotspotTree.rootKey(nodes);
  if (clickedKey == null) return parentOrRoot(targetKey);
  const clicked = nodes[clickedKey];
  if (!clicked || clicked.kind === 'file') return targetKey;
  return clickedKey === targetKey ? parentOrRoot(targetKey) : clickedKey;
}

// The browser global exposing this module's API.
const HotspotZoom = {
  ZOOM_MS,
  ZOOM_MARGIN,
  easeInOut,
  viewFor,
  lerpView,
  viewBoxAt,
  nextZoomTarget,
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = {
    ZOOM_MS,
    ZOOM_MARGIN,
    easeInOut,
    viewFor,
    lerpView,
    viewBoxAt,
    nextZoomTarget,
    HotspotZoom,
  };
}
