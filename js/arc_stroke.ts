import { ArcLogic } from './arc_logic.js';
import { DomAdapter } from './dom_adapter.js';
import { Selectors } from './selectors.js';

// arc_stroke.ts - Draws a visible arc inside its group as hairlines made of
// pieces, or as one path where the browser antialiases a whole curve itself.
// The group keeps the curve in `data-path` and its width in
// `style.strokeWidth`; the paths inherit color and pointer-events from it.

const ArcStroke = {
  /**
   * Replace the children of an arc's group with the paths that draw its curve.
   * @param path - Arc path as `ArcLogic.calculateArcPath` writes it
   * @param width - Arc width in CSS px
   * @param dpr - Device pixels per CSS px
   */
  draw(group: SVGElement, path: string, width: number, dpr: number) {
    group.setAttribute('data-path', path);
    group.style.strokeWidth = `${width}px`;
    if (DomAdapter.antialiasesWholeCurves()) {
      const curve = DomAdapter.createSvgElement('path');
      curve.setAttribute('d', path);
      group.replaceChildren(curve);
      return;
    }
    // Chromium leaves gaps along a long subpath, not along a long element:
    // the pieces go into one element as subpaths, which keeps the number of
    // elements Chromium restyles on every highlight low.
    const pieces = ArcLogic.hairlinePieces(path, width, dpr);
    const middle = DomAdapter.createSvgElement('path');
    middle.setAttribute('d', pieces.map((piece) => piece.middle).join(' '));
    const outers = pieces.flatMap((piece) => piece.outer ?? []);
    if (outers.length === 0) {
      group.replaceChildren(middle);
      return;
    }
    const hairline = `${1 / dpr}px`;
    middle.style.strokeWidth = hairline;
    const outer = DomAdapter.createSvgElement('path');
    outer.classList.add(STATIC_DATA.classes.direction.hairOuter);
    outer.setAttribute('d', outers.join(' '));
    outer.style.strokeWidth = hairline;
    group.replaceChildren(middle, outer);
  },

  /** Draw the curve the group holds at a new width. */
  redraw(group: SVGElement, width: number, dpr: number) {
    this.draw(group, group.getAttribute('data-path') ?? '', width, dpr);
  },

  /** Draw every arc and virtual arc at its current width for a new density. */
  redrawAll(dpr: number) {
    for (const group of DomAdapter.querySelectorAll(Selectors.allArcPaths())) {
      // A shadow carries the classes of its arc but stays a single stroke.
      if (group.classList.contains(STATIC_DATA.classes.relation.shadowPath))
        continue;
      this.redraw(group, parseFloat(group.style.strokeWidth), dpr);
    }
  },
};

export { ArcStroke };
