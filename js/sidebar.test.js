import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { createFakeElement } from './dom_adapter.js';
import { SidebarLogic } from './sidebar.js';

// Mock Selectors (sidebar.js uses _getMaxArcRightX → Selectors.allArcPaths)
globalThis.Selectors = {
  allArcPaths: () => '.dep-arc, .cycle-arc, .virtual-arc',
};

// Mock STATIC_DATA for buildContent tests (structured object format from Phase 1)
globalThis.STATIC_DATA = {
  nodes: {
    crate_a: {
      type: 'crate',
      name: 'crate_a',
      parent: null,
      x: 0,
      y: 0,
      width: 100,
      height: 30,
      hasChildren: false,
    },
    crate_b: {
      type: 'crate',
      name: 'crate_b',
      parent: null,
      x: 0,
      y: 0,
      width: 100,
      height: 30,
      hasChildren: false,
    },
    x: {
      type: 'module',
      name: 'x',
      parent: 'crate_a',
      x: 0,
      y: 0,
      width: 100,
      height: 30,
      hasChildren: false,
    },
    y: {
      type: 'module',
      name: 'y',
      parent: 'crate_b',
      x: 0,
      y: 0,
      width: 100,
      height: 30,
      hasChildren: false,
    },
    mod_render: {
      type: 'module',
      name: 'render',
      parent: 'crate_a',
      x: 0,
      y: 0,
      width: 100,
      height: 30,
      hasChildren: false,
    },
    mod_cli: {
      type: 'module',
      name: 'cli',
      parent: 'crate_a',
      x: 0,
      y: 0,
      width: 100,
      height: 30,
      hasChildren: false,
    },
  },
  arcs: {
    'crate_a-crate_b': {
      from: 'crate_a',
      to: 'crate_b',
      usages: [
        {
          symbol: 'ModuleInfo',
          modulePath: 'graph',
          locations: [
            { file: 'src/cli.rs', line: 7 },
            { file: 'src/render.rs', line: 12 },
          ],
        },
        {
          symbol: 'analyze',
          modulePath: 'graph',
          locations: [{ file: 'src/cli.rs', line: 7 }],
        },
      ],
    },
    empty_arc: {
      from: 'x',
      to: 'y',
      usages: [],
    },
  },
};

// Mock StaticData module (sidebar.js uses StaticData.getNode for name resolution)
globalThis.StaticData = {
  getNode(id) {
    return globalThis.STATIC_DATA.nodes?.[id] || null;
  },
};

