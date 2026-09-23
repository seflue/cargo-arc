// @module HotspotJumpIcon
// @deps DomAdapter, HotspotLabels, JumpSymbol
// @config
// hotspot_jump_icon.js - The hotspot map's own jump affordance: one glyph
// right after the selected leaf's own label, drawing the `#jump-icon` shape
// `jump_symbol.js` defines, without pulling in jump_icons.js's multi-target
// popover, chips or kind labels - a map leaf always carries exactly one
// target, unlike an arc node. See js/sidebar.js's own inline
// `<svg class="sidebar-jump"><use href="#jump-icon">` for the shape this
// follows.

const ICON_SIZE_EM = 1.1; // the glyph's size, relative to the label's own font size
const ICON_GAP_EM = 0.35; // the gap past the label's right edge, same unit
// Screen-fixed fallback size (`glyphPosition`'s no-label branch): there is
// no font size to scale from once a label is not shown.
const FALLBACK_ICON_SIZE = 14;

/**
 * Where the jump glyph sits, in the outer `<svg>`'s own (unscaled) coordinate
 * space: past the selected leaf's own label, on its baseline, sized from the
 * label's own font size so it stays proportional at every zoom. Falls back
 * to the circle's own edge, at a fixed screen size, when the leaf has no
 * visible label right now (too small at the current zoom, or on the zoom
 * path) - the glyph must stay reachable even without a label to hang off.
 * @param {{ cx: number, cy: number, r: number }} node - the leaf's own
 *   circle, in `g#map-content`'s own (unscaled) coordinate space
 * @param {{ fontSize: number, y: number, text: string } | null} placement -
 *   `HotspotLabels.placeLabels`'s own entry for this leaf
 * @param {number} scale - `g#map-content`'s current zoom scale
 * @param {number} tx
 * @param {number} ty
 * @returns {{ x: number, y: number, size: number }} the `<use>` element's
 *   own `x`/`y` (top-left) and `width`=`height`=`size`, already in screen space
 */
function glyphPosition(node, placement, scale, tx, ty) {
  if (placement) {
    // Labels are `text-anchor: middle`, so the box `HotspotLabels` itself
    // places the label with gives the same right edge the label was drawn
    // at - no separate measurement to keep in sync with the label's own.
    const box = HotspotLabels.labelBox(
      node,
      placement.text,
      placement.fontSize,
      placement.y,
    );
    const rightEdge = box.x + box.width;
    const gap = placement.fontSize * ICON_GAP_EM;
    const size = placement.fontSize * ICON_SIZE_EM * scale;
    return {
      x: tx + (rightEdge + gap) * scale,
      y: ty + placement.y * scale - size / 2,
      size,
    };
  }

  // Upper-right of the circle's own edge, matching the icon's own arrow
  // direction - the same point this glyph always used before it followed
  // the label.
  const angle = -Math.PI / 4;
  const size = FALLBACK_ICON_SIZE;
  return {
    x: tx + (node.cx + node.r * Math.cos(angle)) * scale - size / 2,
    y: ty + (node.cy + node.r * Math.sin(angle)) * scale - size / 2,
    size,
  };
}

/**
 * @param {{ layer: Element, defsHost: Element, onJump: (id: number) => void, iconClass: string }} deps -
 *   `iconClass` is `STATIC_DATA.classes.jumpIcon` (same idiom as
 *   `HotspotHover.createHoverTooltip`'s `tooltipClass`).
 */
function createHotspotJumpIcon({ layer, defsHost, onJump, iconClass }) {
  JumpSymbol.defineJumpSymbol(defsHost);
  let icon = null;

  function hide() {
    if (icon) {
      layer.removeChild(icon);
      icon = null;
    }
  }

  /**
   * @param {{ x: number, y: number, size: number }} at - `glyphPosition`'s
   *   own result, in the layer's coordinate space
   * @param {{ jump: number, name: string }} target
   */
  function show(at, target) {
    hide();

    const use = DomAdapter.createSvgElement('use');
    use.setAttribute('href', '#jump-icon');
    use.setAttribute('class', iconClass);
    use.setAttribute('width', String(at.size));
    use.setAttribute('height', String(at.size));
    use.setAttribute('x', String(at.x));
    use.setAttribute('y', String(at.y));

    const title = DomAdapter.createSvgElement('title');
    title.textContent = target.name;
    use.appendChild(title);

    use.addEventListener('click', (event) => {
      event.stopPropagation?.();
      onJump(target.jump);
    });

    layer.appendChild(use);
    icon = use;
  }

  return { show, hide };
}

// The browser global exposing this module's API.
const HotspotJumpIcon = { createHotspotJumpIcon, glyphPosition };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { createHotspotJumpIcon, glyphPosition, HotspotJumpIcon };
}
