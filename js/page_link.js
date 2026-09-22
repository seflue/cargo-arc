// @module PageLink
// @deps
// @config
// page_link.js - The `?select=<file>` protocol carrying a selection across
// the toolbar link between the arc page and the hotspot map.

/**
 * The link to `basePath`, carrying `file` as `?select=<file>` when given, or
 * bare when there is no selection.
 * @param {string} basePath
 * @param {string | null | undefined} file
 * @returns {string}
 */
function buildLink(basePath, file) {
  return file ? `${basePath}?select=${encodeURIComponent(file)}` : basePath;
}

/**
 * The `select` query parameter from a location search string (e.g.
 * `location.search`), decoded, or `null` when absent.
 * @param {string} search
 * @returns {string | null}
 */
function parseSelect(search) {
  return new URLSearchParams(search).get('select');
}

// The browser global exposing this module's API.
const PageLink = { buildLink, parseSelect };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { buildLink, parseSelect, PageLink };
}
