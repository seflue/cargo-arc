// @module HotspotLabels
// @deps HotspotTree
// @config
// hotspot_labels.js - Whether a circle's label is shown, its size and
// position: shown once the circle is large enough on screen, sized to fit
// its label band and dropped where it would overlap an already-placed
// label. Names and values kept from the prototype (`proto/hotspot-map` @
// `b4a3557d2f463a1f1c26da79fbb5d019ce55f7ea`, `proto/hotspots/page.html`,
// PURE-BEGIN..PURE-END) wherever a prototype constant applies directly.
//
// STATIC_DATA carries no label-band field; the band is already baked into
// each circle's `r`, `cx`, `cy` by the time it reaches JS. `labelBand`
// below recovers it geometrically: the gap between a container's top edge
// and its highest child's top edge is, by construction of the packer,
// exactly that band.

const LABEL_PX = 13; // font size on screen when the circle has room for it
const MIN_LABEL_PX = 7; // smaller text is dropped
const MIN_LABEL_DIAMETER_PX = 40; // a circle smaller than this on screen gets no label
const CHAR_WIDTH_EM = 0.6; // average glyph width of the UI font
const LINE_HEIGHT_EM = 1.2;
const FILE_LABEL_WIDTH_RATIO = 0.78; // a file label may span this share of its diameter
const FILE_LABEL_HEIGHT_RATIO = 0.4; // and its font size this share of the radius
const PAD_PX = 2; // screen-space clearance between a child and its container's edge, like LABEL_PX
// Bisection rounds in `solveContainerFontSize`: each halves the search
// interval, so 30 of them narrow it to about a billionth of its starting
// width - far finer than a font size can visibly differ by.
const FONT_SIZE_SOLVE_ROUNDS = 30;

const diameter = (node) => 2 * node.r;
const isFile = (node) => node.kind === 'file';
const textWidth = (name, fontSize) => CHAR_WIDTH_EM * fontSize * name.length;
const textHeight = (fontSize) => LINE_HEIGHT_EM * fontSize;

/** How far below a circle's top point its edge has dropped at a horizontal offset from the centre. */
function arcDepth(radius, offset) {
  return radius - Math.sqrt(Math.max(0, radius * radius - offset * offset));
}

/**
 * The vertical room a container keeps free above its children for its own
 * label: the gap between its top edge and its highest child's top edge.
 * @param {{ cy: number, r: number }} node
 * @param {{ cy: number, r: number }[]} children
 */
function labelBand(node, children) {
  const top = node.cy - node.r;
  const highestChildTop = Math.min(
    ...children.map((child) => child.cy - child.r),
  );
  return highestChildTop - top;
}

/** A file label sits at the centre, sized to its circle. */
function fileLabel(node, pxPerUnit) {
  const chars = Math.max(node.name.length, 3);
  const fitsWidth =
    (FILE_LABEL_WIDTH_RATIO * diameter(node)) / (CHAR_WIDTH_EM * chars);
  const fontSize = Math.min(
    LABEL_PX / pxPerUnit,
    fitsWidth,
    FILE_LABEL_HEIGHT_RATIO * node.r,
  );
  return { fontSize, y: node.cy, text: node.name };
}

/**
 * The font size at which "as large as the room below the container's top
 * edge allows" is self-consistent: growing the font only ever deepens the
 * arc it must clear (never shallows it), so the wanted size bisects to a
 * single fixed point instead of the width/depth pair oscillating between
 * two answers, as a fixed-point iteration on this same relation once did.
 */
function solveContainerFontSize(name, radius, band, wanted, padUnits) {
  const target = (fontSize) => {
    const depth = padUnits + arcDepth(radius, textWidth(name, fontSize) / 2);
    return Math.max(0, Math.min(wanted, (band - depth) / LINE_HEIGHT_EM));
  };
  let lo = 0;
  let hi = wanted;
  for (let round = 0; round < FONT_SIZE_SOLVE_ROUNDS; round++) {
    const mid = (lo + hi) / 2;
    if (target(mid) >= mid) lo = mid;
    else hi = mid;
  }
  return (lo + hi) / 2;
}

/**
 * A container label hangs from the top edge, as high as the arc lets its
 * width through, and shrinks until it fits between the arc and the band's
 * bottom. `PAD_PX` is a screen clearance like `LABEL_PX`, converted to
 * layout units by the same `/ pxPerUnit` division.
 */
