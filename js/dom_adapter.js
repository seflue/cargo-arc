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
    getAttribute(name) { return attrs.get(name) ?? null; },
    setAttribute(name, value) { attrs.set(name, value); },
    classList: {
      add(c) { classes.add(c); },
      remove(c) { classes.delete(c); },
      contains(c) { return classes.has(c); },
    },
    style: new Proxy(styleData, {
      get(target, prop) { return target[prop]; },
      set(target, prop, value) { target[prop] = value; return true; },
    }),
    appendChild(child) { children.push(child); },
    removeChild(child) {
      const idx = children.indexOf(child);
      if (idx !== -1) children.splice(idx, 1);
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
    getElementById(id) { track("getElementById", [id]); return elements.get(id) ?? null; },
    querySelector(sel) { track("querySelector", [sel]); return selectorResults.get(sel) ?? null; },
    querySelectorAll(sel) { track("querySelectorAll", [sel]); return selectorResults.get(sel) ?? []; },
    createSvgElement(tag) { track("createSvgElement", [tag]); return createFakeElement(tag); },
    // Convenience methods (use Selectors)
    getNode(nodeId) { return this.getElementById(Selectors.nodeId(nodeId)); },
    getVisibleArc(arcId) {
      const arc = this.querySelector(Selectors.visibleArc(arcId));
      if (!arc || arc.style.display === 'none') return null;
      return arc;
    },
    getHitarea(arcId) { return this.querySelector(Selectors.hitarea(arcId)); },
    getArrows(arcId) { return this.querySelectorAll(Selectors.arrows(arcId)); },
    getVisibleArrows(arcId) {
      const arrows = this.querySelectorAll(Selectors.arrows(arcId));
      return Array.from(arrows).filter(arr => arr.style.display !== 'none');
    },
    getVirtualArrows(arcId) { return this.querySelectorAll(Selectors.virtualArrows(arcId)); },
    getConnectedHitareas(nodeId) { return this.querySelectorAll(Selectors.connectedHitareas(nodeId)); },
    getLabelGroup(arcId) { return this.querySelector(Selectors.labelGroup(arcId)); },
    getCollapseToggle(nodeId) { return this.querySelector(Selectors.collapseToggle(nodeId)); },
    getCountLabel(nodeId) { return this.getElementById(Selectors.countId(nodeId)); },
    getTreeLines(nodeId, role) {
      const sel = role === 'child' ? Selectors.treeLineChild(nodeId) : Selectors.treeLineParent(nodeId);
      return this.querySelectorAll(sel);
    },
    getSvgRoot() { return this.querySelector('svg'); },
    getAllHitareas() { return this.querySelectorAll(Selectors.allHitareas()); },
    _getCalls(method) { return calls.get(method) ?? []; },
    _registerElement(id, el) { elements.set(id, el); },
    _registerSelector(sel, result) { selectorResults.set(sel, result); },
  };
}

const SVG_NS = "http://www.w3.org/2000/svg";

const DomAdapter = {
  getElementById(id) { return document.getElementById(id); },
  querySelector(sel) { return document.querySelector(sel); },
  querySelectorAll(sel) { return document.querySelectorAll(sel); },
  createSvgElement(tag) { return document.createElementNS(SVG_NS, tag); },
  // Convenience methods (use Selectors)
  getNode(nodeId) { return this.getElementById(Selectors.nodeId(nodeId)); },
  getVisibleArc(arcId) {
    const arc = this.querySelector(Selectors.visibleArc(arcId));
    // Return null if arc doesn't exist or is hidden
    if (!arc || arc.style.display === 'none') return null;
    return arc;
  },
  getHitarea(arcId) { return this.querySelector(Selectors.hitarea(arcId)); },
  // Raw access - returns ALL arrows (including hidden ones, for show/hide operations)
  getArrows(arcId) { return this.querySelectorAll(Selectors.arrows(arcId)); },
  // Filtered access - returns only VISIBLE arrows (for highlight/scale operations)
  getVisibleArrows(arcId) {
    const arrows = this.querySelectorAll(Selectors.arrows(arcId));
    return Array.from(arrows).filter(arr => arr.style.display !== 'none');
  },
  getVirtualArrows(arcId) { return this.querySelectorAll(Selectors.virtualArrows(arcId)); },
  getConnectedHitareas(nodeId) { return this.querySelectorAll(Selectors.connectedHitareas(nodeId)); },
  getLabelGroup(arcId) { return this.querySelector(Selectors.labelGroup(arcId)); },
  getCollapseToggle(nodeId) { return this.querySelector(Selectors.collapseToggle(nodeId)); },
  getCountLabel(nodeId) { return this.getElementById(Selectors.countId(nodeId)); },
  getTreeLines(nodeId, role) {
    const sel = role === 'child' ? Selectors.treeLineChild(nodeId) : Selectors.treeLineParent(nodeId);
    return this.querySelectorAll(sel);
  },
  getSvgRoot() { return this.querySelector('svg'); },
  getAllHitareas() { return this.querySelectorAll(Selectors.allHitareas()); },
};

// Export for Browser
if (typeof window !== "undefined") {
  window.DomAdapter = DomAdapter;
}

// Export for Bun/Node
if (typeof module !== "undefined" && module.exports) {
  module.exports = { DomAdapter, createMockDomAdapter, createFakeElement };
}