describe('SidebarLogic', () => {
  describe('mergeSymbolGroups', () => {
    test('merges groups with same symbol and combines locations', () => {
      const groups = [
        {
          symbol: 'Foo',
          modulePath: null,
          locations: [
            { file: 'a.rs', line: 1 },
            { file: 'b.rs', line: 2 },
          ],
        },
        {
          symbol: 'Foo',
          modulePath: null,
          locations: [{ file: 'c.rs', line: 3 }],
        },
      ];
      const result = SidebarLogic.mergeSymbolGroups(groups);

      expect(result.length).toBe(1);
      expect(result[0].symbol).toBe('Foo');
      expect(result[0].locations.length).toBe(3);
      expect(result[0].locations).toContainEqual({ file: 'a.rs', line: 1 });
      expect(result[0].locations).toContainEqual({ file: 'b.rs', line: 2 });
      expect(result[0].locations).toContainEqual({ file: 'c.rs', line: 3 });
    });

    test('deduplicates locations with same file+line', () => {
      const groups = [
        {
          symbol: 'Bar',
          modulePath: null,
          locations: [{ file: 'x.rs', line: 10 }],
        },
        {
          symbol: 'Bar',
          modulePath: null,
          locations: [
            { file: 'x.rs', line: 10 },
            { file: 'y.rs', line: 20 },
          ],
        },
      ];
      const result = SidebarLogic.mergeSymbolGroups(groups);

      expect(result.length).toBe(1);
      expect(result[0].locations.length).toBe(2);
      expect(result[0].locations).toContainEqual({ file: 'x.rs', line: 10 });
      expect(result[0].locations).toContainEqual({ file: 'y.rs', line: 20 });
    });

    test('keeps groups with different symbols separate', () => {
      const groups = [
        {
          symbol: 'Alpha',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: 'Beta',
          modulePath: null,
          locations: [{ file: 'b.rs', line: 2 }],
        },
      ];
      const result = SidebarLogic.mergeSymbolGroups(groups);

      expect(result.length).toBe(2);
      const symbols = result.map((g) => g.symbol);
      expect(symbols).toContain('Alpha');
      expect(symbols).toContain('Beta');
    });

    test('handles empty symbol strings as single group', () => {
      const groups = [
        {
          symbol: '',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: '',
          modulePath: null,
          locations: [{ file: 'b.rs', line: 2 }],
        },
      ];
      const result = SidebarLogic.mergeSymbolGroups(groups);

      expect(result.length).toBe(1);
      expect(result[0].symbol).toBe('');
      expect(result[0].locations.length).toBe(2);
    });

    test('returns empty array for empty input', () => {
      const result = SidebarLogic.mergeSymbolGroups([]);
      expect(result).toEqual([]);
    });
  });

  describe('buildContent', () => {
    test('header shows from → to from STATIC_DATA', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain('crate_a');
      expect(html).toContain('crate_b');
      expect(html).toContain('sidebar-header');
    });

    test('contains close button', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain('sidebar-close');
      expect(html).toContain('&#x2715;');
    });

    test('renders structured usage groups with symbol and locations', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain('sidebar-usage-group');
      expect(html).toContain('sidebar-symbol');
      expect(html).toContain('ModuleInfo');
      expect(html).toContain('src/cli.rs');
      expect(html).toContain('src/render.rs');
      expect(html).toContain('sidebar-locations');
    });

    test('renders line numbers as badges', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain('sidebar-line-badge');
      expect(html).toContain(':7');
      expect(html).toContain(':12');
    });

    test('empty usages shows Cargo.toml dependency', () => {
      const html = SidebarLogic.buildContent('empty_arc');
      expect(html).toContain('sidebar-header');
      expect(html).toContain('Cargo.toml dependency');
    });

    test('uses overrideData with structured objects', () => {
      const override = {
        from: 'parent_crate',
        to: 'dep_crate',
        usages: [
          {
            symbol: 'VirtSymbol',
            modulePath: null,
            locations: [{ file: 'src/virt.rs', line: 42 }],
          },
        ],
      };
      const html = SidebarLogic.buildContent('nonexistent-id', override);
      expect(html).toContain('parent_crate');
      expect(html).toContain('dep_crate');
      expect(html).toContain('VirtSymbol');
      expect(html).toContain('src/virt.rs');
      expect(html).toContain(':42');
    });

    test('overrideData with empty usages shows Cargo.toml dependency', () => {
      const override = { from: 'a', to: 'b', usages: [] };
      const html = SidebarLogic.buildContent('whatever', override);
      expect(html).toContain('Cargo.toml dependency');
    });

    test('renders footer with reference and symbol counts', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain('sidebar-footer');
      // 3 locations total (2 + 1), 2 symbols
      expect(html).toContain('3 Referenzen');
      expect(html).toContain('2 Symbole');
    });

    test('bare locations (empty symbol) render without symbol name', () => {
      const override = {
        from: 'a',
        to: 'b',
        usages: [
          {
            symbol: '',
            modulePath: null,
            locations: [{ file: 'src/lib.rs', line: 1 }],
          },
        ],
      };
      const html = SidebarLogic.buildContent('bare-id', override);
      expect(html).toContain('src/lib.rs');
      expect(html).toContain(':1');
      expect(html).toContain('sidebar-usage-group');
    });

    test('renders namespace prefix when modulePath is set', () => {
      const override = {
        from: 'a',
        to: 'b',
        usages: [
          {
            symbol: 'ModuleInfo',
            modulePath: 'render::sidebar',
            locations: [{ file: 'src/cli.rs', line: 7 }],
          },
        ],
      };
      const html = SidebarLogic.buildContent('ns-id', override);
      expect(html).toContain(
        '<span class="sidebar-ns">render::sidebar::</span>',
      );
      expect(html).toContain(
        '<span class="sidebar-symbol-name">ModuleInfo</span>',
      );
    });

    test('omits namespace prefix when modulePath is null', () => {
      const override = {
        from: 'a',
        to: 'b',
        usages: [
          {
            symbol: 'SomeType',
            modulePath: null,
            locations: [{ file: 'src/lib.rs', line: 10 }],
          },
        ],
      };
      const html = SidebarLogic.buildContent('no-ns-id', override);
      expect(html).not.toContain('sidebar-ns');
      expect(html).toContain(
        '<span class="sidebar-symbol-name">SomeType</span>',
      );
    });

    test('symbol name is wrapped in sidebar-symbol-name span', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain(
        '<span class="sidebar-symbol-name">ModuleInfo</span>',
      );
      expect(html).toContain(
        '<span class="sidebar-symbol-name">analyze</span>',
      );
    });

    test('renders collapse-all button when groups have symbols', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain('sidebar-collapse-all');
      expect(html).toContain('sidebar-header-actions');
    });

    test('does not render collapse-all for Cargo.toml dependency', () => {
      const html = SidebarLogic.buildContent('empty_arc');
      expect(html).not.toContain('sidebar-collapse-all');
      expect(html).not.toContain('sidebar-header-actions');
    });

    test('does not render collapse-all when symbols are empty strings', () => {
      const override = {
        from: 'a',
        to: 'b',
        usages: [
          {
            symbol: '',
            modulePath: null,
            locations: [{ file: 'a.rs', line: 1 }],
          },
        ],
      };
      const html = SidebarLogic.buildContent('no-sym-id', override);
      expect(html).not.toContain('sidebar-collapse-all');
      expect(html).not.toContain('sidebar-header-actions');
    });

    test('collapse-all and close button inside header-actions wrapper', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      const actionsMatch = html.match(
        /<div class="sidebar-header-actions">([\s\S]*?)<\/div>/,
      );
      expect(actionsMatch).not.toBeNull();
      expect(actionsMatch[1]).toContain('sidebar-collapse-all');
      expect(actionsMatch[1]).toContain('sidebar-close');
    });
  });

  describe('collapse defaults in buildContent', () => {
    test('all groups start expanded', () => {
      const override = {
        from: 'a',
        to: 'b',
        usages: [
          {
            symbol: 'SmallSymbol',
            modulePath: null,
            locations: [
              { file: 'a.rs', line: 1 },
              { file: 'b.rs', line: 2 },
            ],
          },
          {
            symbol: 'BigSymbol',
            modulePath: null,
            locations: [
              { file: 'a.rs', line: 1 },
              { file: 'b.rs', line: 2 },
              { file: 'c.rs', line: 3 },
              { file: 'd.rs', line: 4 },
              { file: 'e.rs', line: 5 },
            ],
          },
        ],
      };
      const html = SidebarLogic.buildContent('test-id', override);
      expect(html).not.toContain('data-collapsed="true"');
      expect(html).not.toContain('display:none');
      // Toggle icons should be ▾ (expanded)
      const toggleMatches = html.match(/&#x25BE;/g);
      expect(toggleMatches).toHaveLength(2);
    });

    test('groups sorted by location count descending', () => {
      const override = {
        from: 'a',
        to: 'b',
        usages: [
          {
            symbol: 'Few',
            modulePath: null,
            locations: [{ file: 'a.rs', line: 1 }],
          },
          {
            symbol: 'Many',
            modulePath: null,
            locations: [
              { file: 'a.rs', line: 1 },
              { file: 'b.rs', line: 2 },
              { file: 'c.rs', line: 3 },
            ],
          },
          {
            symbol: 'Mid',
            modulePath: null,
            locations: [
              { file: 'a.rs', line: 1 },
              { file: 'b.rs', line: 2 },
            ],
          },
        ],
      };
      const html = SidebarLogic.buildContent('test-id', override);
      const symbolOrder = [
        ...html.matchAll(
          /<span class="sidebar-symbol-name">([^<]+)<\/span><span class="sidebar-ref-count">/g,
        ),
      ].map((m) => m[1]);
      expect(symbolOrder).toEqual(['Many', 'Mid', 'Few']);
    });

    test('toggle icon present on symbol headers', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain('sidebar-toggle');
    });
  });

  describe('show/hide/isVisible', () => {
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
      fakeEl = createFakeElement('foreignObject');
      fakeEl.innerHTML = '';
      const innerDiv = createFakeElement('div');
      innerDiv._innerHTML = '';
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML;
        },
        set(v) {
          this._innerHTML = v;
        },
      });
      fakeEl._innerDiv = innerDiv;
      fakeEl.querySelector = () => fakeEl._innerDiv;
      const svgMock = makeSvgMock(0);
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelector(sel) {
          if (sel === 'svg') return svgMock;
          return null;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
    });

    test('show sets display to block and sets content', () => {
      SidebarLogic.show('crate_a-crate_b');
      expect(fakeEl.style.display).toBe('block');
      expect(fakeEl._innerDiv.innerHTML).toContain('sidebar-header');
    });

    test('hide sets display to none', () => {
      SidebarLogic.show('crate_a-crate_b');
      SidebarLogic.hide();
      expect(fakeEl.style.display).toBe('none');
    });

    test('isVisible returns correct state', () => {
      expect(SidebarLogic.isVisible()).toBe(false);
      SidebarLogic.show('crate_a-crate_b');
      expect(SidebarLogic.isVisible()).toBe(true);
      SidebarLogic.hide();
      expect(SidebarLogic.isVisible()).toBe(false);
    });

    test('show() removes sidebar-transient class', () => {
      // First make it transient
      fakeEl._innerDiv.classList.add('sidebar-transient');
      SidebarLogic.show('crate_a-crate_b');
      expect(fakeEl._innerDiv.classList.contains('sidebar-transient')).toBe(
        false,
      );
      expect(SidebarLogic._isTransient).toBe(false);
    });

    test('show() clears debounce timer', () => {
      SidebarLogic._debounceTimer = setTimeout(() => {}, 10000);
      SidebarLogic.show('crate_a-crate_b');
      expect(SidebarLogic._isTransient).toBe(false);
    });

    test('hide() clears transient state', () => {
      SidebarLogic._isTransient = true;
      SidebarLogic._debounceTimer = setTimeout(() => {}, 10000);
      SidebarLogic.show('crate_a-crate_b');
      SidebarLogic.hide();
      expect(SidebarLogic._isTransient).toBe(false);
    });
  });

  describe('showTransient/hideTransient', () => {
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
      fakeEl = createFakeElement('foreignObject');
      fakeEl.innerHTML = '';
      const innerDiv = createFakeElement('div');
      innerDiv._innerHTML = '';
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML;
        },
        set(v) {
          this._innerHTML = v;
        },
      });
      innerDiv.offsetWidth = 0;
      fakeEl._innerDiv = innerDiv;
      fakeEl.querySelector = () => fakeEl._innerDiv;
      const svgMock = makeSvgMock(0);
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelector(sel) {
          if (sel === 'svg') return svgMock;
          return null;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic._isTransient = false;
      SidebarLogic._debounceTimer = null;
    });

    test('showTransient() shows sidebar after debounce', async () => {
      SidebarLogic.showTransient('crate_a-crate_b');
      // Before timer fires, sidebar should not be visible yet
      expect(fakeEl.style.display).not.toBe('block');
      // Wait for debounce (30ms + buffer)
      await new Promise((r) => setTimeout(r, 50));
      expect(fakeEl.style.display).toBe('block');
      expect(SidebarLogic._isTransient).toBe(true);
    });

    test('showTransient() sets sidebar-transient class', async () => {
      SidebarLogic.showTransient('crate_a-crate_b');
      await new Promise((r) => setTimeout(r, 50));
      expect(fakeEl._innerDiv.classList.contains('sidebar-transient')).toBe(
        true,
      );
    });

    test('hideTransient() hides only transient sidebar', () => {
      // Pin sidebar via show() (not transient)
      SidebarLogic.show('crate_a-crate_b');
      expect(fakeEl.style.display).toBe('block');
      // hideTransient should NOT hide a pinned sidebar
      SidebarLogic.hideTransient();
      expect(fakeEl.style.display).toBe('block');
    });

    test('hideTransient() cancels pending debounce', async () => {
      SidebarLogic.showTransient('crate_a-crate_b');
      // Immediately cancel
      SidebarLogic.hideTransient();
      await new Promise((r) => setTimeout(r, 50));
      // Sidebar should remain hidden
      expect(fakeEl.style.display).not.toBe('block');
    });

    test('hideTransient() hides transient sidebar', async () => {
      SidebarLogic.showTransient('crate_a-crate_b');
      await new Promise((r) => setTimeout(r, 50));
      expect(fakeEl.style.display).toBe('block');
      SidebarLogic.hideTransient();
      expect(fakeEl.style.display).toBe('none');
      expect(SidebarLogic._isTransient).toBe(false);
    });
  });

  describe('updatePosition', () => {
    test('positions right of arcs with fallback to viewport edge', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
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
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show('crate_a-crate_b');
      // No arcs → maxArcRight=0, x=0+24=24
      // viewportRight = (1000-0)*2 = 2000, 24+280=304 < 2000 → no fallback
      expect(fakeEl.getAttribute('x')).toBe('24');
      // scaleY = 1600/800 = 2, scrollTop = max(0,300)*2 = 600
      // y = 600 + TOOLBAR_HEIGHT(0 in test) + GAP_TOP(20) = 620
      expect(fakeEl.getAttribute('y')).toBe('620');
    });

    test('falls back to viewport edge when arcs are too wide', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
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
      const fakeArc = {
        style: { display: '' },
        getBBox() {
          return { x: 1800, width: 100 };
        },
      };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [fakeArc];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show('crate_a-crate_b');
      // maxArcRight=1900, x=1900+24=1924
      // viewportRight = (1000-0)*2 = 2000, 1924+280=2204 > 2000
      // fallback: x = 2000-280-16 = 1704
      expect(fakeEl.getAttribute('x')).toBe('1704');
    });

    test('re-clamps X with actual width when wider than SIDEBAR_MIN_WIDTH (ca-0141)', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
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
      const fakeArc = {
        style: { display: '' },
        getBBox() {
          return { x: 1800, width: 100 };
        },
      };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [fakeArc];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show('crate_a-crate_b');
      // _calcX: maxArcRight=1900, x=1924, viewportRight=2000
      // 1924+280>2000 → x=2000-280-16=1704 (cached with MIN_WIDTH)
      // updatePosition: naturalW=400, width=400
      // Re-clamp: 1704+400+16=2120>2000 → x=2000-400-16=1584
      expect(fakeEl.getAttribute('x')).toBe('1584');
    });

    test('height uses content height when it fits within viewport', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
      });
      innerDiv.offsetHeight = 800;
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
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 2000;
      SidebarLogic.show('crate_a-crate_b');
      // vpHeight = 2000 * (1600/800) = 4000, viewport cap = 4000 - 0 - 20 = 3980
      // naturalH=800 < 3980 → effectiveH=800 (content fits, no capping)
      expect(parseInt(innerDiv.style.height, 10)).toBe(800);
      expect(parseInt(fakeEl.getAttribute('height'), 10)).toBe(812);
    });

    test('sets dynamic width from max-content offsetWidth', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
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
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show('crate_a-crate_b');
      // offsetWidth=370 (max-content), max(280, min(370, 1000*0.5)) = 370, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute('width'), 10)).toBe(382);
    });

    test('caps dynamic width at 50% viewport', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
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
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show('crate_a-crate_b');
      // offsetWidth=800 (max-content), max(280, min(800, 1000*0.5=500)) = 500, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute('width'), 10)).toBe(512);
    });

    test('falls back to 280 when offsetWidth is 0', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
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
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic.show('crate_a-crate_b');
      // offsetWidth=0 (max-content), max(280, min(0, 500)) = 280, +12 shadow pad
      expect(parseInt(fakeEl.getAttribute('width'), 10)).toBe(292);
    });

    test('height shrinks to content when content is shorter than max', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
      });
      innerDiv.offsetHeight = 200;
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
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 2000;
      SidebarLogic.show('crate_a-crate_b');
      // vpHeight = 2000 * 2 = 4000, cap = min(4000-0-20, 500) = 500
      // naturalH=200 < cap=500, so effectiveH=200 (shrink-to-content)
      expect(parseInt(innerDiv.style.height, 10)).toBe(200);
      expect(parseInt(fakeEl.getAttribute('height'), 10)).toBe(212);
    });

    test('height capped when content exceeds viewport limit', () => {
      const fakeEl = createFakeElement('foreignObject');
      const innerDiv = createFakeElement('div');
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML || '';
        },
        set(v) {
          this._innerHTML = v;
        },
      });
      innerDiv.offsetHeight = 800;
      fakeEl.querySelector = () => innerDiv;
      // Small viewport: innerHeight=300 * scaleY=2 = 600 SVG units
      const svgMock = {
        getBoundingClientRect() {
          return { left: 0, top: 0, width: 1000, height: 800 };
        },
        viewBox: { baseVal: { width: 2000, height: 1600 } },
        setAttribute() {},
      };
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 300;
      SidebarLogic.show('crate_a-crate_b');
      // vpHeight = 300 * (1600/800) = 600, viewport cap = 600 - 0 - 20 = 580
      // naturalH=800 > cap=580, so effectiveH=580 (viewport-capped)
      expect(parseInt(innerDiv.style.height, 10)).toBe(580);
      expect(parseInt(fakeEl.getAttribute('height'), 10)).toBe(592);
    });
  });

  describe('collapse-all handler', () => {
    function makeSymbolEl(collapsed) {
      const attrs = new Map();
      const classes = new Set(['sidebar-symbol']);
      if (collapsed) attrs.set('data-collapsed', 'true');
      const toggleEl = {
        _innerHTML: collapsed ? '\u25B8' : '\u25BE',
        get innerHTML() {
          return this._innerHTML;
        },
        set innerHTML(v) {
          this._innerHTML = v;
        },
      };
      const locsEl = {
        style: { display: collapsed ? 'none' : '' },
        classList: {
          contains(c) {
            return c === 'sidebar-locations';
          },
        },
      };
      return {
        symbolEl: {
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
            contains(c) {
              return classes.has(c);
            },
          },
          querySelector(sel) {
            if (sel === '.sidebar-toggle') return toggleEl;
            return null;
          },
          nextElementSibling: locsEl,
        },
        locsEl,
        toggleEl,
      };
    }

    function makeHandlerDom(symbolDefs) {
      const symbols = symbolDefs.map((d) => makeSymbolEl(d.collapsed));
      const symbolEls = symbols.map((s) => s.symbolEl);
      const listeners = new Map();
      let collapseAllInner = '\u2212';
      const collapseAllBtn = {
        get innerHTML() {
          return collapseAllInner;
        },
        set innerHTML(v) {
          collapseAllInner = v;
        },
        addEventListener(_evt, fn) {
          if (!listeners.has('collapseAll')) listeners.set('collapseAll', []);
          listeners.get('collapseAll').push(fn);
        },
      };
      const content = {
        querySelectorAll(sel) {
          if (sel === '.sidebar-symbol') return symbolEls;
          if (sel === ':scope > .sidebar-usage-group > .sidebar-symbol')
            return symbolEls;
          return [];
        },
        addEventListener(_evt, fn) {
          if (!listeners.has('content')) listeners.set('content', []);
          listeners.get('content').push(fn);
        },
      };
      const root = {
        querySelector(sel) {
          if (sel === '.sidebar-content') return content;
          if (sel === '.sidebar-collapse-all') return collapseAllBtn;
          return null;
        },
      };
      return { root, symbols, collapseAllBtn, listeners };
    }

    test('clicking collapse-all collapses all expanded groups', () => {
      const dom = makeHandlerDom([{ collapsed: false }, { collapsed: false }]);
      SidebarLogic._setupCollapseHandlers(dom.root);
      // Fire collapse-all click
      for (const fn of dom.listeners.get('collapseAll')) fn();
      for (const s of dom.symbols) {
        expect(s.symbolEl.getAttribute('data-collapsed')).toBe('true');
        expect(s.locsEl.style.display).toBe('none');
        expect(s.toggleEl.innerHTML).toBe('\u25B8');
      }
      expect(dom.collapseAllBtn.innerHTML).toBe('+');
    });

    test('clicking twice expands all again', () => {
      const dom = makeHandlerDom([{ collapsed: false }, { collapsed: false }]);
      SidebarLogic._setupCollapseHandlers(dom.root);
      const handlers = dom.listeners.get('collapseAll');
      // First click: collapse all
      for (const fn of handlers) fn();
      // Second click: expand all
      for (const fn of handlers) fn();
      for (const s of dom.symbols) {
        expect(s.symbolEl.getAttribute('data-collapsed')).toBeNull();
        expect(s.locsEl.style.display).toBe('');
        expect(s.toggleEl.innerHTML).toBe('\u25BE');
      }
      expect(dom.collapseAllBtn.innerHTML).toBe('\u2212');
    });

    test('mixed state: collapses remaining expanded', () => {
      const dom = makeHandlerDom([{ collapsed: true }, { collapsed: false }]);
      SidebarLogic._setupCollapseHandlers(dom.root);
      for (const fn of dom.listeners.get('collapseAll')) fn();
      // Both should be collapsed now
      for (const s of dom.symbols) {
        expect(s.symbolEl.getAttribute('data-collapsed')).toBe('true');
        expect(s.locsEl.style.display).toBe('none');
      }
      expect(dom.collapseAllBtn.innerHTML).toBe('+');
    });

    function makeNestedHandlerDom(l1Defs) {
      const l1Symbols = l1Defs.map((d) => makeSymbolEl(d.collapsed));
      const l2Symbols = l1Defs.flatMap((d) =>
        (d.l2 || []).map((l2d) => makeSymbolEl(l2d.collapsed)),
      );
      const l1Els = l1Symbols.map((s) => s.symbolEl);
      const allEls = [...l1Els, ...l2Symbols.map((s) => s.symbolEl)];
      const listeners = new Map();
      let collapseAllInner = '+';
      const collapseAllBtn = {
        get innerHTML() {
          return collapseAllInner;
        },
        set innerHTML(v) {
          collapseAllInner = v;
        },
        addEventListener(_evt, fn) {
          if (!listeners.has('collapseAll')) listeners.set('collapseAll', []);
          listeners.get('collapseAll').push(fn);
        },
      };
      const content = {
        querySelectorAll(sel) {
          if (sel === '.sidebar-symbol') return allEls;
          if (sel === ':scope > .sidebar-usage-group > .sidebar-symbol')
            return l1Els;
          return [];
        },
        addEventListener(_evt, fn) {
          if (!listeners.has('content')) listeners.set('content', []);
          listeners.get('content').push(fn);
        },
      };
      const root = {
        querySelector(sel) {
          if (sel === '.sidebar-content') return content;
          if (sel === '.sidebar-collapse-all') return collapseAllBtn;
          return null;
        },
      };
      return { root, l1Symbols, l2Symbols, collapseAllBtn, listeners };
    }

    test('collapse-all ignores nested L2 symbols (first click expands L1)', () => {
      const dom = makeNestedHandlerDom([
        { collapsed: true, l2: [{ collapsed: false }] },
        { collapsed: true, l2: [{ collapsed: false }] },
      ]);
      SidebarLogic._setupCollapseHandlers(dom.root);
      for (const fn of dom.listeners.get('collapseAll')) fn();
      // All L1 were collapsed → should expand all L1
      for (const s of dom.l1Symbols) {
        expect(s.symbolEl.getAttribute('data-collapsed')).toBeNull();
        expect(s.locsEl.style.display).toBe('');
        expect(s.toggleEl.innerHTML).toBe('\u25BE');
      }
      expect(dom.collapseAllBtn.innerHTML).toBe('\u2212');
    });

    test('button sync after single-toggle ignores L2 state', () => {
      const dom = makeNestedHandlerDom([
        { collapsed: true, l2: [{ collapsed: false }] },
        { collapsed: true, l2: [{ collapsed: false }] },
      ]);
      SidebarLogic._setupCollapseHandlers(dom.root);
      const contentHandler = dom.listeners.get('content')[0];
      // Expand L1 #1 via simulated click
      contentHandler({
        target: {
          closest(sel) {
            return sel === '.sidebar-symbol' ? dom.l1Symbols[0].symbolEl : null;
          },
        },
      });
      // At least one L1 expanded → button −
      expect(dom.collapseAllBtn.innerHTML).toBe('\u2212');
      // Collapse L1 #1 back
      contentHandler({
        target: {
          closest(sel) {
            return sel === '.sidebar-symbol' ? dom.l1Symbols[0].symbolEl : null;
          },
        },
      });
      // All L1 collapsed → button should show + (L2 state irrelevant)
      expect(dom.collapseAllBtn.innerHTML).toBe('+');
    });

    test('no crash when no collapse-all button', () => {
      const content = {
        querySelectorAll() {
          return [];
        },
        addEventListener() {},
      };
      const root = {
        querySelector(sel) {
          if (sel === '.sidebar-content') return content;
          return null; // no collapse-all button
        },
      };
      // Should not throw
      expect(() => SidebarLogic._setupCollapseHandlers(root)).not.toThrow();
    });
  });

  describe('showNode/showTransientNode', () => {
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
      fakeEl = createFakeElement('foreignObject');
      fakeEl.innerHTML = '';
      const innerDiv = createFakeElement('div');
      innerDiv._innerHTML = '';
      Object.defineProperty(innerDiv, 'innerHTML', {
        get() {
          return this._innerHTML;
        },
        set(v) {
          this._innerHTML = v;
        },
      });
      innerDiv.offsetWidth = 0;
      fakeEl._innerDiv = innerDiv;
      fakeEl.querySelector = () => fakeEl._innerDiv;
      const svgMock = makeSvgMock(0);
      globalThis.DomAdapter = {
        getElementById(id) {
          if (id === 'relation-sidebar') return fakeEl;
          return null;
        },
        getSvgRoot() {
          return svgMock;
        },
        querySelector(sel) {
          if (sel === 'svg') return svgMock;
          return null;
        },
        querySelectorAll() {
          return [];
        },
      };
      globalThis.window = globalThis.window || {};
      globalThis.window.innerWidth = 1000;
      globalThis.window.innerHeight = 800;
      SidebarLogic._isTransient = false;
      SidebarLogic._debounceTimer = null;
    });

    test('showNode sets display:block and renders node content', () => {
      const relations = { incoming: [], outgoing: [] };
      SidebarLogic.showNode('crate_a', relations);
      expect(fakeEl.style.display).toBe('block');
      expect(fakeEl._innerDiv.innerHTML).toContain('sidebar-header');
      expect(fakeEl._innerDiv.innerHTML).toContain('No relations');
    });

    test('showNode removes sidebar-transient class', () => {
      fakeEl._innerDiv.classList.add('sidebar-transient');
      const relations = { incoming: [], outgoing: [] };
      SidebarLogic.showNode('crate_a', relations);
      expect(fakeEl._innerDiv.classList.contains('sidebar-transient')).toBe(
        false,
      );
      expect(SidebarLogic._isTransient).toBe(false);
    });

    test('showTransientNode shows after 30ms debounce', async () => {
      const relations = { incoming: [], outgoing: [] };
      SidebarLogic.showTransientNode('crate_a', relations);
      // Before timer fires
      expect(fakeEl.style.display).not.toBe('block');
      // Wait for debounce (30ms + buffer)
      await new Promise((r) => setTimeout(r, 50));
      expect(fakeEl.style.display).toBe('block');
      expect(SidebarLogic._isTransient).toBe(true);
      expect(fakeEl._innerDiv.classList.contains('sidebar-transient')).toBe(
        true,
      );
    });
  });

  describe('buildNodeContent', () => {
    // Helper: relations with 2 incoming + 1 outgoing for crate_a
    function makeRelations() {
      return {
        incoming: [
          {
            targetId: 'mod_render',
            weight: 5,
            arcId: 'mod_render-crate_a',
            usages: [
              {
                symbol: 'Config',
                modulePath: 'config',
                locations: [
                  { file: 'src/render.rs', line: 10 },
                  { file: 'src/render.rs', line: 20 },
                  { file: 'src/render.rs', line: 30 },
                ],
              },
              {
                symbol: 'parse',
                modulePath: null,
                locations: [
                  { file: 'src/render.rs', line: 40 },
                  { file: 'src/render.rs', line: 50 },
                ],
              },
            ],
          },
          {
            targetId: 'mod_cli',
            weight: 3,
            arcId: 'mod_cli-crate_a',
            usages: [
              {
                symbol: 'run',
                modulePath: 'cli',
                locations: [
                  { file: 'src/cli.rs', line: 5 },
                  { file: 'src/cli.rs', line: 15 },
                  { file: 'src/cli.rs', line: 25 },
                ],
              },
            ],
          },
        ],
        outgoing: [
          {
            targetId: 'crate_b',
            weight: 2,
            arcId: 'crate_a-crate_b',
            usages: [
              {
                symbol: 'ModuleInfo',
                modulePath: 'graph',
                locations: [
                  { file: 'src/lib.rs', line: 7 },
                  { file: 'src/lib.rs', line: 12 },
                ],
              },
            ],
          },
        ],
      };
    }

    test('2 incoming + 1 outgoing renders correct HTML structure', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      expect(html).toContain('sidebar-header');
      expect(html).toContain('sidebar-content');
      expect(html).toContain('sidebar-footer');
      // 3 usage-group Level-1 sections (2 incoming + 1 outgoing)
      const level1Matches = html.match(/data-collapsed="true"/g);
      expect(level1Matches).toHaveLength(3);
    });

    test('incoming: selected node is on the right in From→To pair', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      // For incoming: [source] → [selected]
      // render → crate_a (incoming from mod_render)
      const renderPairMatch = html.match(
        /render[\s\S]*?sidebar-arrow[\s\S]*?crate_a/,
      );
      expect(renderPairMatch).not.toBeNull();
    });

    test('outgoing: selected node is on the left in From→To pair', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      // For outgoing: [selected] → [target]
      // crate_a → crate_b
      const outPairMatch = html.match(
        /sidebar-node-selected[\s\S]*?sidebar-arrow[\s\S]*?crate_b/,
      );
      expect(outPairMatch).not.toBeNull();
    });

    test('selected node has sidebar-node-selected class', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      expect(html).toContain('sidebar-node-selected');
      // Should appear on the selected node badge (crate_a is type crate)
      expect(html).toContain('sidebar-node-crate sidebar-node-selected');
    });

    test('header badge has sidebar-node-selected class', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      const headerMatch = html.match(
        /<div class="sidebar-header">[\s\S]*?<\/div>/,
      );
      expect(headerMatch).not.toBeNull();
      expect(headerMatch[0]).toContain(
        'sidebar-node-crate sidebar-node-selected',
      );
    });

    test('incoming sections appear before outgoing', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      const renderIdx = html.indexOf('render');
      const crate_bIdx = html.indexOf('crate_b');
      expect(renderIdx).toBeLessThan(crate_bIdx);
    });

    test('only incoming: no outgoing block, no divider', () => {
      const relations = { incoming: makeRelations().incoming, outgoing: [] };
      const html = SidebarLogic.buildNodeContent('crate_a', relations);
      expect(html).not.toContain('sidebar-divider');
      // Should not contain any outgoing target
      expect(html).not.toContain('crate_b');
    });

    test('only outgoing: no incoming block', () => {
      const relations = { incoming: [], outgoing: makeRelations().outgoing };
      const html = SidebarLogic.buildNodeContent('crate_a', relations);
      expect(html).not.toContain('sidebar-divider');
      expect(html).not.toContain('render');
      expect(html).toContain('crate_b');
    });

    test('no relations: shows placeholder', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', {
        incoming: [],
        outgoing: [],
      });
      expect(html).toContain('No relations');
    });

    test('Level 1 collapsed, Level 2 expanded', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      // Level 1: all data-collapsed="true"
      const collapsedMatches = html.match(/data-collapsed="true"/g);
      expect(collapsedMatches).toHaveLength(3);
      // Level 2 symbols should NOT have data-collapsed
      // Level 2 toggle icons should be ▾ (expanded)
      expect(html).toContain('&#x25BE;');
    });

    test('footer shows correct counts', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      // 3 total relations (2 incoming + 1 outgoing)
      // 2 Dependents (incoming), 1 Dependencies (outgoing)
      expect(html).toContain('3 Relations');
      expect(html).toContain('2 Dependents');
      expect(html).toContain('1 Dependencies');
    });

    test('empty usages shows Cargo.toml dependency', () => {
      const relations = {
        incoming: [
          {
            targetId: 'mod_render',
            weight: 0,
            arcId: 'mod_render-crate_a',
            usages: [],
          },
        ],
        outgoing: [],
      };
      const html = SidebarLogic.buildNodeContent('crate_a', relations);
      expect(html).toContain('Cargo.toml dependency');
    });

    test('Level 2 sorted by location count descending', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', makeRelations());
      // First incoming relation (mod_render, weight 5): Config (3 locs) before parse (2 locs)
      const configIdx = html.indexOf('Config');
      const parseIdx = html.indexOf('parse');
      expect(configIdx).toBeLessThan(parseIdx);
    });
  });

  describe('buildContent — cycle view', () => {
    let savedArcs, savedCycles;

    beforeEach(() => {
      savedArcs = globalThis.STATIC_DATA.arcs;
      savedCycles = globalThis.STATIC_DATA.cycles;

      // Cycle: A → B → C → A (cycleIds=[0])
      globalThis.STATIC_DATA.arcs = {
        ...savedArcs,
        'A-B': {
          from: 'A',
          to: 'B',
          cycleIds: [0],
          usages: [
            {
              symbol: 'sym1',
              modulePath: null,
              locations: [{ file: 'a.rs', line: 1 }],
            },
          ],
        },
        'B-C': {
          from: 'B',
          to: 'C',
          cycleIds: [0],
          usages: [
            {
              symbol: 'sym2',
              modulePath: null,
              locations: [
                { file: 'b.rs', line: 1 },
                { file: 'b.rs', line: 2 },
                { file: 'b.rs', line: 3 },
              ],
            },
          ],
        },
        'C-A': {
          from: 'C',
          to: 'A',
          cycleIds: [0],
          usages: [
            {
              symbol: 'sym3',
              modulePath: null,
              locations: [
                { file: 'c.rs', line: 1 },
                { file: 'c.rs', line: 2 },
              ],
            },
          ],
        },
      };
      globalThis.STATIC_DATA.nodes = {
        ...globalThis.STATIC_DATA.nodes,
        A: { type: 'module', name: 'mod_a', parent: null },
        B: { type: 'module', name: 'mod_b', parent: null },
        C: { type: 'module', name: 'mod_c', parent: null },
      };
      globalThis.STATIC_DATA.cycles = [
        { nodes: ['A', 'B', 'C'], arcs: ['A-B', 'B-C', 'C-A'] },
      ];
    });

    afterEach(() => {
      globalThis.STATIC_DATA.arcs = savedArcs;
      globalThis.STATIC_DATA.cycles = savedCycles;
    });

    test("cycle-arc: header shows 'Cycle' with edge count", () => {
      const html = SidebarLogic.buildContent('A-B');
      expect(html).toContain('Cycle');
      expect(html).toContain('3 edges');
    });

    test('cycle-arc: all cycle arcs listed', () => {
      const html = SidebarLogic.buildContent('A-B');
      expect(html).toContain('mod_a');
      expect(html).toContain('mod_b');
      expect(html).toContain('mod_c');
      // All three arcs should have their source locations
      expect(html).toContain('a.rs');
      expect(html).toContain('b.rs');
      expect(html).toContain('c.rs');
    });

    test('cycle-arc: sorted ascending by source-location count (weakest link first)', () => {
      const html = SidebarLogic.buildContent('A-B');
      // A→B has 1 loc, C→A has 2 locs, B→C has 3 locs
      // Ascending order: A→B (1) first, then C→A (2), then B→C (3)
      const aIdx = html.indexOf('a.rs');
      const cIdx = html.indexOf('c.rs');
      const bIdx = html.indexOf('b.rs');
      expect(aIdx).toBeLessThan(cIdx);
      expect(cIdx).toBeLessThan(bIdx);
    });

    test('cycle-arc: clicked arc highlighted', () => {
      const html = SidebarLogic.buildContent('A-B');
      expect(html).toContain('sidebar-selected-arc');
    });

    test('non-cycle arc: normal single-arc layout (regression)', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).not.toContain('Cycle (');
      expect(html).toContain('sidebar-header');
      expect(html).toContain('crate_a');
      expect(html).toContain('crate_b');
    });

    // Helper to set up multi-cycle test data (arc B-C in cycles 0 and 1)
    function setupMultiCycleData() {
      globalThis.STATIC_DATA.arcs['B-C'] = {
        from: 'B',
        to: 'C',
        cycleIds: [0, 1],
        usages: [
          {
            symbol: 'sym2',
            modulePath: null,
            locations: [{ file: 'b.rs', line: 1 }],
          },
        ],
      };
      globalThis.STATIC_DATA.cycles.push({
        nodes: ['B', 'C', 'D'],
        arcs: ['B-C', 'C-D', 'D-B'],
      });
      globalThis.STATIC_DATA.arcs['C-D'] = {
        from: 'C',
        to: 'D',
        cycleIds: [1],
        usages: [
          {
            symbol: 'sym_cd',
            modulePath: null,
            locations: [{ file: 'c.rs', line: 10 }],
          },
        ],
      };
      globalThis.STATIC_DATA.arcs['D-B'] = {
        from: 'D',
        to: 'B',
        cycleIds: [1],
        usages: [
          {
            symbol: 'sym_db',
            modulePath: null,
            locations: [{ file: 'd.rs', line: 1 }],
          },
        ],
      };
      globalThis.STATIC_DATA.nodes.D = {
        type: 'module',
        name: 'mod_d',
        parent: null,
      };
    }

    test('multi-cycle arc: shows grouped cycles header', () => {
      setupMultiCycleData();
      const html = SidebarLogic.buildContent('B-C');
      // Should show "Cycles (2)" header for multi-cycle
      expect(html).toContain('Cycles (2)');
      // Header has actions wrapper and collapse-all
      expect(html).toContain('sidebar-header-actions');
      expect(html).toContain('sidebar-collapse-all');
      // Old cycle-group-header class is gone
      expect(html).not.toContain('sidebar-cycle-group-header');
      // Should contain arcs from both cycles
      expect(html).toContain('a.rs');
      expect(html).toContain('b.rs');
      expect(html).toContain('c.rs');
      expect(html).toContain('d.rs');
    });

    test('multi-cycle: cycle groups are collapsible L1', () => {
      setupMultiCycleData();
      const html = SidebarLogic.buildContent('B-C');
      // Cycle group headers should contain "Cycle N" text as sidebar-symbol-name
      expect(html).toContain('Cycle 1');
      expect(html).toContain('Cycle 2');
      // All sidebar-symbol divs should be collapsed
      const symbolDivs = html.match(/<div class="sidebar-symbol"/g) || [];
      const collapsedDivs =
        html.match(/<div class="sidebar-symbol" data-collapsed="true"/g) || [];
      expect(symbolDivs.length).toBeGreaterThan(0);
      expect(collapsedDivs.length).toBe(symbolDivs.length);
    });

    test('multi-cycle: arcs within cycles are collapsible L2', () => {
      setupMultiCycleData();
      const html = SidebarLogic.buildContent('B-C');
      // All sidebar-locations should be hidden
      const locsDivs = html.match(/<div class="sidebar-locations"/g) || [];
      const hiddenDivs =
        html.match(/<div class="sidebar-locations" style="display:none"/g) ||
        [];
      expect(locsDivs.length).toBeGreaterThan(0);
      expect(hiddenDivs.length).toBe(locsDivs.length);
      // Toggle icon should be ▸ (collapsed) not ▾ (expanded)
      expect(html).toContain('&#x25B8;');
      expect(html).not.toContain('&#x25BE;');
    });

    test('multi-cycle: collapse-all button shows +', () => {
      setupMultiCycleData();
      const html = SidebarLogic.buildContent('B-C');
      expect(html).toContain('>+</button>');
    });

    test('multi-cycle: cycle group header shows ref count', () => {
      setupMultiCycleData();
      const html = SidebarLogic.buildContent('B-C');
      // Each cycle group header should have a sidebar-ref-count badge
      expect(html).toContain('sidebar-ref-count');
    });

    test("single-cycle arc: shows original 'Cycle' header", () => {
      const html = SidebarLogic.buildContent('A-B');
      expect(html).toContain('Cycle (');
      expect(html).toContain('3 edges');
      expect(html).not.toContain('Cycles (');
    });

    test('single-cycle: header has collapse-all button and header-actions', () => {
      const html = SidebarLogic.buildContent('A-B');
      expect(html).toContain('sidebar-header-actions');
      expect(html).toContain('sidebar-collapse-all');
    });

    test('single-cycle: arcs start collapsed', () => {
      const html = SidebarLogic.buildContent('A-B');
      // All sidebar-symbol divs should have data-collapsed="true"
      const symbolDivs = html.match(/<div class="sidebar-symbol"/g) || [];
      const collapsedDivs =
        html.match(/<div class="sidebar-symbol" data-collapsed="true"/g) || [];
      expect(symbolDivs.length).toBeGreaterThan(0);
      expect(collapsedDivs.length).toBe(symbolDivs.length);
      // All sidebar-locations should have display:none
      const locsDivs = html.match(/<div class="sidebar-locations"/g) || [];
      const hiddenDivs =
        html.match(/<div class="sidebar-locations" style="display:none"/g) ||
        [];
      expect(locsDivs.length).toBeGreaterThan(0);
      expect(hiddenDivs.length).toBe(locsDivs.length);
      // Toggle icon should be ▸ (collapsed) not ▾ (expanded)
      expect(html).toContain('&#x25B8;');
      expect(html).not.toContain('&#x25BE;');
    });

    test('single-cycle: collapse-all button shows +', () => {
      const html = SidebarLogic.buildContent('A-B');
      expect(html).toContain('>+</button>');
    });

    test('single-cycle: arc headers show symbol annotation', () => {
      const html = SidebarLogic.buildContent('A-B');
      // Symbols should appear in sidebar-arc-symbols spans
      expect(html).toContain('sidebar-arc-symbols');
      expect(html).toContain('::sym1');
      expect(html).toContain('::sym2');
      expect(html).toContain('::sym3');
    });

    test('single-cycle: arc without symbols shows no annotation', () => {
      // Add a bare arc to the cycle
      globalThis.STATIC_DATA.arcs['A-B'] = {
        from: 'A',
        to: 'B',
        cycleIds: [0],
        usages: [
          {
            symbol: '',
            modulePath: null,
            locations: [{ file: 'a.rs', line: 1 }],
          },
        ],
      };
      globalThis.STATIC_DATA.cycles[0].arcs = ['A-B', 'B-C', 'C-A'];
      const html = SidebarLogic.buildContent('A-B');
      // B→C still has sym2, so sidebar-arc-symbols should appear there
      expect(html).toContain('sidebar-arc-symbols');
      // But A→B header area should not have ::
      // Count: we should have 2 symbol annotations (B→C with sym2, C→A with sym3), not 3
      const symbolSpans = html.match(/sidebar-arc-symbols/g) || [];
      expect(symbolSpans).toHaveLength(2);
    });

    test('multi-cycle: arc headers show symbol annotation', () => {
      setupMultiCycleData();
      const html = SidebarLogic.buildContent('B-C');
      expect(html).toContain('sidebar-arc-symbols');
      expect(html).toContain('::sym2');
      expect(html).toContain('::sym_cd');
      expect(html).toContain('::sym_db');
    });
  });

  describe('formatArcSymbols', () => {
    test('empty usages returns empty string', () => {
      expect(SidebarLogic.formatArcSymbols([])).toBe('');
    });

    test('single symbol returns ::Symbol', () => {
      const usages = [
        {
          symbol: 'Package',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
      ];
      expect(SidebarLogic.formatArcSymbols(usages)).toBe('::Package');
    });

    test('multiple symbols returns ::{S1, S2}', () => {
      const usages = [
        {
          symbol: 'Alpha',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: 'Beta',
          modulePath: null,
          locations: [{ file: 'b.rs', line: 2 }],
        },
      ];
      expect(SidebarLogic.formatArcSymbols(usages)).toBe('::{Alpha, Beta}');
    });

    test('three symbols returns ::{S1, S2, S3}', () => {
      const usages = [
        {
          symbol: 'A',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: 'B',
          modulePath: null,
          locations: [{ file: 'b.rs', line: 1 }],
        },
        {
          symbol: 'C',
          modulePath: null,
          locations: [{ file: 'c.rs', line: 1 }],
        },
      ];
      expect(SidebarLogic.formatArcSymbols(usages)).toBe('::{A, B, C}');
    });

    test('bare usages (symbol="") excluded', () => {
      const usages = [
        {
          symbol: '',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: 'Foo',
          modulePath: null,
          locations: [{ file: 'b.rs', line: 1 }],
        },
      ];
      expect(SidebarLogic.formatArcSymbols(usages)).toBe('::Foo');
    });

    test('all-bare usages returns empty string', () => {
      const usages = [
        {
          symbol: '',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: '',
          modulePath: null,
          locations: [{ file: 'b.rs', line: 2 }],
        },
      ];
      expect(SidebarLogic.formatArcSymbols(usages)).toBe('');
    });

    test('4+ symbols truncated with ellipsis', () => {
      const usages = [
        {
          symbol: 'A',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: 'B',
          modulePath: null,
          locations: [{ file: 'b.rs', line: 1 }],
        },
        {
          symbol: 'C',
          modulePath: null,
          locations: [{ file: 'c.rs', line: 1 }],
        },
        {
          symbol: 'D',
          modulePath: null,
          locations: [{ file: 'd.rs', line: 1 }],
        },
      ];
      expect(SidebarLogic.formatArcSymbols(usages)).toBe('::{A, B, \u2026}');
    });

    test('deduplicates symbols across groups', () => {
      const usages = [
        {
          symbol: 'Foo',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: 'Foo',
          modulePath: null,
          locations: [{ file: 'b.rs', line: 2 }],
        },
        {
          symbol: 'Bar',
          modulePath: null,
          locations: [{ file: 'c.rs', line: 3 }],
        },
      ];
      expect(SidebarLogic.formatArcSymbols(usages)).toBe('::{Foo, Bar}');
    });
  });

  describe('_formatNodeName', () => {
    test('returns fallback when node is null', () => {
      expect(SidebarLogic._formatNodeName(null, 'fallback-id')).toBe(
        'fallback-id',
      );
    });

    test('returns name without version for regular nodes', () => {
      const node = { name: 'my_crate', type: 'crate' };
      expect(SidebarLogic._formatNodeName(node, 'id')).toBe('my_crate');
    });

    test('appends version for external crates', () => {
      const node = { name: 'serde', type: 'external', version: '1.0.204' };
      expect(SidebarLogic._formatNodeName(node, 'id')).toBe('serde v1.0.204');
    });

    test('no version suffix when version is undefined', () => {
      const node = { name: 'tokio', type: 'external' };
      expect(SidebarLogic._formatNodeName(node, 'id')).toBe('tokio');
    });
  });

  describe('buildContent with external nodes', () => {
    test('shows version in header for external arc', () => {
      globalThis.STATIC_DATA.nodes.ext_serde = {
        type: 'external',
        name: 'serde',
        version: '1.0.204',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 30,
        hasChildren: false,
      };
      globalThis.STATIC_DATA.arcs['crate_a-ext_serde'] = {
        from: 'crate_a',
        to: 'ext_serde',
        usages: [],
      };
      const html = SidebarLogic.buildContent('crate_a-ext_serde');
      expect(html).toContain('serde v1.0.204');
      expect(html).toContain('Cargo.toml dependency');
      delete globalThis.STATIC_DATA.nodes.ext_serde;
      delete globalThis.STATIC_DATA.arcs['crate_a-ext_serde'];
    });

    test('shows version in node sidebar for external crate', () => {
      globalThis.STATIC_DATA.nodes.ext_tokio = {
        type: 'external',
        name: 'tokio',
        version: '1.35.0',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 30,
        hasChildren: false,
      };
      const relations = {
        incoming: [
          {
            targetId: 'crate_a',
            weight: 3,
            arcId: 'crate_a-ext_tokio',
            usages: [
              {
                symbol: 'Runtime',
                modulePath: 'runtime',
                locations: [
                  { file: 'src/main.rs', line: 5 },
                  { file: 'src/main.rs', line: 10 },
                  { file: 'src/main.rs', line: 15 },
                ],
              },
            ],
          },
        ],
        outgoing: [],
      };
      const html = SidebarLogic.buildNodeContent('ext_tokio', relations);
      expect(html).toContain('tokio v1.35.0');
      expect(html).toContain('sidebar-header');
      delete globalThis.STATIC_DATA.nodes.ext_tokio;
    });
  });

  describe('data-node-id attributes on badges', () => {
    test('buildContent adds data-node-id to header from/to badges', () => {
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).toContain('data-node-id="crate_a"');
      expect(html).toContain('data-node-id="crate_b"');
    });

    test('buildNodeContent adds data-node-id to header badge', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', {
        incoming: [],
        outgoing: [],
      });
      expect(html).toContain('data-node-id="crate_a"');
    });

    test('_buildRelationSection adds data-node-id to from/to badges (incoming)', () => {
      const relations = {
        incoming: [
          {
            targetId: 'mod_render',
            weight: 2,
            arcId: 'mod_render-crate_a',
            usages: [],
          },
        ],
        outgoing: [],
      };
      const html = SidebarLogic.buildNodeContent('crate_a', relations);
      // incoming: from=mod_render, to=crate_a
      expect(html).toContain('data-node-id="mod_render"');
      expect(html).toContain('data-node-id="crate_a"');
    });

    test('_buildRelationSection adds data-node-id to from/to badges (outgoing)', () => {
      const relations = {
        incoming: [],
        outgoing: [
          {
            targetId: 'crate_b',
            weight: 2,
            arcId: 'crate_a-crate_b',
            usages: [],
          },
        ],
      };
      const html = SidebarLogic.buildNodeContent('crate_a', relations);
      // outgoing: from=crate_a, to=crate_b
      expect(html).toContain('data-node-id="crate_a"');
      expect(html).toContain('data-node-id="crate_b"');
    });

    test('_buildCycleContent adds data-node-id to arc from/to badges (single cycle)', () => {
      const savedArcs = globalThis.STATIC_DATA.arcs;
      const savedCycles = globalThis.STATIC_DATA.cycles;
      const savedNodes = globalThis.STATIC_DATA.nodes;

      globalThis.STATIC_DATA.arcs = {
        'A-B': {
          from: 'A',
          to: 'B',
          cycleIds: [0],
          usages: [
            {
              symbol: 'sym1',
              modulePath: null,
              locations: [{ file: 'a.rs', line: 1 }],
            },
          ],
        },
        'B-A': {
          from: 'B',
          to: 'A',
          cycleIds: [0],
          usages: [],
        },
      };
      globalThis.STATIC_DATA.cycles = [{ arcs: ['A-B', 'B-A'] }];
      globalThis.STATIC_DATA.nodes = {
        ...savedNodes,
        A: { type: 'module', name: 'mod_a', parent: null },
        B: { type: 'module', name: 'mod_b', parent: null },
      };

      const html = SidebarLogic.buildContent('A-B');
      expect(html).toContain('data-node-id="A"');
      expect(html).toContain('data-node-id="B"');

      globalThis.STATIC_DATA.arcs = savedArcs;
      globalThis.STATIC_DATA.cycles = savedCycles;
      globalThis.STATIC_DATA.nodes = savedNodes;
    });

    test('_buildCycleContent adds data-node-id to arc from/to badges (multi cycle)', () => {
      const savedArcs = globalThis.STATIC_DATA.arcs;
      const savedCycles = globalThis.STATIC_DATA.cycles;
      const savedNodes = globalThis.STATIC_DATA.nodes;

      globalThis.STATIC_DATA.arcs = {
        'X-Y': {
          from: 'X',
          to: 'Y',
          cycleIds: [0, 1],
          usages: [],
        },
        'Y-X': {
          from: 'Y',
          to: 'X',
          cycleIds: [0],
          usages: [],
        },
        'Y-Z': {
          from: 'Y',
          to: 'Z',
          cycleIds: [1],
          usages: [],
        },
        'Z-X': {
          from: 'Z',
          to: 'X',
          cycleIds: [1],
          usages: [],
        },
      };
      globalThis.STATIC_DATA.cycles = [
        { arcs: ['X-Y', 'Y-X'] },
        { arcs: ['X-Y', 'Y-Z', 'Z-X'] },
      ];
      globalThis.STATIC_DATA.nodes = {
        ...savedNodes,
        X: { type: 'module', name: 'mod_x', parent: null },
        Y: { type: 'module', name: 'mod_y', parent: null },
        Z: { type: 'module', name: 'mod_z', parent: null },
      };

      const html = SidebarLogic.buildContent('X-Y');
      expect(html).toContain('data-node-id="X"');
      expect(html).toContain('data-node-id="Y"');
      expect(html).toContain('data-node-id="Z"');

      globalThis.STATIC_DATA.arcs = savedArcs;
      globalThis.STATIC_DATA.cycles = savedCycles;
      globalThis.STATIC_DATA.nodes = savedNodes;
    });
  });

  describe('badge click handler', () => {
    function makeBadgeMock(nodeId) {
      const listeners = new Map();
      return {
        dataset: { nodeId },
        addEventListener(evt, fn) {
          if (!listeners.has(evt)) listeners.set(evt, []);
          listeners.get(evt).push(fn);
        },
        _fire(evt, event) {
          for (const fn of listeners.get(evt) || []) fn(event);
        },
      };
    }

    function makeBadgeHandlerDom(badges) {
      const contentListeners = new Map();
      const content = {
        querySelectorAll(sel) {
          if (sel === ':scope > .sidebar-usage-group > .sidebar-symbol')
            return [];
          if (sel === '.sidebar-symbol') return [];
          return [];
        },
        addEventListener(evt, fn) {
          if (!contentListeners.has(evt)) contentListeners.set(evt, []);
          contentListeners.get(evt).push(fn);
        },
      };
      const root = {
        querySelector(sel) {
          if (sel === '.sidebar-content') return content;
          if (sel === '.sidebar-collapse-all') return null;
          return null;
        },
        querySelectorAll(sel) {
          if (sel === '[data-node-id]') return badges;
          return [];
        },
      };
      return { root, content, contentListeners };
    }

    test('_onBadgeClick is null by default', () => {
      expect(SidebarLogic._onBadgeClick).toBeNull();
    });

    test('badge click calls _onBadgeClick with node ID', () => {
      const calls = [];
      SidebarLogic._onBadgeClick = (id) => calls.push(id);

      const badge = makeBadgeMock('test_node');
      const dom = makeBadgeHandlerDom([badge]);
      SidebarLogic._setupCollapseHandlers(dom.root);

      badge._fire('click', { stopPropagation() {} });
      expect(calls).toEqual(['test_node']);

      SidebarLogic._onBadgeClick = null;
    });

    test('badge click calls stopPropagation', () => {
      SidebarLogic._onBadgeClick = () => {};

      const badge = makeBadgeMock('test_node');
      const dom = makeBadgeHandlerDom([badge]);
      SidebarLogic._setupCollapseHandlers(dom.root);

      let stopped = false;
      badge._fire('click', {
        stopPropagation() {
          stopped = true;
        },
      });
      expect(stopped).toBe(true);

      SidebarLogic._onBadgeClick = null;
    });

    test('badge click does nothing when _onBadgeClick is null', () => {
      SidebarLogic._onBadgeClick = null;

      const badge = makeBadgeMock('test_node');
      const dom = makeBadgeHandlerDom([badge]);
      SidebarLogic._setupCollapseHandlers(dom.root);

      // Should not throw
      expect(() => {
        badge._fire('click', { stopPropagation() {} });
      }).not.toThrow();
    });
  });
});