function containerLabel(node, band, pxPerUnit) {
  const top = node.cy - node.r;
  const padUnits = PAD_PX / pxPerUnit;
  const wanted = Math.min(LABEL_PX / pxPerUnit, band / LINE_HEIGHT_EM);
  const fontSize = solveContainerFontSize(
    node.name,
    node.r,
    band,
    wanted,
    padUnits,
  );
  const halfWidth = textWidth(node.name, fontSize) / 2;
  const depth = padUnits + arcDepth(node.r, halfWidth);
  return {
    fontSize,
    y: top + depth + textHeight(fontSize) / 2,
    text: node.name,
  };
}

/** Font size and centre line for `node`; a file is centred, a container hangs from its band. */
function labelFor(node, children, pxPerUnit) {
  return isFile(node)
    ? fileLabel(node, pxPerUnit)
    : containerLabel(node, labelBand(node, children), pxPerUnit);
}

function labelReadable(node, fontSize, pxPerUnit) {
  return (
    diameter(node) * pxPerUnit >= MIN_LABEL_DIAMETER_PX &&
    fontSize * pxPerUnit >= MIN_LABEL_PX
  );
}

function labelBox(node, text, fontSize, y) {
  const width = textWidth(text, fontSize);
  const height = textHeight(fontSize);
  return { x: node.cx - width / 2, y: y - height / 2, width, height };
}

function boxesOverlap(a, b) {
  return (
    a.x < b.x + b.width &&
    b.x < a.x + a.width &&
    a.y < b.y + b.height &&
    b.y < a.y + a.height
  );
}

/**
 * Every node's label placement for one frame: `null` when the label is not
 * shown, `{ fontSize, y, text }` when it is. Labels are placed in preorder; a
 * label whose box intersects an already-placed one is dropped, except the
 * hovered node's, which always wins even over a collision and never blocks
 * a later label either, matching the prototype.
 * @param {Record<string, {kind: string, name: string, parent?: string, cx: number, cy: number, r: number, lines: number}>} nodes
 * @param {string} targetKey - the current zoom target; it and its ancestors carry no label
 * @param {string | null} hoveredKey
 * @param {number} pxPerUnit
 * @returns {Map<string, {fontSize: number, y: number, text: string} | null>}
 */
function placeLabels(nodes, targetKey, hoveredKey, pxPerUnit) {
  const onPath = new Set(HotspotTree.ancestorPath(nodes, targetKey));
  const children = HotspotTree.childrenOf(nodes);
  const placed = [];
  const result = new Map();

  for (const key of HotspotTree.preorderKeys(nodes)) {
    const node = nodes[key];
    const kids = (children.get(key) ?? []).map((childKey) => nodes[childKey]);
    const { fontSize, y, text } = labelFor(node, kids, pxPerUnit);
    const wanted = !onPath.has(key) && labelReadable(node, fontSize, pxPerUnit);
    const isHovered = key === hoveredKey;
    let show = isHovered || wanted;
    if (show && !isHovered) {
      const box = labelBox(node, text, fontSize, y);
      if (placed.some((other) => boxesOverlap(other, box))) show = false;
      else placed.push(box);
    }
    result.set(key, show ? { fontSize, y, text } : null);
  }
  return result;
}

// The browser global exposing this module's API.
const HotspotLabels = {
  LABEL_PX,
  MIN_LABEL_PX,
  MIN_LABEL_DIAMETER_PX,
  CHAR_WIDTH_EM,
  LINE_HEIGHT_EM,
  FILE_LABEL_WIDTH_RATIO,
  arcDepth,
  labelBand,
  fileLabel,
  containerLabel,
  labelFor,
  labelReadable,
  labelBox,
  boxesOverlap,
  placeLabels,
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = {
    LABEL_PX,
    MIN_LABEL_PX,
    MIN_LABEL_DIAMETER_PX,
    CHAR_WIDTH_EM,
    LINE_HEIGHT_EM,
    FILE_LABEL_WIDTH_RATIO,
    arcDepth,
    labelBand,
    fileLabel,
    containerLabel,
    labelFor,
    labelReadable,
    labelBox,
    boxesOverlap,
    placeLabels,
    HotspotLabels,
  };
}
