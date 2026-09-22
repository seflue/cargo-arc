// @module HotspotLayout
// @deps
// @config
// hotspot_layout.js - The window-sized layout for the hotspot map: the
// viewBox the root SVG fills, the toolbar and sidebar rects sized to the
// window, and the square area left over for the packed circles. `hotspot_
// script.js` calls `computeLayout` on load and on resize and applies the
// result to the DOM and to the map's own zoom state; the constants come
// from `STATIC_DATA.layout` (`render::hotspots::HotspotLayoutData`), not
// repeated here as literals.

/**
 * @typedef {{ sidebarWidth: number, sidebarGap: number, mapMargin: number, toolbarHeight: number }} HotspotLayoutConstants
 */

/**
 * The smallest `mapAreaSize` a window ever gets, even when the furniture
 * (toolbar, sidebar, margins) no longer fits it. Below this, a zero map
 * area makes `buildHotspotMap` return `null`, and the returned handle is
 * what the page's resize path and its tests call `resize` on - so a window
 * that started this narrow would leave an unrecoverable page.
 */
const MIN_MAP_AREA_SIZE = 40;

/**
 * The layout for a `width` x `height` box (the browser window, or the root
 * SVG's own measured box): `viewBox` fills it exactly (one SVG unit per CSS
 * pixel, so no text scales with the window), the toolbar spans the full
 * width at its fixed height, the sidebar sits at its fixed width against
 * the right edge at the full height, and `mapAreaSize` is the side of the
 * largest square left over once the toolbar strip, the sidebar strip with
 * its gap, and a margin on every side of the square are taken out - never
 * below `MIN_MAP_AREA_SIZE`.
 * @param {{ width: number, height: number }} box
 * @param {HotspotLayoutConstants} constants
 */
function computeLayout({ width, height }, constants) {
  const { sidebarWidth, sidebarGap, mapMargin, toolbarHeight } = constants;
  const freeWidth = width - sidebarGap - sidebarWidth - 2 * mapMargin;
  const freeHeight = height - toolbarHeight - 2 * mapMargin;
  return {
    viewBox: { width, height },
    toolbar: { width, height: toolbarHeight },
    sidebar: { x: width - sidebarWidth, width: sidebarWidth, height },
    mapAreaSize: Math.max(MIN_MAP_AREA_SIZE, Math.min(freeWidth, freeHeight)),
  };
}

/**
 * Writes `layout` onto the root SVG's viewBox (mutating `baseVal` in place,
 * so the DOM's own attribute stays in sync) and onto the toolbar and
 * sidebar foreignObjects - only the attributes that actually move with the
 * window; the sidebar's own width and the toolbar's own height are already
 * fixed by the initial render. Either foreignObject may be absent (a page
 * rendered without jump ids still carries both here, but a caller mid
 * teardown should not have to guard for it).
 * @param {{ svg: { viewBox: { baseVal: { width: number, height: number } } }, toolbarFo: Element | null, sidebarFo: Element | null }} elements
 * @param {ReturnType<typeof computeLayout>} layout
 */
function apply({ svg, toolbarFo, sidebarFo }, layout) {
  svg.viewBox.baseVal.width = layout.viewBox.width;
  svg.viewBox.baseVal.height = layout.viewBox.height;
  if (toolbarFo) toolbarFo.setAttribute('width', String(layout.toolbar.width));
  if (sidebarFo) {
    sidebarFo.setAttribute('x', String(layout.sidebar.x));
    sidebarFo.setAttribute('height', String(layout.sidebar.height));
  }
}

// The browser global exposing this module's API.
const HotspotLayout = { computeLayout, apply, MIN_MAP_AREA_SIZE };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { computeLayout, apply, MIN_MAP_AREA_SIZE, HotspotLayout };
}
