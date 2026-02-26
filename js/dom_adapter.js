// @module DomAdapter
// @deps Selectors
// @config
// dom_adapter.js - DOM abstraction layer for SVG manipulation
// Selectors is loaded before this file (see render.rs load order)

function createFakeElement(tagName) {
  const attrs = new Map();
  const classes = new Set();
  const styleData = {};
  const children = [];
  return {
    tagName,
    children,
    get firstChild() {
      return children[0] ?? null;
    },
    getAttribute(name) {
      return attrs.get(name) ?? null;
    },
    setAttribute(name, value) {
      attrs.set(name, value);
    },
    removeAttribute(name) {
      attrs.delete(name);
    },
    classList: {
      add(c) {
        classes.add(c);
      },
      remove(c) {
        classes.delete(c);
      },
      contains(c) {
        return classes.has(c);
      },
    },
    style: new Proxy(styleData, {
      get(target, prop) {
        return target[prop];
      },
      set(target, prop, value) {
        target[prop] = value;
        return true;
      },
    }),
    cloneNode(_deep) {
      const clone = createFakeElement(tagName);
      for (const [k, v] of attrs) clone.setAttribute(k, v);
      for (const c of classes) clone.classList.add(c);
      Object.assign(clone.style, { ...styleData });
      return clone;
    },
    appendChild(child) {
      // Mimic real DOM: remove from previous parent before appending
      if (child._parentRef) {
        child._parentRef.removeChild(child);
      }
      children.push(child);
      child._parentRef = this;
    },
    removeChild(child) {
      const idx = children.indexOf(child);
      if (idx !== -1) {
        children.splice(idx, 1);
        child._parentRef = null;
      }
    },
  };
}

// Convenience methods shared between DomAdapter and mock.
// These only call this.getElementById/querySelector/querySelectorAll,
// so they work on any object that provides those four base methods.
// Factory function: each caller gets its own arc element cache (closure).
function createConvenienceMethods() {
  const _arcCache = new Map();

  return {
    cacheArcElements(arcId, arc, arrows, labelGroup) {
      _arcCache.set(arcId, { arc, arrows, labelGroup });
    },
    clearArcCache() {
      _arcCache.clear();
    },
    evictArcCache(arcId) {
      _arcCache.delete(arcId);
    },
    getNode(nodeId) {
      return this.getElementById(Selectors.nodeId(nodeId));
    },
    // Raw access - returns arc regardless of visibility (for reset operations)
    getArc(arcId) {
      if (_arcCache.has(arcId)) return _arcCache.get(arcId).arc;
      return this.querySelector(Selectors.baseArc(arcId));
    },
    // Filtered access - returns only VISIBLE arc (for apply/highlight operations)
    getVisibleArc(arcId) {
      const arc = this.getArc(arcId);
      if (!arc || arc.style.display === 'none') return null;
      return arc;
    },
    getHitarea(arcId) {
      return this.querySelector(Selectors.hitarea(arcId));
    },
    // Raw access - returns ALL arrows (including hidden ones, for reset operations)
    getArrows(arcId) {
      if (_arcCache.has(arcId)) return _arcCache.get(arcId).arrows;
      return Array.from(this.querySelectorAll(Selectors.arrows(arcId)));
    },
    // Filtered access - returns only VISIBLE arrows (for apply/highlight operations)
    getVisibleArrows(arcId) {
      const arrows = this.getArrows(arcId);
      return Array.from(arrows).filter((arr) => arr.style.display !== 'none');
    },
    getVirtualArrows(arcId) {
      return this.querySelectorAll(Selectors.virtualArrows(arcId));
    },
    getLabelGroup(arcId) {
      if (_arcCache.has(arcId)) return _arcCache.get(arcId).labelGroup;
      return this.querySelector(Selectors.labelGroup(arcId));
    },
    getCollapseToggle(nodeId) {
      return this.querySelector(Selectors.collapseToggle(nodeId));
    },
    getCountLabel(nodeId) {
      return this.getElementById(Selectors.countId(nodeId));
    },
    getTreeLines(nodeId, role) {
      const sel =
        role === 'child'
          ? Selectors.treeLineChild(nodeId)
          : Selectors.treeLineParent(nodeId);
      return this.querySelectorAll(sel);
    },
    getSvgRoot() {
      return this.querySelector('svg');
    },
    getAllHitareas() {
      return this.querySelectorAll(Selectors.allHitareas());
    },
  };
}

function createMockDomAdapter() {
  const calls = new Map();
  const elements = new Map();
  const selectorResults = new Map();
  function track(method, args) {
    if (!calls.has(method)) calls.set(method, []);
    calls.get(method).push([...args]);
  }
  return {
    getElementById(id) {
      track('getElementById', [id]);
      return elements.get(id) ?? null;
    },
    querySelector(sel) {
      track('querySelector', [sel]);
      return selectorResults.get(sel) ?? null;
    },
    querySelectorAll(sel) {
      track('querySelectorAll', [sel]);
      return selectorResults.get(sel) ?? [];
    },
    createSvgElement(tag) {
      track('createSvgElement', [tag]);
      return createFakeElement(tag);
    },
    ...createConvenienceMethods(),
    _getCalls(method) {
      return calls.get(method) ?? [];
    },
    _registerElement(id, el) {
      elements.set(id, el);
    },
    _registerSelector(sel, result) {
      selectorResults.set(sel, result);
    },
  };
}

const SVG_NS = 'http://www.w3.org/2000/svg';

const DomAdapter = {
  getElementById(id) {
    return document.getElementById(id);
  },
  querySelector(sel) {
    return document.querySelector(sel);
  },
  querySelectorAll(sel) {
    return document.querySelectorAll(sel);
  },
  createSvgElement(tag) {
    return document.createElementNS(SVG_NS, tag);
  },
  ...createConvenienceMethods(),
};

// Export for Bun/Node
if (typeof module !== 'undefined' && module.exports) {
  module.exports = { DomAdapter, createMockDomAdapter, createFakeElement };
}
