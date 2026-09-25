// @module HotspotBars
// @deps
// @config
// hotspot_bars.js - The sidebar's ranked-bars view of the top-N hotspots:
// the same list `render::hotspots` already drew as `<li>` rows, reshaped
// into bars. Bar length is the hotspot's own share of the highest score in
// the list; a bar's fill reads `fillPercent` off the node itself, the same
// value `render::hotspots::circle_style` computed for that node's circle,
// so the two stay on one scale without JS recomputing it.

/**
 * The top-N hotspots as bar rows, in the given order.
 * @param {Record<string, {file: string, lines: number, commits: number, rank?: number, fillPercent: number}>} nodes
 * @param {string[]} hotspots - keys into `nodes`, rank order
 * @returns {{ key: string, file: string, rank: number | undefined, lines: number, commits: number, score: number, widthPercent: number, fillPercent: number }[]}
 */
function hotspotBars(nodes, hotspots) {
  const scored = hotspots.map((key) => {
    const node = nodes[key];
    return { key, node, score: node.lines * node.commits };
  });
  const maxScore = Math.max(1, ...scored.map((s) => s.score));
  return scored.map(({ key, node, score }) => ({
    key,
    file: node.file,
    rank: node.rank,
    lines: node.lines,
    commits: node.commits,
    score,
    widthPercent: (100 * score) / maxScore,
    fillPercent: node.fillPercent,
  }));
}

// The browser global exposing this module's API.
const HotspotBars = { hotspotBars };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { hotspotBars, HotspotBars };
}
