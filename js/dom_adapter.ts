import { Selectors } from './selectors.js';

// dom_adapter.ts - DOM abstraction layer for SVG manipulation

type DomQueries = {
  getElementById(id: string): Element | null;
  querySelector<E extends Element = SVGElement>(sel: string): E | null;
  querySelectorAll<E extends Element = SVGElement>(sel: string): NodeListOf<E>;
  createSvgElement(tag: string): SVGElement;
};

type ArcLookup = DomQueries & {
  getArc(arcId: string): SVGElement | null;
  getArrows(arcId: string): SVGElement[];
};

type CachedArc = {
  arc: SVGElement;
  arrows: SVGElement[];
  labelGroup: SVGElement | null;
};

// Convenience methods shared between DomAdapter and mock.
// These only call this.getElementById/querySelector/querySelectorAll,
// so they work on any object that provides those four base methods.
// Factory function: each caller gets its own arc element cache (closure).
function createConvenienceMethods() {
  const _arcCache = new Map<string, CachedArc>();

  return {
    cacheArcElements(
      arcId: string,
      arc: SVGElement,
      arrows: SVGElement[],
      labelGroup: SVGElement | null,
    ) {
      _arcCache.set(arcId, { arc, arrows, labelGroup });
    },
    clearArcCache() {
      _arcCache.clear();
    },
    evictArcCache(arcId: string) {
      _arcCache.delete(arcId);
    },
    getNode(this: DomQueries, nodeId: string) {
      return this.getElementById(Selectors.nodeId(nodeId));
    },
    // Raw access - returns arc regardless of visibility (for reset operations)
    getArc(this: DomQueries, arcId: string): SVGElement | null {
      const cached = _arcCache.get(arcId);
      if (cached) return cached.arc;
      return this.querySelector(Selectors.baseArc(arcId));
    },
    // Filtered access - returns only VISIBLE arc (for apply/highlight operations)
    getVisibleArc(this: ArcLookup, arcId: string) {
      const arc = this.getArc(arcId);
      if (!arc || arc.style.display === 'none') return null;
      return arc;
    },
    getHitarea(this: DomQueries, arcId: string) {
      return this.querySelector(Selectors.hitarea(arcId));
    },
    // Raw access - returns ALL arrows (including hidden ones, for reset operations)
    getArrows(this: DomQueries, arcId: string): SVGElement[] {
      const cached = _arcCache.get(arcId);
      if (cached) return cached.arrows;
      return Array.from(this.querySelectorAll(Selectors.arrows(arcId)));
    },
    // Filtered access - returns only VISIBLE arrows (for apply/highlight operations)
    getVisibleArrows(this: ArcLookup, arcId: string) {
      const arrows = this.getArrows(arcId);
      return arrows.filter((arr) => arr.style.display !== 'none');
    },
    getVirtualArrows(this: DomQueries, arcId: string) {
      return Array.from(this.querySelectorAll(Selectors.virtualArrows(arcId)));
    },
    getLabelGroup(this: DomQueries, arcId: string) {
      const cached = _arcCache.get(arcId);
      if (cached) return cached.labelGroup;
      return this.querySelector(Selectors.labelGroup(arcId));
    },
    getCollapseToggle(this: DomQueries, nodeId: string) {
      return this.querySelector(Selectors.collapseToggle(nodeId));
    },
    getCountLabel(this: DomQueries, nodeId: string) {
      return this.getElementById(Selectors.countId(nodeId));
    },
    getCycleMarker(this: DomQueries, nodeId: string) {
      return this.getElementById(Selectors.cycleMarkerId(nodeId));
    },
    getTreeLines(this: DomQueries, nodeId: string, role: 'child' | 'parent') {
      const sel =
        role === 'child'
          ? Selectors.treeLineChild(nodeId)
          : Selectors.treeLineParent(nodeId);
      return this.querySelectorAll(sel);
    },
    getSvgRoot(this: DomQueries) {
      return this.querySelector<SVGSVGElement>('svg');
    },
    getAllHitareas(this: DomQueries) {
      return this.querySelectorAll(Selectors.allHitareas());
    },
  };
}

// NaN when the attribute is missing.
function numberAttribute(el: Element, name: string) {
  return parseFloat(el.getAttribute(name) ?? '');
}

const SVG_NS = 'http://www.w3.org/2000/svg';

const DomAdapter = {
  getElementById(id: string) {
    return document.getElementById(id);
  },
  querySelector<E extends Element = SVGElement>(sel: string) {
    return document.querySelector<E>(sel);
  },
  querySelectorAll<E extends Element = SVGElement>(sel: string) {
    return document.querySelectorAll<E>(sel);
  },
  createSvgElement(tag: string) {
    return document.createElementNS(SVG_NS, tag);
  },
  ...createConvenienceMethods(),
};

export { DomAdapter, createConvenienceMethods, numberAttribute };
