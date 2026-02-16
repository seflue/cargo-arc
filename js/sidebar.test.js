import { test, expect, describe, beforeEach, afterEach } from "bun:test";
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
    "mod_render": { type: "module", name: "render", parent: "crate_a", x: 0, y: 0, width: 100, height: 30, hasChildren: false },
    "mod_cli": { type: "module", name: "cli", parent: "crate_a", x: 0, y: 0, width: 100, height: 30, hasChildren: false },
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

    test("renders collapse-all button when groups have symbols", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).toContain("sidebar-collapse-all");
      expect(html).toContain("sidebar-header-actions");
    });

    test("does not render collapse-all for Cargo.toml dependency", () => {
      const html = SidebarLogic.buildContent("empty_arc");
      expect(html).not.toContain("sidebar-collapse-all");
      expect(html).not.toContain("sidebar-header-actions");
    });

    test("does not render collapse-all when symbols are empty strings", () => {
      const override = {
        from: "a", to: "b",
        usages: [
          { symbol: "", modulePath: null, locations: [{ file: "a.rs", line: 1 }] },
        ],
      };
      const html = SidebarLogic.buildContent("no-sym-id", override);
      expect(html).not.toContain("sidebar-collapse-all");
      expect(html).not.toContain("sidebar-header-actions");
    });

    test("collapse-all and close button inside header-actions wrapper", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      const actionsMatch = html.match(/<div class="sidebar-header-actions">([\s\S]*?)<\/div>/);
      expect(actionsMatch).not.toBeNull();
      expect(actionsMatch[1]).toContain("sidebar-collapse-all");
      expect(actionsMatch[1]).toContain("sidebar-close");
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
        setAttribute() {},
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

    test("show() removes sidebar-transient class", () => {
      // First make it transient
      fakeEl._innerDiv.classList.add("sidebar-transient");
      SidebarLogic.show("crate_a-crate_b");
      expect(fakeEl._innerDiv.classList.contains("sidebar-transient")).toBe(false);
      expect(SidebarLogic._isTransient).toBe(false);
    });

    test("show() clears debounce timer", () => {
      SidebarLogic._debounceTimer = setTimeout(() => {}, 10000);
      SidebarLogic.show("crate_a-crate_b");
      expect(SidebarLogic._isTransient).toBe(false);
    });

    test("hide() clears transient state", () => {
      SidebarLogic._isTransient = true;
      SidebarLogic._debounceTimer = setTimeout(() => {}, 10000);
      SidebarLogic.show("crate_a-crate_b");
      SidebarLogic.hide();
      expect(SidebarLogic._isTransient).toBe(false);
    });
  });

  describe("showTransient/hideTransient", () => {
    let fakeEl;

    function makeSvgMock(rectTop) {
      return {
        getBoundingClientRect() {
          return { left: 0, top: rectTop ?? 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
        setAttribute() {},
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
      innerDiv.offsetWidth = 0;
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
      SidebarLogic._isTransient = false;
      SidebarLogic._debounceTimer = null;
    });

    test("showTransient() shows sidebar after debounce", async () => {
      SidebarLogic.showTransient("crate_a-crate_b");
      // Before timer fires, sidebar should not be visible yet
      expect(fakeEl.style.display).not.toBe("block");
      // Wait for debounce (30ms + buffer)
      await new Promise(r => setTimeout(r, 50));
      expect(fakeEl.style.display).toBe("block");
      expect(SidebarLogic._isTransient).toBe(true);
    });

    test("showTransient() sets sidebar-transient class", async () => {
      SidebarLogic.showTransient("crate_a-crate_b");
      await new Promise(r => setTimeout(r, 50));
      expect(fakeEl._innerDiv.classList.contains("sidebar-transient")).toBe(true);
    });

    test("hideTransient() hides only transient sidebar", () => {
      // Pin sidebar via show() (not transient)
      SidebarLogic.show("crate_a-crate_b");
      expect(fakeEl.style.display).toBe("block");
      // hideTransient should NOT hide a pinned sidebar
      SidebarLogic.hideTransient();
      expect(fakeEl.style.display).toBe("block");
    });

    test("hideTransient() cancels pending debounce", async () => {
      SidebarLogic.showTransient("crate_a-crate_b");
      // Immediately cancel
      SidebarLogic.hideTransient();
      await new Promise(r => setTimeout(r, 50));
      // Sidebar should remain hidden
      expect(fakeEl.style.display).not.toBe("block");
    });

    test("hideTransient() hides transient sidebar", async () => {
      SidebarLogic.showTransient("crate_a-crate_b");
      await new Promise(r => setTimeout(r, 50));
      expect(fakeEl.style.display).toBe("block");
      SidebarLogic.hideTransient();
      expect(fakeEl.style.display).toBe("none");
      expect(SidebarLogic._isTransient).toBe(false);
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
        setAttribute() {},
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
        setAttribute() {},
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

    test("re-clamps X with actual width when wider than SIDEBAR_MIN_WIDTH (ca-0141)", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      innerDiv.offsetWidth = 400;
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
        setAttribute() {},
      };
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
      // _calcX: maxArcRight=1900, x=1924, viewportRight=2000
      // 1924+280>2000 → x=2000-280-16=1704 (cached with MIN_WIDTH)
      // updatePosition: naturalW=400, width=400
      // Re-clamp: 1704+400+16=2120>2000 → x=2000-400-16=1584
      expect(fakeEl.getAttribute("x")).toBe("1584");
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
        setAttribute() {},
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

    test("sets dynamic width from max-content offsetWidth", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      innerDiv.offsetWidth = 370;
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
        setAttribute() {},
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
      // offsetWidth=370 (max-content), max(280, min(370, 1000*0.5)) = 370, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute("width"))).toBe(382);
    });

    test("caps dynamic width at 50% viewport", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      innerDiv.offsetWidth = 800;
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
        setAttribute() {},
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
      // offsetWidth=800 (max-content), max(280, min(800, 1000*0.5=500)) = 500, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute("width"))).toBe(512);
    });

    test("falls back to 280 when offsetWidth is 0", () => {
      const fakeEl = createFakeElement("foreignObject");
      const innerDiv = createFakeElement("div");
      Object.defineProperty(innerDiv, "innerHTML", {
        get() { return this._innerHTML || ""; },
        set(v) { this._innerHTML = v; },
      });
      innerDiv.offsetWidth = 0;
      fakeEl.querySelector = () => innerDiv;
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
        setAttribute() {},
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
      // offsetWidth=0 (max-content), max(280, min(0, 500)) = 280, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute("width"))).toBe(292);
    });
  });

  describe("collapse-all handler", () => {
    function makeSymbolEl(collapsed) {
      const attrs = new Map();
      const classes = new Set(["sidebar-symbol"]);
      if (collapsed) attrs.set("data-collapsed", "true");
      const toggleEl = {
        _innerHTML: collapsed ? "\u25B8" : "\u25BE",
        get innerHTML() { return this._innerHTML; },
        set innerHTML(v) { this._innerHTML = v; },
      };
      const locsEl = {
        style: { display: collapsed ? "none" : "" },
        classList: { contains(c) { return c === "sidebar-locations"; } },
      };
      return {
        symbolEl: {
          getAttribute(name) { return attrs.get(name) ?? null; },
          setAttribute(name, value) { attrs.set(name, value); },
          removeAttribute(name) { attrs.delete(name); },
          classList: {
            contains(c) { return classes.has(c); },
          },
          querySelector(sel) { if (sel === ".sidebar-toggle") return toggleEl; return null; },
          nextElementSibling: locsEl,
        },
        locsEl,
        toggleEl,
      };
    }

    function makeHandlerDom(symbolDefs) {
      const symbols = symbolDefs.map(d => makeSymbolEl(d.collapsed));
      const symbolEls = symbols.map(s => s.symbolEl);
      const listeners = new Map();
      let collapseAllInner = "\u2212";
      const collapseAllBtn = {
        get innerHTML() { return collapseAllInner; },
        set innerHTML(v) { collapseAllInner = v; },
        addEventListener(evt, fn) {
          if (!listeners.has("collapseAll")) listeners.set("collapseAll", []);
          listeners.get("collapseAll").push(fn);
        },
      };
      const content = {
        querySelectorAll(sel) {
          if (sel === ".sidebar-symbol") return symbolEls;
          return [];
        },
        addEventListener(evt, fn) {
          if (!listeners.has("content")) listeners.set("content", []);
          listeners.get("content").push(fn);
        },
      };
      const root = {
        querySelector(sel) {
          if (sel === ".sidebar-content") return content;
          if (sel === ".sidebar-collapse-all") return collapseAllBtn;
          return null;
        },
      };
      return { root, symbols, collapseAllBtn, listeners };
    }

    test("clicking collapse-all collapses all expanded groups", () => {
      const dom = makeHandlerDom([{ collapsed: false }, { collapsed: false }]);
      SidebarLogic._setupCollapseHandlers(dom.root);
      // Fire collapse-all click
      for (const fn of dom.listeners.get("collapseAll")) fn();
      for (const s of dom.symbols) {
        expect(s.symbolEl.getAttribute("data-collapsed")).toBe("true");
        expect(s.locsEl.style.display).toBe("none");
        expect(s.toggleEl.innerHTML).toBe("\u25B8");
      }
      expect(dom.collapseAllBtn.innerHTML).toBe("+");
    });

    test("clicking twice expands all again", () => {
      const dom = makeHandlerDom([{ collapsed: false }, { collapsed: false }]);
      SidebarLogic._setupCollapseHandlers(dom.root);
      const handlers = dom.listeners.get("collapseAll");
      // First click: collapse all
      for (const fn of handlers) fn();
      // Second click: expand all
      for (const fn of handlers) fn();
      for (const s of dom.symbols) {
        expect(s.symbolEl.getAttribute("data-collapsed")).toBeNull();
        expect(s.locsEl.style.display).toBe("");
        expect(s.toggleEl.innerHTML).toBe("\u25BE");
      }
      expect(dom.collapseAllBtn.innerHTML).toBe("\u2212");
    });

    test("mixed state: collapses remaining expanded", () => {
      const dom = makeHandlerDom([{ collapsed: true }, { collapsed: false }]);
      SidebarLogic._setupCollapseHandlers(dom.root);
      for (const fn of dom.listeners.get("collapseAll")) fn();
      // Both should be collapsed now
      for (const s of dom.symbols) {
        expect(s.symbolEl.getAttribute("data-collapsed")).toBe("true");
        expect(s.locsEl.style.display).toBe("none");
      }
      expect(dom.collapseAllBtn.innerHTML).toBe("+");
    });

    test("no crash when no collapse-all button", () => {
      const content = {
        querySelectorAll() { return []; },
        addEventListener() {},
      };
      const root = {
        querySelector(sel) {
          if (sel === ".sidebar-content") return content;
          return null; // no collapse-all button
        },
      };
      // Should not throw
      expect(() => SidebarLogic._setupCollapseHandlers(root)).not.toThrow();
    });
  });

  describe("showNode/showTransientNode", () => {
    let fakeEl;

    function makeSvgMock(rectTop) {
      return {
        getBoundingClientRect() {
          return { left: 0, top: rectTop ?? 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
        setAttribute() {},
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
      innerDiv.offsetWidth = 0;
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
      SidebarLogic._isTransient = false;
      SidebarLogic._debounceTimer = null;
    });

    test("showNode sets display:block and renders node content", () => {
      const relations = { incoming: [], outgoing: [] };
      SidebarLogic.showNode("crate_a", relations);
      expect(fakeEl.style.display).toBe("block");
      expect(fakeEl._innerDiv.innerHTML).toContain("sidebar-header");
      expect(fakeEl._innerDiv.innerHTML).toContain("No relations");
    });

    test("showNode removes sidebar-transient class", () => {
      fakeEl._innerDiv.classList.add("sidebar-transient");
      const relations = { incoming: [], outgoing: [] };
      SidebarLogic.showNode("crate_a", relations);
      expect(fakeEl._innerDiv.classList.contains("sidebar-transient")).toBe(false);
      expect(SidebarLogic._isTransient).toBe(false);
    });

    test("showTransientNode shows after 30ms debounce", async () => {
      const relations = { incoming: [], outgoing: [] };
      SidebarLogic.showTransientNode("crate_a", relations);
      // Before timer fires
      expect(fakeEl.style.display).not.toBe("block");
      // Wait for debounce (30ms + buffer)
      await new Promise(r => setTimeout(r, 50));
      expect(fakeEl.style.display).toBe("block");
      expect(SidebarLogic._isTransient).toBe(true);
      expect(fakeEl._innerDiv.classList.contains("sidebar-transient")).toBe(true);
    });
  });

  describe("buildNodeContent", () => {
    // Helper: relations with 2 incoming + 1 outgoing for crate_a
    function makeRelations() {
      return {
        incoming: [
          {
            targetId: "mod_render", weight: 5, arcId: "mod_render-crate_a",
            usages: [
              { symbol: "Config", modulePath: "config", locations: [{ file: "src/render.rs", line: 10 }, { file: "src/render.rs", line: 20 }, { file: "src/render.rs", line: 30 }] },
              { symbol: "parse", modulePath: null, locations: [{ file: "src/render.rs", line: 40 }, { file: "src/render.rs", line: 50 }] },
            ],
          },
          {
            targetId: "mod_cli", weight: 3, arcId: "mod_cli-crate_a",
            usages: [
              { symbol: "run", modulePath: "cli", locations: [{ file: "src/cli.rs", line: 5 }, { file: "src/cli.rs", line: 15 }, { file: "src/cli.rs", line: 25 }] },
            ],
          },
        ],
        outgoing: [
          {
            targetId: "crate_b", weight: 2, arcId: "crate_a-crate_b",
            usages: [
              { symbol: "ModuleInfo", modulePath: "graph", locations: [{ file: "src/lib.rs", line: 7 }, { file: "src/lib.rs", line: 12 }] },
            ],
          },
        ],
      };
    }

    test("2 incoming + 1 outgoing renders correct HTML structure", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", makeRelations());
      expect(html).toContain("sidebar-header");
      expect(html).toContain("sidebar-content");
      expect(html).toContain("sidebar-footer");
      // 3 usage-group Level-1 sections (2 incoming + 1 outgoing)
      const level1Matches = html.match(/data-collapsed="true"/g);
      expect(level1Matches).toHaveLength(3);
    });

    test("incoming: selected node is on the right in From→To pair", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", makeRelations());
      // For incoming: [source] → [selected]
      // render → crate_a (incoming from mod_render)
      const renderPairMatch = html.match(/render[\s\S]*?sidebar-arrow[\s\S]*?crate_a/);
      expect(renderPairMatch).not.toBeNull();
    });

    test("outgoing: selected node is on the left in From→To pair", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", makeRelations());
      // For outgoing: [selected] → [target]
      // crate_a → crate_b
      const outPairMatch = html.match(/sidebar-node-selected[\s\S]*?sidebar-arrow[\s\S]*?crate_b/);
      expect(outPairMatch).not.toBeNull();
    });

    test("selected node has sidebar-node-selected class", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", makeRelations());
      expect(html).toContain("sidebar-node-selected");
      // Should appear on the selected node badge (crate_a is type crate)
      expect(html).toContain("sidebar-node-crate sidebar-node-selected");
    });

    test("incoming sections appear before outgoing", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", makeRelations());
      const renderIdx = html.indexOf("render");
      const crate_bIdx = html.indexOf("crate_b");
      expect(renderIdx).toBeLessThan(crate_bIdx);
    });

    test("only incoming: no outgoing block, no divider", () => {
      const relations = { incoming: makeRelations().incoming, outgoing: [] };
      const html = SidebarLogic.buildNodeContent("crate_a", relations);
      expect(html).not.toContain("sidebar-divider");
      // Should not contain any outgoing target
      expect(html).not.toContain("crate_b");
    });

    test("only outgoing: no incoming block", () => {
      const relations = { incoming: [], outgoing: makeRelations().outgoing };
      const html = SidebarLogic.buildNodeContent("crate_a", relations);
      expect(html).not.toContain("sidebar-divider");
      expect(html).not.toContain("render");
      expect(html).toContain("crate_b");
    });

    test("no relations: shows placeholder", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", { incoming: [], outgoing: [] });
      expect(html).toContain("No relations");
    });

    test("Level 1 collapsed, Level 2 expanded", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", makeRelations());
      // Level 1: all data-collapsed="true"
      const collapsedMatches = html.match(/data-collapsed="true"/g);
      expect(collapsedMatches).toHaveLength(3);
      // Level 2 symbols should NOT have data-collapsed
      // Level 2 toggle icons should be ▾ (expanded)
      expect(html).toContain("&#x25BE;");
    });

    test("footer shows correct counts", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", makeRelations());
      // 3 total relations (2 incoming + 1 outgoing)
      // 2 Dependents (incoming), 1 Dependencies (outgoing)
      expect(html).toContain("3 Relations");
      expect(html).toContain("2 Dependents");
      expect(html).toContain("1 Dependencies");
    });

    test("empty usages shows Cargo.toml dependency", () => {
      const relations = {
        incoming: [{ targetId: "mod_render", weight: 0, arcId: "mod_render-crate_a", usages: [] }],
        outgoing: [],
      };
      const html = SidebarLogic.buildNodeContent("crate_a", relations);
      expect(html).toContain("Cargo.toml dependency");
    });

    test("Level 2 sorted by location count descending", () => {
      const html = SidebarLogic.buildNodeContent("crate_a", makeRelations());
      // First incoming relation (mod_render, weight 5): Config (3 locs) before parse (2 locs)
      const configIdx = html.indexOf("Config");
      const parseIdx = html.indexOf("parse");
      expect(configIdx).toBeLessThan(parseIdx);
    });
  });

  describe("buildContent — cycle view", () => {
    let savedArcs, savedCycles;

    beforeEach(() => {
      savedArcs = globalThis.STATIC_DATA.arcs;
      savedCycles = globalThis.STATIC_DATA.cycles;

      // Cycle: A → B → C → A (cycleIds=[0])
      globalThis.STATIC_DATA.arcs = {
        ...savedArcs,
        "A-B": { from: "A", to: "B", cycleIds: [0], usages: [
          { symbol: "sym1", modulePath: null, locations: [{ file: "a.rs", line: 1 }] }
        ]},
        "B-C": { from: "B", to: "C", cycleIds: [0], usages: [
          { symbol: "sym2", modulePath: null, locations: [{ file: "b.rs", line: 1 }, { file: "b.rs", line: 2 }, { file: "b.rs", line: 3 }] }
        ]},
        "C-A": { from: "C", to: "A", cycleIds: [0], usages: [
          { symbol: "sym3", modulePath: null, locations: [{ file: "c.rs", line: 1 }, { file: "c.rs", line: 2 }] }
        ]},
      };
      globalThis.STATIC_DATA.nodes = {
        ...globalThis.STATIC_DATA.nodes,
        A: { type: "module", name: "mod_a", parent: null },
        B: { type: "module", name: "mod_b", parent: null },
        C: { type: "module", name: "mod_c", parent: null },
      };
      globalThis.STATIC_DATA.cycles = [
        { nodes: ["A", "B", "C"], arcs: ["A-B", "B-C", "C-A"] }
      ];
    });

    afterEach(() => {
      globalThis.STATIC_DATA.arcs = savedArcs;
      globalThis.STATIC_DATA.cycles = savedCycles;
    });

    test("cycle-arc: header shows 'Cycle' with edge count", () => {
      const html = SidebarLogic.buildContent("A-B");
      expect(html).toContain("Cycle");
      expect(html).toContain("3 edges");
    });

    test("cycle-arc: all cycle arcs listed", () => {
      const html = SidebarLogic.buildContent("A-B");
      expect(html).toContain("mod_a");
      expect(html).toContain("mod_b");
      expect(html).toContain("mod_c");
      // All three arcs should have their source locations
      expect(html).toContain("a.rs");
      expect(html).toContain("b.rs");
      expect(html).toContain("c.rs");
    });

    test("cycle-arc: sorted ascending by source-location count (weakest link first)", () => {
      const html = SidebarLogic.buildContent("A-B");
      // A→B has 1 loc, C→A has 2 locs, B→C has 3 locs
      // Ascending order: A→B (1) first, then C→A (2), then B→C (3)
      const aIdx = html.indexOf("a.rs");
      const cIdx = html.indexOf("c.rs");
      const bIdx = html.indexOf("b.rs");
      expect(aIdx).toBeLessThan(cIdx);
      expect(cIdx).toBeLessThan(bIdx);
    });

    test("cycle-arc: clicked arc highlighted", () => {
      const html = SidebarLogic.buildContent("A-B");
      expect(html).toContain("sidebar-selected-arc");
    });

    test("non-cycle arc: normal single-arc layout (regression)", () => {
      const html = SidebarLogic.buildContent("crate_a-crate_b");
      expect(html).not.toContain("Cycle (");
      expect(html).toContain("sidebar-header");
      expect(html).toContain("crate_a");
      expect(html).toContain("crate_b");
    });

    test("multi-cycle arc: shows grouped cycles header", () => {
      // Arc B-C belongs to cycle 0 AND cycle 1
      globalThis.STATIC_DATA.arcs["B-C"] = {
        from: "B", to: "C", cycleIds: [0, 1], usages: [
          { symbol: "sym2", modulePath: null, locations: [{ file: "b.rs", line: 1 }] }
        ]
      };
      globalThis.STATIC_DATA.cycles.push(
        { nodes: ["B", "C", "D"], arcs: ["B-C", "C-D", "D-B"] }
      );
      globalThis.STATIC_DATA.arcs["C-D"] = {
        from: "C", to: "D", cycleIds: [1], usages: [
          { symbol: "sym_cd", modulePath: null, locations: [{ file: "c.rs", line: 10 }] }
        ]
      };
      globalThis.STATIC_DATA.arcs["D-B"] = {
        from: "D", to: "B", cycleIds: [1], usages: [
          { symbol: "sym_db", modulePath: null, locations: [{ file: "d.rs", line: 1 }] }
        ]
      };
      globalThis.STATIC_DATA.nodes.D = { type: "module", name: "mod_d", parent: null };

      const html = SidebarLogic.buildContent("B-C");
      // Should show "Cycles (2)" header for multi-cycle
      expect(html).toContain("Cycles (2)");
      // Should contain arcs from both cycles
      expect(html).toContain("a.rs");
      expect(html).toContain("b.rs");
      expect(html).toContain("c.rs");
      expect(html).toContain("d.rs");
    });

    test("single-cycle arc: shows original 'Cycle' header", () => {
      const html = SidebarLogic.buildContent("A-B");
      expect(html).toContain("Cycle (");
      expect(html).toContain("3 edges");
      expect(html).not.toContain("Cycles (");
    });
  });
});
