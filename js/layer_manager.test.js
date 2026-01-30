import { test, expect, describe } from "bun:test";
import { LayerManager } from "./layer_manager.js";
import { createFakeElement, createMockDomAdapter } from "./dom_adapter.js";

describe("LayerManager", () => {
  describe("LAYERS constants", () => {
    test("defines all layer IDs", () => {
      expect(LayerManager.LAYERS.BASE_ARCS).toBe("base-arcs-layer");
      expect(LayerManager.LAYERS.BASE_LABELS).toBe("base-labels-layer");
      expect(LayerManager.LAYERS.HIGHLIGHT_ARCS).toBe("highlight-arcs-layer");
      expect(LayerManager.LAYERS.HIGHLIGHT_LABELS).toBe("highlight-labels-layer");
      expect(LayerManager.LAYERS.HITAREAS).toBe("hitareas-layer");
      expect(LayerManager.LAYERS.HIGHLIGHT_HITAREAS).toBe("highlight-hitareas-layer");
      expect(LayerManager.LAYERS.SHADOWS).toBe("highlight-shadows");
    });
  });

  describe("getLayerForElement", () => {
    test("returns null for null element", () => {
      expect(LayerManager.getLayerForElement(null, false)).toBeNull();
      expect(LayerManager.getLayerForElement(null, true)).toBeNull();
    });

    test("returns arc layer for dep-arc", () => {
      const el = createFakeElement("path");
      el.classList.add("dep-arc");
      expect(LayerManager.getLayerForElement(el, false)).toBe("base-arcs-layer");
      expect(LayerManager.getLayerForElement(el, true)).toBe("highlight-arcs-layer");
    });

    test("returns arc layer for cycle-arc", () => {
      const el = createFakeElement("path");
      el.classList.add("cycle-arc");
      expect(LayerManager.getLayerForElement(el, false)).toBe("base-arcs-layer");
      expect(LayerManager.getLayerForElement(el, true)).toBe("highlight-arcs-layer");
    });

    test("returns arc layer for virtual-arc", () => {
      const el = createFakeElement("path");
      el.classList.add("virtual-arc");
      expect(LayerManager.getLayerForElement(el, false)).toBe("base-arcs-layer");
      expect(LayerManager.getLayerForElement(el, true)).toBe("highlight-arcs-layer");
    });

    test("returns arc layer for polygon (arrow)", () => {
      const el = createFakeElement("polygon");
      expect(LayerManager.getLayerForElement(el, false)).toBe("base-arcs-layer");
      expect(LayerManager.getLayerForElement(el, true)).toBe("highlight-arcs-layer");
    });

    test("returns label layer for arc-count-group", () => {
      const el = createFakeElement("g");
      el.classList.add("arc-count-group");
      expect(LayerManager.getLayerForElement(el, false)).toBe("base-labels-layer");
      expect(LayerManager.getLayerForElement(el, true)).toBe("highlight-labels-layer");
    });

    test("returns hitarea layer for arc-hitarea", () => {
      const el = createFakeElement("path");
      el.classList.add("arc-hitarea");
      expect(LayerManager.getLayerForElement(el, false)).toBe("hitareas-layer");
      expect(LayerManager.getLayerForElement(el, true)).toBe("highlight-hitareas-layer");
    });

    test("returns null for unknown element type", () => {
      const el = createFakeElement("rect");
      el.classList.add("some-class");
      expect(LayerManager.getLayerForElement(el, false)).toBeNull();
      expect(LayerManager.getLayerForElement(el, true)).toBeNull();
    });
  });

  describe("moveToLayer", () => {
    test("appends element to layer", () => {
      const mock = createMockDomAdapter();
      const layer = createFakeElement("g");
      const element = createFakeElement("path");
      mock._registerElement("base-arcs-layer", layer);

      LayerManager.moveToLayer(element, "base-arcs-layer", mock);

      expect(layer.children).toContain(element);
    });

    test("does nothing for null element", () => {
      const mock = createMockDomAdapter();
      const layer = createFakeElement("g");
      mock._registerElement("base-arcs-layer", layer);

      LayerManager.moveToLayer(null, "base-arcs-layer", mock);

      expect(layer.children).toHaveLength(0);
    });

    test("does nothing for null layerId", () => {
      const mock = createMockDomAdapter();
      const element = createFakeElement("path");

      // Should not throw
      LayerManager.moveToLayer(element, null, mock);
    });
  });

  describe("clearLayer", () => {
    test("clears layer innerHTML", () => {
      const mock = createMockDomAdapter();
      const layer = createFakeElement("g");
      layer.innerHTML = "<path/><path/>";
      mock._registerElement("highlight-shadows", layer);

      LayerManager.clearLayer("highlight-shadows", mock);

      expect(layer.innerHTML).toBe("");
    });

    test("does nothing for non-existent layer", () => {
      const mock = createMockDomAdapter();
      // Should not throw
      LayerManager.clearLayer("non-existent", mock);
    });
  });

  describe("moveToHighlightLayer", () => {
    test("moves arc to highlight-arcs-layer", () => {
      const mock = createMockDomAdapter();
      const layer = createFakeElement("g");
      const arc = createFakeElement("path");
      arc.classList.add("dep-arc");
      mock._registerElement("highlight-arcs-layer", layer);

      LayerManager.moveToHighlightLayer(arc, mock);

      expect(layer.children).toContain(arc);
    });

    test("does nothing for unknown element type", () => {
      const mock = createMockDomAdapter();
      const element = createFakeElement("rect");

      // Should not throw
      LayerManager.moveToHighlightLayer(element, mock);
    });
  });

  describe("moveToBaseLayer", () => {
    test("moves arc to base-arcs-layer", () => {
      const mock = createMockDomAdapter();
      const layer = createFakeElement("g");
      const arc = createFakeElement("path");
      arc.classList.add("cycle-arc");
      mock._registerElement("base-arcs-layer", layer);

      LayerManager.moveToBaseLayer(arc, mock);

      expect(layer.children).toContain(arc);
    });

    test("moves label to base-labels-layer", () => {
      const mock = createMockDomAdapter();
      const layer = createFakeElement("g");
      const label = createFakeElement("g");
      label.classList.add("arc-count-group");
      mock._registerElement("base-labels-layer", layer);

      LayerManager.moveToBaseLayer(label, mock);

      expect(layer.children).toContain(label);
    });
  });
});
