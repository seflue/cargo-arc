import { describe, expect, test } from 'bun:test';
import { HotspotTree } from './hotspot_tree.js';

global.HotspotTree = HotspotTree;

import {
  arcDepth,
  boxesOverlap,
  CHAR_WIDTH_EM,
  containerLabel,
  FILE_LABEL_WIDTH_RATIO,
  fileLabel,
  LINE_HEIGHT_EM,
  labelBand,
  labelBox,
  labelReadable,
  MIN_LABEL_DIAMETER_PX,
  MIN_LABEL_PX,
  placeLabels,
} from './hotspot_labels.js';

/** `containerLabel`'s own vertical placement decomposed back into the depth
 * below the circle's top point its returned `y` sits at. */
function depthOf(node, fontSize, y) {
  const top = node.cy - node.r;
  return y - top - (LINE_HEIGHT_EM * fontSize) / 2;
}

describe('arcDepth', () => {
  test('is 0 at the centre offset', () => {
    expect(arcDepth(10, 0)).toBe(0);
  });

  test('is the full radius at the edge offset', () => {
    expect(arcDepth(10, 10)).toBe(10);
  });

  test('is between the two at a partial offset', () => {
    // radius 10, offset 6 -> 10 - sqrt(100 - 36) = 10 - 8 = 2
    expect(arcDepth(10, 6)).toBeCloseTo(2, 10);
  });
});

describe('fileLabel', () => {
  test('centres the label on the circle', () => {
    const node = { cx: 5, cy: -3, r: 30, name: 'lib.rs' };
    const { y } = fileLabel(node, 1);
    expect(y).toBe(-3);
  });

  test('shrinks the font for a small circle', () => {
    const small = fileLabel({ cx: 0, cy: 0, r: 4, name: 'lib.rs' }, 1);
    const big = fileLabel({ cx: 0, cy: 0, r: 400, name: 'lib.rs' }, 1);
    expect(small.fontSize).toBeLessThan(big.fontSize);
  });

  // A container's own declaring file (e.g. "mod.rs") gets this same
  // treatment, at the smallest radius the map still labels at all - the
  // prototype's own budget for a file label (`FILE_LABEL_WIDTH_RATIO` of its
  // diameter, same value as the prototype's).
  test('never renders wider than FILE_LABEL_WIDTH_RATIO of the diameter, even at the smallest labelled size', () => {
    const node = { cx: 0, cy: 0, r: MIN_LABEL_DIAMETER_PX / 2, name: 'mod.rs' };
    const { fontSize } = fileLabel(node, 1);
    const chars = Math.max(node.name.length, 3);
    const width = CHAR_WIDTH_EM * fontSize * chars;
    expect(width).toBeLessThanOrEqual(FILE_LABEL_WIDTH_RATIO * 2 * node.r);
  });
});

describe('labelBand', () => {
  test('is the gap between the container’s top edge and its topmost child', () => {
    const container = { cx: 0, cy: 0, r: 100 };
    const children = [
      { cx: -10, cy: 20, r: 30 }, // top at -10
      { cx: 20, cy: -40, r: 20 }, // top at -60
    ];
    // container top: -100; highest child top: -60 -> band = -60 - (-100) = 40
    expect(labelBand(container, children)).toBeCloseTo(40, 10);
  });
});

describe('containerLabel', () => {
  test('a short name in a roomy zone gets the full wanted size', () => {
    const node = { cx: 0, cy: 0, r: 100, name: 'x' };
    const { fontSize } = containerLabel(node, 50, 1);
    expect(fontSize).toBeCloseTo(13, 1); // LABEL_PX / pxPerUnit
  });

  test('a long name in a tight zone shrinks below the wanted size', () => {
    const node = { cx: 0, cy: 0, r: 20, name: 'a-rather-long-module-name' };
    const roomy = containerLabel({ ...node, r: 200 }, 50, 1);
    const tight = containerLabel(node, 6, 1);
    expect(tight.fontSize).toBeLessThan(roomy.fontSize);
  });

  test('the label sits below the top edge by more than the plain arc depth', () => {
    const node = { cx: 0, cy: 0, r: 50, name: 'mod' };
    const { fontSize, y } = containerLabel(node, 15, 1);
    const top = node.cy - node.r;
    expect(y).toBeGreaterThan(top);
    expect(fontSize).toBeGreaterThan(0);
  });
});

describe('containerLabel - deterministic, band-bounded placement (HARD 1)', () => {
  test('a real container label stays inside its band, unlike the old iteration', () => {
    // This repo's own STATIC_DATA for src/js_registry/mod.rs: the container
    // and its actual children (bundle.rs, mod.rs#leaf, table.rs), at the
    // pxPerUnit the default, unzoomed view renders it at
    // (mapAreaSize 800 / (2 * root radius 380)).
    const node = {
      cx: 368.67217346421074,
      cy: 156.45631880697027,
      r: 22.354542024137164,
      name: 'js_registry',
    };
    const children = [
      { cx: 363.4075344077552, cy: 157.17005012082507, r: 10.577976750936765 }, // bundle.rs
      { cx: 381.7595588506783, cy: 157.17005012082507, r: 2.7875416307723193 }, // mod.rs#leaf
      { cx: 378.59553621447867, cy: 166.93767137094437, r: 2.49325303060701 }, // table.rs
    ];
    const band = labelBand(node, children);
    const pxPerUnit = 1.0526315789473684; // 800 / (2 * 380)

    const { fontSize, y } = containerLabel(node, band, pxPerUnit);

    // The old fixed-point loop let this label's depth overshoot its band by
    // nearly 3x, sitting on top of its own children; the fix keeps the
    // label's bottom edge at or above the band's bottom instead.
    const depth = depthOf(node, fontSize, y);
    expect(depth + LINE_HEIGHT_EM * fontSize).toBeLessThanOrEqual(band + 1e-9);
  });

  test('a name that fits comfortably is left untruncated, unaffected by the fix', () => {
    const node = { cx: 0, cy: 0, r: 100, name: 'x' };
    const { fontSize, text } = containerLabel(node, 50, 1);
    expect(text).toBe('x');
    expect(fontSize).toBeCloseTo(13, 1); // LABEL_PX / pxPerUnit
  });
});

