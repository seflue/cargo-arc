import { test, expect, describe, beforeEach } from "bun:test";
import { createFakeElement } from "./dom_adapter.js";
import { SidebarLogic } from "./sidebar.js";

// Mock Selectors (sidebar.js uses _getMaxArcRightX → Selectors.allArcPaths)
globalThis.Selectors = {
  allArcPaths: () => ".dep-arc, .cycle-arc, .virtual-arc",
};

// Mock STATIC_DATA for buildContent tests (structured object format from Phase 1)
globalThis.STATIC_DATA = {
  nodes: {
    "crate_a": { type: "crate", name: "crate_a", parent: null, x: 0, y: 0, width: 100, height: 30, hasChildren: false },
    "crate_b": { type: "crate", name: "crate_b", parent: null, x: 0, y: 0, width: 100, height: 30, hasChildren: false },
    "x": { type: "module", name: "x", parent: "crate_a", x: 0, y: 0, width: 100, height: 30, hasChildren: false },
    "y": { type: "module", name: "y", parent: "crate_b", x: 0, y: 0, width: 100, height: 30, hasChildren: false },
  },
  arcs: {
    "crate_a-crate_b": {
      from: "crate_a",
      to: "crate_b",
      usages: [
        { symbol: "ModuleInfo", modulePath: "graph", locations: [{ file: "src/cli.rs", line: 7 }, { file: "src/render.rs", line: 12 }] },
        { symbol: "analyze", modulePath: "graph", locations: [{ file: "src/cli.rs", line: 7 }] },
      ],
    },
    "empty_arc": {
      from: "x",
      to: "y",
      usages: [],
    },
  },
};

// Mock StaticData module (sidebar.js uses StaticData.getNode for name resolution)
globalThis.StaticData = {
  getNode(id) {
    return (globalThis.STATIC_DATA.nodes || {})[id] || null;
  },
};

