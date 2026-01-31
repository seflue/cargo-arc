import { test, expect, describe } from "bun:test";

// Provide STATIC_DATA.classes for selectors (normally injected by render.rs)
globalThis.STATIC_DATA = globalThis.STATIC_DATA || { classes: {} };
Object.assign(globalThis.STATIC_DATA.classes, {
  depArc: "dep-arc", cycleArc: "cycle-arc", virtualArc: "virtual-arc",
  arcHitarea: "arc-hitarea", arcCount: "arc-count", arcCountGroup: "arc-count-group",
  arcCountBg: "arc-count-bg", collapseToggle: "collapse-toggle",
  virtualHitarea: "virtual-hitarea", virtualArrow: "virtual-arrow",
  depArrow: "dep-arrow", cycleArrow: "cycle-arrow",
});

import { Selectors } from "./selectors.js";

// Set Selectors globally (simulating browser environment where it's loaded before dom_adapter.js)
global.Selectors = Selectors;

import { createFakeElement, createMockDomAdapter } from "./dom_adapter.js";

describe("createFakeElement", () => {
  test("setAttribute/getAttribute roundtrip", () => {
    const el = createFakeElement("rect");
    el.setAttribute("id", "my-rect");
    el.setAttribute("width", "100");
    expect(el.getAttribute("id")).toBe("my-rect");
    expect(el.getAttribute("width")).toBe("100");
  });

  test("classList.add/contains/remove", () => {
    const el = createFakeElement("g");
    expect(el.classList.contains("active")).toBe(false);
    el.classList.add("active");
    expect(el.classList.contains("active")).toBe(true);
    el.classList.remove("active");
    expect(el.classList.contains("active")).toBe(false);
  });

  test("style property get/set", () => {
    const el = createFakeElement("path");
    el.style.strokeWidth = "5px";
    el.style.fill = "red";
    expect(el.style.strokeWidth).toBe("5px");
    expect(el.style.fill).toBe("red");
  });

  test("appendChild/removeChild", () => {
    const parent = createFakeElement("g");
    const child = createFakeElement("rect");
    parent.appendChild(child);
    expect(parent.children).toContain(child);
    parent.removeChild(child);
    expect(parent.children).not.toContain(child);
  });
});

describe("createMockDomAdapter", () => {
  test("getElementById tracks calls", () => {
    const mock = createMockDomAdapter();
    mock.getElementById("foo");
    mock.getElementById("bar");
    expect(mock._getCalls("getElementById")).toEqual([["foo"], ["bar"]]);
  });

  test("_registerElement makes getElementById return element", () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement("g");
    mock._registerElement("my-id", el);
    expect(mock.getElementById("my-id")).toBe(el);
  });

  test("querySelector, querySelectorAll, createSvgElement track calls", () => {
    const mock = createMockDomAdapter();
    mock.querySelector(".node");
    mock.querySelectorAll("rect");
    const svgEl = mock.createSvgElement("path");
    expect(mock._getCalls("querySelector")).toEqual([[".node"]]);
    expect(mock._getCalls("querySelectorAll")).toEqual([["rect"]]);
    expect(mock._getCalls("createSvgElement")).toEqual([["path"]]);
    expect(svgEl.tagName).toBe("path");
  });
});

