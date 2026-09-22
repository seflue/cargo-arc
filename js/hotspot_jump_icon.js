// @module HotspotJumpIcon
// @deps DomAdapter, JumpSymbol
// @config
// hotspot_jump_icon.js - The hotspot map's own jump affordance: one glyph at
// the selected leaf's edge, drawing the `#jump-icon` shape `jump_symbol.js`
// defines, without pulling in jump_icons.js's multi-target popover, chips or
// kind labels - a map leaf always carries exactly one target, unlike an arc
// node. See js/sidebar.js's own inline
// `<svg class="sidebar-jump"><use href="#jump-icon">` for the shape this
// follows.

const ICON_SIZE = 14; // SVG units, screen-fixed like the sidebar's own icon

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
   * @param {{ cx: number, cy: number, r: number }} circle - the selected
   *   leaf's own drawn circle, in the layer's coordinate space
   * @param {{ jump: number, name: string }} target
   */
  function show(circle, target) {
    hide();
    // Upper-right of the circle's edge, matching the icon's own arrow
    // direction.
    const angle = -Math.PI / 4;
    const x = circle.cx + circle.r * Math.cos(angle) - ICON_SIZE / 2;
    const y = circle.cy + circle.r * Math.sin(angle) - ICON_SIZE / 2;

    const use = DomAdapter.createSvgElement('use');
    use.setAttribute('href', '#jump-icon');
    use.setAttribute('class', iconClass);
    use.setAttribute('width', String(ICON_SIZE));
    use.setAttribute('height', String(ICON_SIZE));
    use.setAttribute('x', String(x));
    use.setAttribute('y', String(y));

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
const HotspotJumpIcon = { createHotspotJumpIcon };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { createHotspotJumpIcon, HotspotJumpIcon };
}
