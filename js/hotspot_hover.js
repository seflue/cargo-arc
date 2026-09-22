// @module HotspotHover
// @deps HotspotTree, DomAdapter, TextMeasure
// @config
// hotspot_hover.js - The hover tooltip's content (title, kind, lines,
// commits, rank or child count) and a small SVG popover that shows it
// beside the pointer, built the way jump_icons.js builds its popover
// (self-contained SVG, ADR-006 - no separate stylesheet, no foreignObject).

/** The number of direct children `key` has, for a container's tooltip row. */
function childCount(nodes, key) {
  return HotspotTree.childrenOf(nodes).get(key)?.length ?? 0;
}

/** The tooltip's title: the node's path, or its name when it has none (the workspace root). */
function tooltipTitle(node) {
  return node.file || node.name;
}

/**
 * The tooltip's content for one node: a title (path or name) and the detail
 * rows (kind, lines, commits, and rank or child count).
 * @param {Record<string, {kind: string, name: string, file: string, lines: number, commits: number, rank?: number}>} nodes
 * @param {string} key
 * @returns {{ title: string, rows: [string, string | number][] }}
 */
function tooltipRows(nodes, key) {
  const node = nodes[key];
  /** @type {[string, string | number][]} */
  const rows = [
    ['kind', node.kind],
    ['lines', node.lines],
    ['commits', node.commits],
  ];
  if (node.rank) rows.push(['rank', `#${node.rank}`]);
  if (node.kind !== 'file') rows.push(['children', childCount(nodes, key)]);
  return { title: tooltipTitle(node), rows };
}

/** Padding inside the tooltip background, in SVG units. */
const TOOLTIP_PAD = 6;
/** Line height for a tooltip row, in SVG units. */
const TOOLTIP_LINE_HEIGHT = 14;
/** The tooltip's own font size, matching `.hotspot-tooltip text`'s CSS rule. */
const TOOLTIP_FONT_PX = 11;
/** Gap between the pointer and the tooltip. */
const TOOLTIP_GAP = 8;

/**
 * Builds and positions the hover tooltip beside the pointer: a background
 * sized to its longest line, a title row and one row per `[label, value]`
 * pair. Kept clear of the canvas's right edge, where the always-visible
 * hotspot sidebar sits, by flipping to the pointer's left when it would
 * cross it.
 * @param {{ layer: Element, canvasWidth: number, tooltipClass?: string }} deps -
 *   the SVG layer the tooltip is appended to, the initial canvas width to
 *   clamp within (`setCanvasWidth` on the returned object updates it after
 *   a resize), and the group's own class (`STATIC_DATA.classes.tooltip` on
 *   a real page; a literal default keeps this module usable stand-alone).
 */
function createHoverTooltip({
  layer,
  canvasWidth,
  tooltipClass = 'hotspot-tooltip',
}) {
  let group = null;
  // Live, not the value captured at creation: a resize calls `setCanvasWidth`
  // so a tooltip already open (or shown next) clamps against the current
  // canvas, not the one measured when the page first loaded.
  let width = canvasWidth;

  function removeGroup() {
    if (group) {
      layer.removeChild(group);
      group = null;
    }
  }

  /**
   * @param {{ x: number, y: number }} point - the pointer position, in the
   *   outer `<svg>`'s own coordinate space.
   * @param {{ title: string, rows: [string, string | number][] }} content
   */
  function show(point, content) {
    removeGroup();
    const lines = [
      content.title,
      ...content.rows.map(([label, value]) => `${label}: ${value}`),
    ];
    const boxWidth =
      TOOLTIP_PAD * 2 +
      Math.max(
        ...lines.map((line) =>
          TextMeasure.estimateWidth(line, TOOLTIP_FONT_PX),
        ),
      );
    const height = TOOLTIP_PAD * 2 + TOOLTIP_LINE_HEIGHT * lines.length;
    let x = point.x + TOOLTIP_GAP;
    if (x + boxWidth > width) x = point.x - TOOLTIP_GAP - boxWidth;
    x = Math.max(0, Math.min(x, width - boxWidth));
    const y = point.y - height / 2;

    const g = DomAdapter.createSvgElement('g');
    g.setAttribute('class', tooltipClass);

    const bg = DomAdapter.createSvgElement('rect');
    bg.setAttribute('x', x);
    bg.setAttribute('y', y);
    bg.setAttribute('width', boxWidth);
    bg.setAttribute('height', height);
    bg.setAttribute('rx', 3);
    g.appendChild(bg);

    lines.forEach((line, index) => {
      const text = DomAdapter.createSvgElement('text');
      text.setAttribute('x', x + TOOLTIP_PAD);
      text.setAttribute(
        'y',
        y + TOOLTIP_PAD + TOOLTIP_LINE_HEIGHT * index + TOOLTIP_LINE_HEIGHT / 2,
      );
      text.textContent = line;
      g.appendChild(text);
    });

    layer.appendChild(g);
    group = g;
  }

  /** Called on resize, so a tooltip clamps against the current canvas width. */
  function setCanvasWidth(newWidth) {
    width = newWidth;
  }

  return { show, hide: removeGroup, setCanvasWidth };
}

// The browser global exposing this module's API.
const HotspotHover = { tooltipTitle, tooltipRows, createHoverTooltip };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = {
    tooltipTitle,
    tooltipRows,
    createHoverTooltip,
    HotspotHover,
  };
}