describe('containerLabel - PAD is screen-space, like LABEL_PX (HARD 2)', () => {
  /** The clearance, in screen pixels, between a label's top corner and the
   * circle's edge along the radius - PAD_PX if it converts to layout units
   * the same way LABEL_PX does. */
  function clearancePxAt(pxPerUnit) {
    const node = { cx: 0, cy: 0, r: 100, name: 'core' };
    const { fontSize, y, text } = containerLabel(node, 30, pxPerUnit);

    const halfWidth = (CHAR_WIDTH_EM * fontSize * text.length) / 2;
    const boxTopDepth = depthOf(node, fontSize, y);
    // Euclidean distance from the box's top corner to the circle's centre,
    // along the radius: how far inside the arc that corner actually sits
    // (less than the vertical PAD itself, since the arc slopes away there).
    const verticalToCentre = node.r - boxTopDepth;
    const distanceToCentre = Math.sqrt(halfWidth ** 2 + verticalToCentre ** 2);
    return (node.r - distanceToCentre) * pxPerUnit;
  }

  test('the clearance in screen pixels is the same at default zoom and zoomed in', () => {
    // A PAD_PX that is not divided by pxPerUnit (the reverted bug) stays a
    // fixed number of layout units, so the screen clearance it produces
    // grows with the zoom instead of staying put.
    const atDefaultZoom = clearancePxAt(1);
    const zoomedIn = clearancePxAt(10);
    expect(zoomedIn).toBeCloseTo(atDefaultZoom, 0);
  });
});

describe('labelReadable', () => {
  test('a circle right at the diameter threshold is readable', () => {
    const node = { r: MIN_LABEL_DIAMETER_PX / 2 };
    expect(labelReadable(node, MIN_LABEL_PX, 1)).toBe(true);
  });

  test('a circle just under the diameter threshold is not', () => {
    const node = { r: MIN_LABEL_DIAMETER_PX / 2 - 1 };
    expect(labelReadable(node, MIN_LABEL_PX, 1)).toBe(false);
  });

  test('a font just under the pixel threshold is not readable either', () => {
    const node = { r: MIN_LABEL_DIAMETER_PX };
    expect(labelReadable(node, MIN_LABEL_PX - 0.1, 1)).toBe(false);
  });
});

describe('labelBox and boxesOverlap', () => {
  test('two boxes centred on the same point overlap', () => {
    const a = labelBox({ cx: 0 }, 'abc', 10, 0);
    const b = labelBox({ cx: 1 }, 'xy', 10, 1);
    expect(boxesOverlap(a, b)).toBe(true);
  });

  test('two boxes far apart do not overlap', () => {
    const a = labelBox({ cx: 0 }, 'abc', 10, 0);
    const b = labelBox({ cx: 1000 }, 'xy', 10, 0);
    expect(boxesOverlap(a, b)).toBe(false);
  });
});

describe('placeLabels', () => {
  // root (container) -> near, far (two files close enough their labels
  // collide at this pxPerUnit; far is a bit further and larger).
  function nodes() {
    return {
      root: { name: 'root', kind: 'crate', cx: 0, cy: 0, r: 200, lines: 20 },
      near: {
        name: 'near',
        kind: 'file',
        parent: 'root',
        cx: -60,
        cy: 0,
        r: 50,
        lines: 10,
      },
      far: {
        name: 'far',
        kind: 'file',
        parent: 'root',
        cx: 60,
        cy: 0,
        r: 50,
        lines: 5,
      },
    };
  }

  test('the zoom target and its ancestors carry no label', () => {
    const placed = placeLabels(nodes(), 'root', null, 1);
    expect(placed.get('root')).toBeNull();
  });

  test('a label whose box collides with an already-placed one is dropped', () => {
    const placed = placeLabels(nodes(), 'far', null, 1);
    // preorder is root, near, far (both lines-sorted the same way
    // childrenOf sorts them: near has more lines, so it is placed first).
    expect(placed.get('near')).not.toBeNull();
    expect(placed.get('far')).toBeNull();
  });

  test('the hovered node always wins, even over a collision', () => {
    const placed = placeLabels(nodes(), 'far', 'far', 1);
    expect(placed.get('far')).not.toBeNull();
  });

  test('a circle too small on screen gets no label', () => {
    const placed = placeLabels(nodes(), 'far', null, 0.01);
    expect(placed.get('near')).toBeNull();
  });
});
