// @module PathFit
// @deps
// @config
// path_fit.js - Shortens a path in a span until it fits: the full path
// stays in the span's `data-full`, the visible text loses segments before
// the last part, replaced by `…`. Shared by the arc sidebar and the map's.

/**
 * Shortens a path by replacing `dropCount` segments with `…`, taken from
 * the end of the prefix backwards. The part after the last separator is
 * never dropped: for a module path `a::b::` that is the empty tail, so the
 * result still ends in `::`; for a file path it is the file name.
 * @param {string} full
 * @param {string} separator
 * @param {number} dropCount
 * @returns {string}
 */
function elidePath(full, separator, dropCount) {
  if (dropCount <= 0) return full;
  const parts = full.split(separator);
  const tail = parts.pop();
  const kept = parts.slice(0, Math.max(0, parts.length - dropCount));
  return [...kept, '…', tail].join(separator);
}

/**
 * Puts the full text back into `span`. Must run before a sidebar measures
 * its natural width, or the measurement sees the cut text, the sidebar
 * narrows, and the next pass cuts more.
 * @param {HTMLElement} span
 */
function resetPath(span) {
  span.textContent = span.dataset.full ?? '';
  span.style.flexShrink = '';
}

/**
 * Rewrites `span` so it fits its box: the text is reset to `data-full`,
 * then segments are dropped one at a time while `overflows(span)` holds.
 * A span that overflows even at its shortest form sits in a row whose other
 * children are wider than the sidebar; it stops shrinking there so the `…`
 * stays visible and the row scrolls instead.
 * @param {HTMLElement} span
 * @param {string} separator
 * @param {(span: HTMLElement) => boolean} overflows
 */
function fitPath(span, separator, overflows) {
  const full = span.dataset.full ?? '';
  resetPath(span);
  for (let drop = 1; overflows(span); drop++) {
    const shorter = elidePath(full, separator, drop);
    if (shorter === span.textContent) {
      span.style.flexShrink = '0';
      break;
    }
    span.textContent = shorter;
  }
}

// The browser global exposing this module's API.
const PathFit = { elidePath, resetPath, fitPath };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { elidePath, resetPath, fitPath, PathFit };
}
