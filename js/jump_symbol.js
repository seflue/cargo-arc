// @module JumpSymbol
// @deps DomAdapter
// @config
// jump_symbol.js - Defines the shared `#jump-icon` `<symbol>`: a box with an
// arrow pointing out of its top-right corner. The sidebar rows
// (`js/sidebar.js`), the arc page's popover (`js/jump_icons.js`) and the
// hotspot map's own single icon (`js/hotspot_jump_icon.js`) all reference it
// by id, so whichever page loads must define it before its first use, not
// lazily on first hover. Kept in its own module so a page that needs only
// the glyph (the hotspot map) does not pull in `jump_icons.js`'s popover and
// chip code for it.

/**
 * @param {Element} defsHost
 */
function defineJumpSymbol(defsHost) {
  const defs = DomAdapter.createSvgElement('defs');
  const symbol = DomAdapter.createSvgElement('symbol');
  symbol.setAttribute('id', 'jump-icon');
  symbol.setAttribute('viewBox', '0 0 16 16');
  const path = DomAdapter.createSvgElement('path');
  path.setAttribute(
    'd',
    'M2 5a2 2 0 0 1 2-2h3v2H4v7h7V9h2v3a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5z' +
      'M9 2h5v5h-2V5.4l-5 5-1.4-1.4 5-5H9z',
  );
  symbol.appendChild(path);
  defs.appendChild(symbol);
  defsHost.appendChild(defs);
}

// The browser global exposing this module's API.
const JumpSymbol = { defineJumpSymbol };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { defineJumpSymbol, JumpSymbol };
}