describe("Convenience methods", () => {
  test("getNode uses Selectors.nodeId", () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement("rect");
    mock._registerElement("node-foo", el);
    expect(mock.getNode("foo")).toBe(el);
    expect(mock._getCalls("getElementById")).toContainEqual(["node-foo"]);
  });

  test("getVisibleArc uses Selectors.visibleArc", () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement("path");
    mock._registerSelector(Selectors.visibleArc("a-b"), el);
    expect(mock.getVisibleArc("a-b")).toBe(el);
    expect(mock._getCalls("querySelector")).toContainEqual([Selectors.visibleArc("a-b")]);
  });

  test("getHitarea uses Selectors.hitarea", () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement("path");
    mock._registerSelector(Selectors.hitarea("x-y"), el);
    expect(mock.getHitarea("x-y")).toBe(el);
    expect(mock._getCalls("querySelector")).toContainEqual([Selectors.hitarea("x-y")]);
  });

  test("getArrows uses Selectors.arrows", () => {
    const mock = createMockDomAdapter();
    const arrows = [createFakeElement("polygon"), createFakeElement("polygon")];
    mock._registerSelector(Selectors.arrows("e-id"), arrows);
    expect(mock.getArrows("e-id")).toEqual(arrows);
    expect(mock._getCalls("querySelectorAll")).toContainEqual([Selectors.arrows("e-id")]);
  });

  test("getVirtualArrows uses Selectors.virtualArrows", () => {
    const mock = createMockDomAdapter();
    const arrows = [createFakeElement("polygon")];
    mock._registerSelector(Selectors.virtualArrows("v-id"), arrows);
    expect(mock.getVirtualArrows("v-id")).toEqual(arrows);
  });

  test("getConnectedHitareas uses Selectors.connectedHitareas", () => {
    const mock = createMockDomAdapter();
    const hitareas = [createFakeElement("path"), createFakeElement("path")];
    mock._registerSelector(Selectors.connectedHitareas("node1"), hitareas);
    expect(mock.getConnectedHitareas("node1")).toEqual(hitareas);
  });

  test("getLabelGroup uses Selectors.labelGroup", () => {
    const mock = createMockDomAdapter();
    const group = createFakeElement("g");
    mock._registerSelector(Selectors.labelGroup("arc-1"), group);
    expect(mock.getLabelGroup("arc-1")).toBe(group);
  });

  test("getCollapseToggle uses Selectors.collapseToggle", () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement("text");
    mock._registerSelector(Selectors.collapseToggle("myNode"), el);
    expect(mock.getCollapseToggle("myNode")).toBe(el);
    expect(mock._getCalls("querySelector")).toContainEqual([Selectors.collapseToggle("myNode")]);
  });

  test("getCountLabel uses Selectors.countId", () => {
    const mock = createMockDomAdapter();
    const el = createFakeElement("tspan");
    mock._registerElement(Selectors.countId("node1"), el);
    expect(mock.getCountLabel("node1")).toBe(el);
    expect(mock._getCalls("getElementById")).toContainEqual([Selectors.countId("node1")]);
  });

  test("getTreeLines with role=child uses Selectors.treeLineChild", () => {
    const mock = createMockDomAdapter();
    const lines = [createFakeElement("line"), createFakeElement("line")];
    mock._registerSelector(Selectors.treeLineChild("c1"), lines);
    expect(mock.getTreeLines("c1", "child")).toEqual(lines);
    expect(mock._getCalls("querySelectorAll")).toContainEqual([Selectors.treeLineChild("c1")]);
  });

  test("getTreeLines with role=parent uses Selectors.treeLineParent", () => {
    const mock = createMockDomAdapter();
    const lines = [createFakeElement("line")];
    mock._registerSelector(Selectors.treeLineParent("p1"), lines);
    expect(mock.getTreeLines("p1", "parent")).toEqual(lines);
    expect(mock._getCalls("querySelectorAll")).toContainEqual([Selectors.treeLineParent("p1")]);
  });

  test("getSvgRoot uses querySelector('svg')", () => {
    const mock = createMockDomAdapter();
    const svg = createFakeElement("svg");
    mock._registerSelector("svg", svg);
    expect(mock.getSvgRoot()).toBe(svg);
    expect(mock._getCalls("querySelector")).toContainEqual(["svg"]);
  });

  test("getAllHitareas uses Selectors.allHitareas", () => {
    const mock = createMockDomAdapter();
    const hitareas = [createFakeElement("path"), createFakeElement("path")];
    mock._registerSelector(Selectors.allHitareas(), hitareas);
    expect(mock.getAllHitareas()).toEqual(hitareas);
    expect(mock._getCalls("querySelectorAll")).toContainEqual([Selectors.allHitareas()]);
  });
});