describe("SidebarLogic", () => {
  describe("mergeSymbolGroups", () => {
    test("merges groups with same symbol and combines locations", () => {
      const groups = [
        { symbol: "Foo", modulePath: null, locations: [{ file: "a.rs", line: 1 }, { file: "b.rs", line: 2 }] },
        { symbol: "Foo", modulePath: null, locations: [{ file: "c.rs", line: 3 }] }
      ];
      const result = SidebarLogic.mergeSymbolGroups(groups);

      expect(result.length).toBe(1);
      expect(result[0].symbol).toBe("Foo");
      expect(result[0].locations.length).toBe(3);
      expect(result[0].locations).toContainEqual({ file: "a.rs", line: 1 });
      expect(result[0].locations).toContainEqual({ file: "b.rs", line: 2 });
      expect(result[0].locations).toContainEqual({ file: "c.rs", line: 3 });
    });

    test("deduplicates locations with same file+line", () => {
      const groups = [
        { symbol: "Bar", modulePath: null, locations: [{ file: "x.rs", line: 10 }] },
        { symbol: "Bar", modulePath: null, locations: [{ file: "x.rs", line: 10 }, { file: "y.rs", line: 20 }] }
      ];
      const result = SidebarLogic.mergeSymbolGroups(groups);

      expect(result.length).toBe(1);
      expect(result[0].locations.length).toBe(2);
      expect(result[0].locations).toContainEqual({ file: "x.rs", line: 10 });
      expect(result[0].locations).toContainEqual({ file: "y.rs", line: 20 });
    });

    test("keeps groups with different symbols separate", () => {
      const groups = [
        { symbol: "Alpha", modulePath: null, locations: [{ file: "a.rs", line: 1 }] },
        { symbol: "Beta", modulePath: null, locations: [{ file: "b.rs", line: 2 }] }
      ];
      const result = SidebarLogic.mergeSymbolGroups(groups);

      expect(result.length).toBe(2);
      const symbols = result.map(g => g.symbol);
      expect(symbols).toContain("Alpha");
      expect(symbols).toContain("Beta");
    });

    test("handles empty symbol strings as single group", () => {
      const groups = [
        { symbol: "", modulePath: null, locations: [{ file: "a.rs", line: 1 }] },
        { symbol: "", modulePath: null, locations: [{ file: "b.rs", line: 2 }] }
      ];
      const result = SidebarLogic.mergeSymbolGroups(groups);

      expect(result.length).toBe(1);
      expect(result[0].symbol).toBe("");
      expect(result[0].locations.length).toBe(2);
    });

    test("returns empty array for empty input", () => {
      const result = SidebarLogic.mergeSymbolGroups([]);
      expect(result).toEqual([]);
    });
  });

  describe("buildContent", () => {
    test("header shows from → to from STATIC_DATA", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).toContain("crate_a");
      expect(html).toContain("crate_b");
      expect(html).toContain("sidebar-header");
    });

    test("contains close button", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).toContain("sidebar-close");
      expect(html).toContain("&#x2715;");
    });

    test("renders structured usage groups with symbol and locations", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).toContain("sidebar-usage-group");
      expect(html).toContain("sidebar-symbol");
      expect(html).toContain("ModuleInfo");
      expect(html).toContain("src/cli.rs");
      expect(html).toContain("src/render.rs");
      expect(html).toContain("sidebar-locations");
    });

    test("renders line numbers as badges", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).toContain("sidebar-line-badge");
      expect(html).toContain(":7");
      expect(html).toContain(":12");
    });

    test("empty usages shows Cargo.toml dependency", () => {
      const html = SidebarLogic.buildContent("empty_arc");
      expect(html).toContain("sidebar-header");
      expect(html).toContain("Cargo.toml dependency");
    });

    test("uses overrideData with structured objects", () => {
      const override = {
        from: "parent_crate",
        to: "dep_crate",
        usages: [
          { symbol: "VirtSymbol", modulePath: null, locations: [{ file: "src/virt.rs", line: 42 }] },
        ],
      };
      const html = SidebarLogic.buildContent("nonexistent-id", override);
      expect(html).toContain("parent_crate");
      expect(html).toContain("dep_crate");
      expect(html).toContain("VirtSymbol");
      expect(html).toContain("src/virt.rs");
      expect(html).toContain(":42");
    });

    test("overrideData with empty usages shows Cargo.toml dependency", () => {
      const override = { from: "a", to: "b", usages: [] };
      const html = SidebarLogic.buildContent("whatever", override);
      expect(html).toContain("Cargo.toml dependency");
    });

    test("overrideData with originalArcs shows badge-relations", () => {
      const override = {
        from: "parent",
        to: "child",
        usages: [
          { symbol: "Sym", modulePath: null, locations: [{ file: "a.rs", line: 1 }] },
        ],
        originalArcs: ["arc1", "arc2"],
      };
      const html = SidebarLogic.buildContent("virt-id", override);
      expect(html).toContain("sidebar-badge-relations");
      expect(html).toContain("2 relations");
    });

    test("renders footer with reference and symbol counts", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).toContain("sidebar-footer");
      // 3 locations total (2 + 1), 2 symbols
      expect(html).toContain("3 Referenzen");
      expect(html).toContain("2 Symbole");
    });

    test("bare locations (empty symbol) render without symbol name", () => {
      const override = {
        from: "a", to: "b",
        usages: [
          { symbol: "", modulePath: null, locations: [{ file: "src/lib.rs", line: 1 }] },
        ],
      };
      const html = SidebarLogic.buildContent("bare-id", override);
      expect(html).toContain("src/lib.rs");
      expect(html).toContain(":1");
      expect(html).toContain("sidebar-usage-group");
    });

    test("renders namespace prefix when modulePath is set", () => {
      const override = {
        from: "a", to: "b",
        usages: [
          { symbol: "ModuleInfo", modulePath: "render::sidebar", locations: [{ file: "src/cli.rs", line: 7 }] },
        ],
      };
      const html = SidebarLogic.buildContent("ns-id", override);
      expect(html).toContain('<span class="sidebar-ns">render::sidebar::</span>');
      expect(html).toContain('<span class="sidebar-symbol-name">ModuleInfo</span>');
    });

    test("omits namespace prefix when modulePath is null", () => {
      const override = {
        from: "a", to: "b",
        usages: [
          { symbol: "SomeType", modulePath: null, locations: [{ file: "src/lib.rs", line: 10 }] },
        ],
      };
      const html = SidebarLogic.buildContent("no-ns-id", override);
      expect(html).not.toContain("sidebar-ns");
      expect(html).toContain('<span class="sidebar-symbol-name">SomeType</span>');
    });

    test("symbol name is wrapped in sidebar-symbol-name span", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).toContain('<span class="sidebar-symbol-name">ModuleInfo</span>');
      expect(html).toContain('<span class="sidebar-symbol-name">analyze</span>');
    });
  });

  describe("collapse defaults in buildContent", () => {
    test("all groups start expanded", () => {
      const override = {
        from: "a", to: "b",
        usages: [{
          symbol: "SmallSymbol", modulePath: null,
          locations: [
            { file: "a.rs", line: 1 }, { file: "b.rs", line: 2 },
          ],
        }, {
          symbol: "BigSymbol", modulePath: null,
          locations: [
            { file: "a.rs", line: 1 }, { file: "b.rs", line: 2 },
            { file: "c.rs", line: 3 }, { file: "d.rs", line: 4 },
            { file: "e.rs", line: 5 },
          ],
        }],
      };
      const html = SidebarLogic.buildContent("test-id", override);
      expect(html).not.toContain('data-collapsed="true"');
      expect(html).not.toContain('display:none');
      // Toggle icons should be ▾ (expanded)
      const toggleMatches = html.match(/&#x25BE;/g);
      expect(toggleMatches).toHaveLength(2);
    });

    test("groups sorted by location count descending", () => {
      const override = {
        from: "a", to: "b",
        usages: [
          { symbol: "Few", modulePath: null, locations: [{ file: "a.rs", line: 1 }] },
          { symbol: "Many", modulePath: null, locations: [
            { file: "a.rs", line: 1 }, { file: "b.rs", line: 2 }, { file: "c.rs", line: 3 },
          ]},
          { symbol: "Mid", modulePath: null, locations: [
            { file: "a.rs", line: 1 }, { file: "b.rs", line: 2 },
          ]},
        ],
      };
      const html = SidebarLogic.buildContent("test-id", override);
      const symbolOrder = [...html.matchAll(/<span class="sidebar-symbol-name">([^<]+)<\/span><span class="sidebar-ref-count">/g)]
        .map(m => m[1]);
      expect(symbolOrder).toEqual(["Many", "Mid", "Few"]);
    });

    test("toggle icon present on symbol headers", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).toContain("sidebar-toggle");
    });
  });

  describe("show/hide/isVisible", () => {
    let fakeEl;

    function makeSvgMock(rectTop) {
      return {
        getBoundingClientRect() {
          return { left: 0, top: rectTop ?? 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
      };
    }

    beforeEach(() => {
      fakeEl = createFakeElement("foreignObject");
      fakeEl.innerHTML = "";
      const innerDiv = createFakeElement("div");
      innerDiv._innerHTML = "";
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML; },
        set(v) { this._innerHTML = v; },
      });
      fakeEl._innerDiv = innerDiv;
      fakeEl.querySelector = () => fakeEl._innerDiv;
      const svgMock = makeSvgMock(0);
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === "relation-sidebar") return fakeEl;
          return null;
        },
        getSvgRoot() { return svgMock; },
        querySelector(sel) { if (sel === "svg") return svgMock; return null; },
        querySelectorAll() { return []; },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
    });

    test("show sets display to block and sets content", () => {
      SidebarLogic.show("crate_a-crate_b");
      expect(fakeEl.style.display).toBe("block");
      expect(fakeEl._innerDiv.innerHTML).toContain("sidebar-header");
    });

    test("hide sets display to none", () => {
      SidebarLogic.show("crate_a-crate_b");
      SidebarLogic.hide();
      expect(fakeEl.style.display).toBe("none");
    });

    test("isVisible returns correct state", () => {
      expect(SidebarLogic.isVisible()).toBe(false);
      SidebarLogic.show("crate_a-crate_b");
      expect(SidebarLogic.isVisible()).toBe(true);
      SidebarLogic.hide();
      expect(SidebarLogic.isVisible()).toBe(false);
    });
  });

  describe("updatePosition", () => {
    test("positions right of arcs with fallback to viewport edge", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: -300, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
      };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === "relation-sidebar") return fakeEl;
          return null;
        },
        getSvgRoot() { return svgMock; },
        querySelectorAll() { return []; },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show("crate_a-crate_b");
      // No arcs → maxArcRight=0, x=0+24=24
      // viewportRight = (1000-0)*2 = 2000, 24+280=304 < 2000 → no fallback
      expect(fakeEl.getAttribute("x")).toBe("24");
      // scaleY = 1600/800 = 2, scrollTop = max(0,300)*2 = 600
      // y = 600 + TOOLBAR_HEIGHT(0 in test) + GAP_TOP(20) = 620
      expect(fakeEl.getAttribute("y")).toBe("620");
    });

    test("falls back to viewport edge when arcs are too wide", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
      };
      // Mock an arc at x=1800, width=100 → right edge at 1900
      const fakeArc = { style: { display: '' }, getBBox() { return { x: 1800, width: 100 }; } };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === "relation-sidebar") return fakeEl;
          return null;
        },
        getSvgRoot() { return svgMock; },
        querySelectorAll() { return [fakeArc]; },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show("crate_a-crate_b");
      // maxArcRight=1900, x=1900+24=1924
      // viewportRight = (1000-0)*2 = 2000, 1924+280=2204 > 2000
      // fallback: x = 2000-280-16 = 1704
      expect(fakeEl.getAttribute("x")).toBe("1704");
    });

    test("height is capped at MAX_HEIGHT", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      fakeEl.querySelector = () => innerDiv;
      // Large viewport: innerHeight=2000 * scaleY=2 = 4000 SVG units
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
      };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === "relation-sidebar") return fakeEl;
          return null;
        },
        getSvgRoot() { return svgMock; },
        querySelectorAll() { return []; },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 2000;
      SidebarLogic.show("crate_a-crate_b");
      // vpHeight = 2000 * (1600/800) = 4000, 4000 - 0 - 20 = 3980, capped at 500
      // Inner div gets content height, foreignObject gets +12 for shadow padding
      expect(parseInt(innerDiv.style.height)).toBeLessThanOrEqual(500);
    });

    test("sets dynamic width from scrollWidth", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      innerDiv.scrollWidth = 350;
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
      };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === "relation-sidebar") return fakeEl;
          return null;
        },
        getSvgRoot() { return svgMock; },
        querySelectorAll() { return []; },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show("crate_a-crate_b");
      // scrollWidth=350 + 20 = 370, max(280, min(370, 1000*0.5)) = 370, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute("width"))).toBe(382);
    });

    test("caps dynamic width at 50% viewport", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      innerDiv.scrollWidth = 800;
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
      };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === "relation-sidebar") return fakeEl;
          return null;
        },
        getSvgRoot() { return svgMock; },
        querySelectorAll() { return []; },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show("crate_a-crate_b");
      // scrollWidth=800+20=820, max(280, min(820, 1000*0.5=500)) = 500, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute("width"))).toBe(512);
    });

    test("falls back to 280 when scrollWidth is 0", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      innerDiv.scrollWidth = 0;
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
      };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === "relation-sidebar") return fakeEl;
          return null;
        },
        getSvgRoot() { return svgMock; },
        querySelectorAll() { return []; },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show("crate_a-crate_b");
      // scrollWidth=0+20=20, max(280, min(20, 500)) = 280, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute("width"))).toBe(292);
    });
  });
});
