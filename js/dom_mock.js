import { createConvenienceMethods } from './dom_adapter.js';

// dom_mock.js - Fake DOM adapter and elements for tests

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
    style: styleData,
    cloneNode(_deep) {
      const clone = createFakeElement(tagName);
      for (const [k, v] of attrs) clone.setAttribute(k, v);
      for (const c of classes) clone.classList.add(c);
      Object.assign(clone.style, styleData);
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

export { createMockDomAdapter, createFakeElement };
