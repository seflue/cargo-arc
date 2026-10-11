// page_link.ts - The `?select=<file>` protocol carrying a selection across
// the toolbar link between the arc page and the hotspot map.

/**
 * The link to `basePath`, carrying `file` as `?select=<file>` when given, or
 * bare when there is no selection.
 */
function buildLink(basePath: string, file: string | null | undefined): string {
  return file ? `${basePath}?select=${encodeURIComponent(file)}` : basePath;
}

/**
 * The `select` query parameter from a location search string (e.g.
 * `location.search`), decoded, or `null` when absent.
 */
function parseSelect(search: string): string | null {
  return new URLSearchParams(search).get('select');
}

const PageLink = { buildLink, parseSelect };

export { buildLink, parseSelect, PageLink };
