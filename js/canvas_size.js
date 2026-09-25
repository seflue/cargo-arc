// @module CanvasSize
// @deps
// @config
// canvas_size.js - The arc page's root SVG size while its script runs, and
// the visible page area it and the sidebar are sized against. The rendered
// size fits the diagram alone, which is right for an SVG shown without the
// script (and without the toolbar); in a browser the toolbar spans the SVG,
// so a diagram narrower than the window would wrap it.

/**
 * Return the visible page area in CSS pixels. `window.innerWidth`/`innerHeight`
 * include the scrollbars, so a sidebar clamped to them ends under the
 * vertical scrollbar whenever the page scrolls, as it does in an editor pane,
 * and an SVG widened to them gets a horizontal scrollbar.
 * @returns {{ width: number, height: number }}
 */
function visibleArea() {
  const root = document.documentElement;
  return { width: root.clientWidth, height: root.clientHeight };
}

/**
 * Return the SVG height for a diagram `contentHeight` tall: `toolbarOffset`
 * more, the amount the toolbar's extra rows push the diagram down.
 * @param {number} contentHeight
 * @param {number} toolbarOffset
 * @returns {number}
 */
function svgHeight(contentHeight, toolbarOffset) {
  return contentHeight + toolbarOffset;
}

/**
 * Return the SVG width for a diagram `contentWidth` wide: at least
 * `visibleWidth`, so the toolbar wraps only when the window itself is too
 * narrow.
 * @param {number} contentWidth
 * @param {number} visibleWidth
 * @returns {number}
 */
function svgWidth(contentWidth, visibleWidth) {
  return Math.max(contentWidth, visibleWidth);
}

// The browser global exposing this module's API.
const CanvasSize = { visibleArea, svgHeight, svgWidth };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { CanvasSize };
}
