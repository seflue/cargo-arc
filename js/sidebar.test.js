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
  clusters: {},
};

// Mock StaticData module (sidebar.js uses StaticData.getNode for name resolution)
globalThis.StaticData = {
  getNode(id) {
    return globalThis.STATIC_DATA.nodes?.[id] || null;
  },
  hasChildren(nodeId) {
    return globalThis.STATIC_DATA.nodes?.[nodeId]?.hasChildren ?? false;
  },
  qualifiedParts(nodeId) {
    const nodes = globalThis.STATIC_DATA.nodes;
    const node = nodes?.[nodeId];
    if (!node) return { crate: null, path: nodeId };
    const segments = [node.name];
    let crate = null;
    let parentId = node.parent;
    while (parentId != null) {
      const parent = nodes?.[parentId];
      if (!parent) break;
      if (parent.type === 'crate') {
        crate = parent.name;
        break;
      }
      segments.push(parent.name);
      parentId = parent.parent;
    }
    segments.reverse();
    return { crate, path: segments.join('::') };
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

  describe('buildContent — consumer-scope tags', () => {
    afterEach(() => {
      delete globalThis.STATIC_DATA.symbolScopes;
    });

    const overrideWith = (usages) => ({ from: 'x', to: 'y', usages });

    test('single-consumer scope names the sole consumer as a fact', () => {
      globalThis.STATIC_DATA.symbolScopes = {
        y: { Foo: { scope: 'singleConsumer', module: 'x', consumers: ['x'] } },
      };
      const html = SidebarLogic.buildContent(
        'any',
        overrideWith([
          {
            symbol: 'Foo',
            modulePath: null,
            locations: [{ file: 'a.rs', line: 1 }],
          },
        ]),
      );
      expect(html).toContain('sidebar-scope-singleConsumer');
      expect(html).toContain('only used by x');
    });

    test('common-ancestor states the home, crate-wide states the breadth', () => {
      globalThis.STATIC_DATA.symbolScopes = {
        y: {
          Anc: { scope: 'commonAncestor', module: 'x', consumers: ['x', 'z'] },
          Wide: { scope: 'crateWide', consumers: ['x', 'z'] },
        },
      };
      const html = SidebarLogic.buildContent(
        'any',
        overrideWith([
          {
            symbol: 'Anc',
            modulePath: null,
            locations: [{ file: 'a.rs', line: 1 }],
          },
          {
            symbol: 'Wide',
            modulePath: null,
            locations: [{ file: 'a.rs', line: 2 }],
          },
        ]),
      );
      expect(html).toContain('sidebar-scope-commonAncestor');
      expect(html).toContain('used under x');
      expect(html).toContain('sidebar-scope-crateWide');
      expect(html).toContain('widely used (2 modules)');
    });

    test('renders no tag for a symbol without a scope', () => {
      globalThis.STATIC_DATA.symbolScopes = { y: {} };
      const html = SidebarLogic.buildContent(
        'any',
        overrideWith([
          {
            symbol: 'Bare',
            modulePath: null,
            locations: [{ file: 'a.rs', line: 1 }],
          },
        ]),
      );
      expect(html).not.toContain('sidebar-scope');
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

    test('generated HTML uses XML-safe attributes', () => {
      // SVG is XML — attributes inside foreignObject must have explicit values.
      // Boolean HTML attributes like data-foo (without ="...") cause XML parsing errors.
      const html = SidebarLogic.buildContent('crate_a-crate_b');
      const valueless = [...html.matchAll(/\sdata-[\w-]+/g)]
        .filter((m) => html[m.index + m[0].length] !== '=')
        .map((m) => m[0].trim());
      expect(valueless).toEqual([]);
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
      attrs.set('data-collapsible', '');
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
          hasAttribute(name) {
            return attrs.has(name);
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
          if (sel === ':scope .sidebar-symbol[data-collapsible]')
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
          if (sel === ':scope .sidebar-symbol[data-collapsible]') return l1Els;
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

    // External deps render as static symbols (no data-collapsible, no .sidebar-locations sibling)
    function makeStaticSymbolEl() {
      // External dep: .sidebar-symbol without data-collapsible
      const attrs = new Map();
      return {
        symbolEl: {
          getAttribute(name) {
            return attrs.get(name) ?? null;
          },
          hasAttribute(name) {
            return attrs.has(name);
          },
          setAttribute(name, value) {
            attrs.set(name, value);
          },
          removeAttribute(name) {
            attrs.delete(name);
          },
          classList: {
            contains(c) {
              return c === 'sidebar-symbol';
            },
          },
          querySelector() {
            return null;
          },
          nextElementSibling: null, // no .sidebar-locations sibling
        },
      };
    }

    function makeHandlerDomWithExtDeps(collapsibleDefs, extDepCount) {
      const collapsible = collapsibleDefs.map((d) => makeSymbolEl(d.collapsed));
      const extDeps = Array.from({ length: extDepCount }, () =>
        makeStaticSymbolEl(),
      );
      // L1 symbols as querySelectorAll returns them: collapsible + external
      const l1Els = [
        ...collapsible.map((s) => s.symbolEl),
        ...extDeps.map((s) => s.symbolEl),
      ];
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
      const collapsibleEls = collapsible.map((s) => s.symbolEl);
      const content = {
        querySelectorAll(sel) {
          if (sel === ':scope > .sidebar-usage-group > .sidebar-symbol')
            return l1Els;
          if (sel === ':scope .sidebar-symbol[data-collapsible]')
            return collapsibleEls;
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
        querySelectorAll() {
          return [];
        },
      };
      return { root, collapsible, extDeps, collapseAllBtn, listeners };
    }

    test('collapse-all toggle works with external deps present', () => {
      // Two collapsible (collapsed) + one external dep (no data-collapsed, no sibling)
      const dom = makeHandlerDomWithExtDeps(
        [{ collapsed: true }, { collapsed: true }],
        1,
      );
      SidebarLogic._setupCollapseHandlers(dom.root);
      const handlers = dom.listeners.get('collapseAll');

      // First click: all collapsible are collapsed → should expand
      for (const fn of handlers) fn();
      for (const s of dom.collapsible) {
        expect(s.symbolEl.getAttribute('data-collapsed')).toBeNull();
        expect(s.locsEl.style.display).toBe('');
      }
      expect(dom.collapseAllBtn.innerHTML).toBe('\u2212');

      // Second click: all collapsible are expanded → should collapse
      for (const fn of handlers) fn();
      for (const s of dom.collapsible) {
        expect(s.symbolEl.getAttribute('data-collapsed')).toBe('true');
        expect(s.locsEl.style.display).toBe('none');
      }
      expect(dom.collapseAllBtn.innerHTML).toBe('+');
    });

    test('per-item toggle button sync ignores external deps', () => {
      const dom = makeHandlerDomWithExtDeps(
        [{ collapsed: true }, { collapsed: true }],
        1,
      );
      SidebarLogic._setupCollapseHandlers(dom.root);
      const contentHandler = dom.listeners.get('content')[0];

      // Expand first collapsible item
      contentHandler({
        target: {
          closest(sel) {
            return sel === '.sidebar-symbol'
              ? dom.collapsible[0].symbolEl
              : null;
          },
        },
      });
      // One expanded, one collapsed → button shows −
      expect(dom.collapseAllBtn.innerHTML).toBe('\u2212');

      // Expand second collapsible item
      contentHandler({
        target: {
          closest(sel) {
            return sel === '.sidebar-symbol'
              ? dom.collapsible[1].symbolEl
              : null;
          },
        },
      });
      // Both expanded → still −
      expect(dom.collapseAllBtn.innerHTML).toBe('\u2212');

      // Collapse both back
      contentHandler({
        target: {
          closest(sel) {
            return sel === '.sidebar-symbol'
              ? dom.collapsible[0].symbolEl
              : null;
          },
        },
      });
      contentHandler({
        target: {
          closest(sel) {
            return sel === '.sidebar-symbol'
              ? dom.collapsible[1].symbolEl
              : null;
          },
        },
      });
      // All collapsible collapsed → button shows + (external dep must not poison this)
      expect(dom.collapseAllBtn.innerHTML).toBe('+');
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

    test('only external deps (no usages): no collapse-all button', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', {
        incoming: [],
        outgoing: [
          {
            targetId: 'ext_serde',
            weight: 0,
            arcId: 'crate_a-ext_serde',
            usages: [],
          },
          {
            targetId: 'ext_tokio',
            weight: 0,
            arcId: 'crate_a-ext_tokio',
            usages: [],
          },
        ],
      });
      expect(html).not.toContain('sidebar-collapse-all');
      expect(html).toContain('sidebar-close');
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

  describe('buildContent — cluster view', () => {
    let savedClusters;
    beforeEach(() => {
      savedClusters = globalThis.STATIC_DATA.clusters;
      globalThis.STATIC_DATA.clusters = {
        0: {
          crate: 'my_crate',
          moduleCount: 4,
          cycleCount: 3,
          cycles: [
            [
              { fromId: 'x', toId: 'y', refs: 2 },
              { fromId: 'y', toId: 'x', refs: 5 },
            ],
          ],
        },
      };
      globalThis.STATIC_DATA.arcs['x-y'] = {
        from: 'x',
        to: 'y',
        usages: [],
        sccId: 0,
      };
      SidebarLogic._isClusterMode = () => true;
    });
    afterEach(() => {
      globalThis.STATIC_DATA.clusters = savedClusters;
      delete globalThis.STATIC_DATA.arcs['x-y'];
      delete globalThis.STATIC_DATA.symbolScopes;
      SidebarLogic._isClusterMode = null;
    });

    test('cluster-mode on: header names crate and counts', () => {
      const html = SidebarLogic.buildContent('x-y');
      expect(html).toContain('my_crate');
      expect(html).toContain('4 modules');
      expect(html).toContain('3 cycles');
    });

    test('cluster-mode on: rows appear in cycle path order', () => {
      const html = SidebarLogic.buildContent('x-y');
      const iX = html.indexOf('data-arc-id="x-y"');
      const iY = html.indexOf('data-arc-id="y-x"');
      expect(iX).toBeGreaterThanOrEqual(0);
      expect(iY).toBeGreaterThan(iX); // path order: x-y then the closing y-x
    });

    test('cluster-mode off: normal usage detail (cycles-off fix)', () => {
      SidebarLogic._isClusterMode = () => false;
      const html = SidebarLogic.buildContent('x-y');
      expect(html).not.toContain('data-arc-id="y-x"');
      expect(html).toContain('sidebar-header');
    });

    test('missing clusters[sccId]: falls back to usage detail', () => {
      globalThis.STATIC_DATA.clusters = {};
      const html = SidebarLogic.buildContent('x-y');
      expect(html).not.toContain('data-arc-id="y-x"');
      expect(html).toContain('sidebar-header');
    });

    test('edge endpoints carry dependent/dependency color classes', () => {
      // from = dependent (purple frame), to = dependency (green frame)
      const html = SidebarLogic.buildContent('x-y');
      expect(html).toContain('sidebar-node-from');
      expect(html).toContain('sidebar-node-to');
    });

    test('edge row expands to crossing symbols with scope tags', () => {
      globalThis.STATIC_DATA.arcs['x-y'].usages = [
        {
          symbol: 'Foo',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
      ];
      globalThis.STATIC_DATA.symbolScopes = {
        y: { Foo: { scope: 'singleConsumer', module: 'x', consumers: ['x'] } },
      };
      const html = SidebarLogic.buildContent('x-y');
      expect(html).toContain('data-collapsible');
      expect(html).toContain('sidebar-edge-symbol');
      expect(html).toContain('<span class="sidebar-symbol-name">Foo</span>');
      expect(html).toContain('sidebar-scope-singleConsumer');
      expect(html).toContain('only used by x');
    });

    test('edge row drops re-export-only symbols, counts coupling only', () => {
      globalThis.STATIC_DATA.arcs['x-y'].usages = [
        {
          symbol: 'Coupled',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
        {
          symbol: 'Reexported',
          modulePath: null,
          viaReexport: true,
          locations: [{ file: 'b.rs', line: 2 }],
        },
      ];
      const html = SidebarLogic.buildContent('x-y');
      expect(html).toContain(
        '<span class="sidebar-symbol-name">Coupled</span>',
      );
      expect(html).not.toContain('Reexported');
      // meta counts the filtered coupling symbols, not edge.refs
      expect(html).toContain('1 symbols');
    });

    test('edge row of only re-exports is not collapsible', () => {
      globalThis.STATIC_DATA.arcs['x-y'].usages = [
        {
          symbol: 'Reexported',
          modulePath: null,
          viaReexport: true,
          locations: [{ file: 'b.rs', line: 2 }],
        },
      ];
      const html = SidebarLogic.buildContent('x-y');
      const xyIdx = html.indexOf('data-arc-id="x-y"');
      const xyRow = html.slice(xyIdx, xyIdx + 600);
      expect(xyRow).not.toContain('data-collapsible');
      expect(xyRow).toContain('0 symbols');
    });

    test('edge row without crossing symbols is not collapsible', () => {
      // arcs['y-x'] is absent -> no usages -> plain, non-expandable row.
      const html = SidebarLogic.buildContent('x-y');
      const yxIdx = html.indexOf('data-arc-id="y-x"');
      const yxRow = html.slice(yxIdx, yxIdx + 200);
      expect(yxRow).not.toContain('data-collapsible');
    });

    test('no expand-all button when no row is collapsible', () => {
      const html = SidebarLogic.buildContent('x-y');
      expect(html).not.toContain('sidebar-collapse-all');
      expect(html).not.toContain('sidebar-header-actions');
    });

    test('expand-all button appears when at least one row is collapsible', () => {
      globalThis.STATIC_DATA.arcs['x-y'].usages = [
        {
          symbol: 'Foo',
          modulePath: null,
          locations: [{ file: 'a.rs', line: 1 }],
        },
      ];
      const html = SidebarLogic.buildContent('x-y');
      expect(html).toContain('sidebar-collapse-all');
      expect(html).toContain('sidebar-header-actions');
    });

    test('cluster view focuses the resolved edge, not the triggering arc', () => {
      // No resolved focus -> cluster overview, no row focused.
      const overview = SidebarLogic.buildContent('x-y');
      expect(overview).not.toContain('sidebar-edge-row-focus');

      // Resolved focus on y-x -> that row is focused, regardless of which
      // arc opened the cluster view.
      SidebarLogic._resolvedFocusArc = () => 'y-x';
      const html = SidebarLogic.buildContent('x-y');
      const yxIdx = html.indexOf('data-arc-id="y-x"');
      const yxRow = html.slice(Math.max(0, yxIdx - 60), yxIdx + 60);
      expect(yxRow).toContain('sidebar-edge-row-focus');
      const xyIdx = html.indexOf('data-arc-id="x-y"');
      const xyRow = html.slice(Math.max(0, xyIdx - 60), xyIdx + 60);
      expect(xyRow).not.toContain('sidebar-edge-row-focus');
      SidebarLogic._resolvedFocusArc = null;
    });
  });

  describe('buildContent — cluster view — cycle blocks', () => {
    let savedClusters;
    let savedNodes;

    // Mirrors the design's own ordering example: three cycles sharing a
    // start node and a first edge (a-b), lengths 3, 3, 4.
    const cycleAbc = [
      { fromId: 'a', toId: 'b', refs: 1 },
      { fromId: 'b', toId: 'c', refs: 1 },
      { fromId: 'c', toId: 'a', refs: 1 },
    ];
    const cycleAbd = [
      { fromId: 'a', toId: 'b', refs: 1 },
      { fromId: 'b', toId: 'd', refs: 1 },
      { fromId: 'd', toId: 'a', refs: 1 },
    ];
    const cycleAbcd = [
      { fromId: 'a', toId: 'b', refs: 1 },
      { fromId: 'b', toId: 'c', refs: 1 },
      { fromId: 'c', toId: 'd', refs: 1 },
      { fromId: 'd', toId: 'a', refs: 1 },
    ];

    beforeEach(() => {
      savedClusters = globalThis.STATIC_DATA.clusters;
      savedNodes = globalThis.STATIC_DATA.nodes;
      globalThis.STATIC_DATA.nodes = { ...savedNodes };
      for (const name of ['a', 'b', 'c', 'd']) {
        globalThis.STATIC_DATA.nodes[name] = {
          type: 'module',
          name,
          parent: 'crate_a',
          x: 0,
          y: 0,
          width: 100,
          height: 30,
          hasChildren: false,
        };
      }
      globalThis.STATIC_DATA.clusters = {
        0: {
          crate: 'my_crate',
          moduleCount: 4,
          cycleCount: 3,
          cycles: [cycleAbc, cycleAbd, cycleAbcd],
        },
      };
      SidebarLogic._isClusterMode = () => true;
    });

    afterEach(() => {
      globalThis.STATIC_DATA.clusters = savedClusters;
      globalThis.STATIC_DATA.nodes = savedNodes;
      SidebarLogic._isClusterMode = null;
    });

    // Splits rendered cluster HTML into one chunk per cycle block. Each
    // `<details>` block never nests another, so slicing on the closing tag
    // isolates block i's markup (header + rows) from the others.
    function blockChunks(html) {
      return html.split('</details>').slice(0, -1);
    }

    function rowClassLists(chunk) {
      return [
        ...chunk.matchAll(/<div class="([^"]*sidebar-edge-row[^"]*)"/g),
      ].map((m) => m[1]);
    }

    test('one block per cycle, block i has cycles[i].length rows', () => {
      const html = SidebarLogic._buildClusterContent('0');
      const chunks = blockChunks(html);
      expect(chunks.length).toBe(3);
      expect(rowClassLists(chunks[0]).length).toBe(cycleAbc.length);
      expect(rowClassLists(chunks[1]).length).toBe(cycleAbd.length);
      expect(rowClassLists(chunks[2]).length).toBe(cycleAbcd.length);
      expect(html.match(/<details class="cycle-block">/g).length).toBe(3);
    });

    test('closing edge: only the last row of each block is marked', () => {
      const html = SidebarLogic._buildClusterContent('0');
      const chunks = blockChunks(html);
      for (const chunk of chunks) {
        const rows = rowClassLists(chunk);
        rows.forEach((classes, i) => {
          if (i === rows.length - 1) {
            expect(classes).toContain('edge-closing');
          } else {
            expect(classes).not.toContain('edge-closing');
          }
        });
      }
    });

    test('repeat: a shared arc dims from its second occurrence, first stays full', () => {
      const html = SidebarLogic._buildClusterContent('0');
      const chunks = blockChunks(html);
      // a-b is the first (non-closing) edge of all three cycles.
      const abClasses = chunks.map(
        (chunk) => rowClassLists(chunk)[0], // a-b is always row 0 here
      );
      expect(abClasses[0]).not.toContain('edge-repeat'); // first occurrence
      expect(abClasses[1]).toContain('edge-repeat'); // repeated
      expect(abClasses[2]).toContain('edge-repeat'); // repeated
    });

    test('repeat exception: a closing edge stays full even if seen before', () => {
      // Block 0: a→b→a (closing b-a). Block 1: b→c→a→b (closing a-b, which
      // reuses block 0's non-closing a-b arc id).
      globalThis.STATIC_DATA.clusters[0].cycles = [
        [
          { fromId: 'a', toId: 'b', refs: 1 },
          { fromId: 'b', toId: 'a', refs: 1 },
        ],
        [
          { fromId: 'b', toId: 'c', refs: 1 },
          { fromId: 'c', toId: 'a', refs: 1 },
          { fromId: 'a', toId: 'b', refs: 1 },
        ],
      ];
      const html = SidebarLogic._buildClusterContent('0');
      const chunks = blockChunks(html);
      const block1Rows = rowClassLists(chunks[1]);
      const closingRow = block1Rows[block1Rows.length - 1];
      expect(closingRow).toContain('edge-closing');
      expect(closingRow).not.toContain('edge-repeat');
    });

    test('closing edge renders a leading loop-closer marker, plain rows do not', () => {
      const closing = SidebarLogic._buildEdgeRow(
        { fromId: 'a', toId: 'b', refs: 1 },
        [],
        undefined,
        ['edge-closing'],
      );
      expect(closing).toContain('sidebar-edge-closing-marker');
      expect(closing).toContain('&#x21ba;');
      const plain = SidebarLogic._buildEdgeRow(
        { fromId: 'a', toId: 'b', refs: 1 },
        [],
        undefined,
        [],
      );
      expect(plain).not.toContain('sidebar-edge-closing-marker');
    });

    test('rows reuse _buildEdgeRow: symbol expand and data-arc-id present', () => {
      globalThis.STATIC_DATA.arcs['a-b'] = {
        from: 'a',
        to: 'b',
        usages: [
          {
            symbol: 'Foo',
            modulePath: null,
            locations: [{ file: 'a.rs', line: 1 }],
          },
        ],
        sccId: 0,
      };
      const html = SidebarLogic._buildClusterContent('0');
      expect(html).toContain('data-arc-id="a-b"');
      expect(html).toContain('data-collapsible');
      expect(html).toContain('<span class="sidebar-symbol-name">Foo</span>');
      delete globalThis.STATIC_DATA.arcs['a-b'];
    });

    test('header: summary carries ordinal, leaf path and module count', () => {
      const html = SidebarLogic._buildClusterContent('0');
      const chunks = blockChunks(html);
      const firstSummary = chunks[0].match(
        /<summary class="block-head">([\s\S]*?)<\/summary>/,
      )[1];
      expect(firstSummary).toContain('>1<'); // ordinal
      expect(firstSummary).toContain('a → b → c → a'); // leaf path, closes the loop
      expect(firstSummary).toContain('3 Module'); // node count
    });

    test('header of a long cycle (k > 4) elides the middle', () => {
      globalThis.STATIC_DATA.nodes.e = {
        type: 'module',
        name: 'e',
        parent: 'crate_a',
        x: 0,
        y: 0,
        width: 100,
        height: 30,
        hasChildren: false,
      };
      globalThis.STATIC_DATA.clusters[0].cycles = [
        [
          { fromId: 'a', toId: 'b', refs: 1 },
          { fromId: 'b', toId: 'c', refs: 1 },
          { fromId: 'c', toId: 'd', refs: 1 },
          { fromId: 'd', toId: 'e', refs: 1 },
          { fromId: 'e', toId: 'a', refs: 1 },
        ],
      ];
      const html = SidebarLogic._buildClusterContent('0');
      const chunks = blockChunks(html);
      const summary = chunks[0].match(
        /<summary class="block-head">([\s\S]*?)<\/summary>/,
      )[1];
      expect(summary).toContain('>1<'); // ordinal
      expect(summary).toContain('…'); // ellipsis
      expect(summary).toContain('a'); // head
      expect(summary).toContain('e → a'); // tail + closing edge
      expect(summary).toContain('5 Module');
    });
  });

  describe('shortLabel', () => {
    let savedNodes;

    beforeEach(() => {
      savedNodes = globalThis.STATIC_DATA.nodes;
      globalThis.STATIC_DATA.nodes = { ...savedNodes };
    });

    afterEach(() => {
      globalThis.STATIC_DATA.nodes = savedNodes;
    });

    function addNode(id, name, parent) {
      globalThis.STATIC_DATA.nodes[id] = {
        type: 'module',
        name,
        parent,
        x: 0,
        y: 0,
        width: 100,
        height: 30,
        hasChildren: false,
      };
    }

    test('unique leaf name resolves to the leaf', () => {
      addNode('token', 'token', 'crate_a');
      const html = SidebarLogic.shortLabel('token', ['token']);
      expect(html).toBe('token');
    });

    test('leaf collision grows to the shortest unique suffix, no crate prefix', () => {
      addNode('mod_a', 'a', 'crate_a');
      addNode('mod_a_util', 'util', 'mod_a');
      addNode('mod_b', 'b', 'crate_a');
      addNode('mod_b_util', 'util', 'mod_b');
      const sccNodeIds = ['mod_a_util', 'mod_b_util'];
      expect(SidebarLogic.shortLabel('mod_a_util', sccNodeIds)).toBe('a::util');
      expect(SidebarLogic.shortLabel('mod_b_util', sccNodeIds)).toBe('b::util');
    });
  });

  describe('elideCyclePath', () => {
    test('length <= n shows the full closed path', () => {
      const label = SidebarLogic.elideCyclePath(['a', 'b', 'c'], 4);
      expect(label).toBe('a → b → c → a');
    });

    test('length > n keeps head, second node and closing edge, elides rest', () => {
      const label = SidebarLogic.elideCyclePath(['a', 'b', 'c', 'd', 'e'], 4);
      expect(label).toBe('a → b → … → e → a');
      expect(label).not.toContain('c');
      expect(label).not.toContain('d');
    });

    test('over the char budget drops the second node too', () => {
      const label = SidebarLogic.elideCyclePath(
        ['a', 'b', 'c', 'd', 'e'],
        4,
        5,
      );
      expect(label).toBe('a → … → e → a');
      expect(label).not.toContain('b');
    });
  });

  describe('cluster row hover wiring', () => {
    function makeRow(arcId) {
      const listeners = {};
      return {
        dataset: { arcId },
        addEventListener(evt, fn) {
          listeners[evt] = fn;
        },
        _fire(evt) {
          listeners[evt]();
        },
      };
    }

    test('row mouseenter calls _onEdgeHover with the row arc id', () => {
      const row = makeRow('x-y');
      const content = {
        addEventListener() {},
        querySelectorAll(sel) {
          if (sel === '.sidebar-edge-row') return [row];
          return [];
        },
      };
      const root = {
        querySelector(sel) {
          if (sel === '.sidebar-content') return content;
          return null;
        },
        querySelectorAll: content.querySelectorAll,
      };
      let hovered;
      SidebarLogic._onEdgeHover = (id) => {
        hovered = id;
      };
      SidebarLogic._onEdgeHoverEnd = null;
      SidebarLogic._setupCollapseHandlers(root);
      row._fire('mouseenter');
      expect(hovered).toBe('x-y');
      SidebarLogic._onEdgeHover = null;
    });

    test('row mouseleave calls _onEdgeHoverEnd', () => {
      const row = makeRow('x-y');
      const content = {
        addEventListener() {},
        querySelectorAll(sel) {
          if (sel === '.sidebar-edge-row') return [row];
          return [];
        },
      };
      const root = {
        querySelector(sel) {
          if (sel === '.sidebar-content') return content;
          return null;
        },
        querySelectorAll: content.querySelectorAll,
      };
      let ended = false;
      SidebarLogic._onEdgeHoverEnd = () => {
        ended = true;
      };
      SidebarLogic._setupCollapseHandlers(root);
      row._fire('mouseleave');
      expect(ended).toBe(true);
      SidebarLogic._onEdgeHoverEnd = null;
    });
  });

  describe('cluster row click wiring', () => {
    function makeHead() {
      const attrs = { 'data-collapsible': '', 'data-collapsed': 'true' };
      const toggle = { innerHTML: '▸' };
      return {
        nextElementSibling: { style: { display: 'none' } },
        hasAttribute: (a) => a in attrs,
        getAttribute: (a) => attrs[a] ?? null,
        setAttribute: (a, v) => {
          attrs[a] = v;
        },
        removeAttribute: (a) => {
          delete attrs[a];
        },
        querySelector: (sel) => (sel === '.sidebar-toggle' ? toggle : null),
        _attrs: attrs,
        _toggle: toggle,
      };
    }

    function harness(row) {
      let clickHandler;
      const content = {
        addEventListener: (evt, fn) => {
          if (evt === 'click') clickHandler = fn;
        },
        querySelectorAll: (sel) => (sel === '.sidebar-edge-row' ? [row] : []),
      };
      const root = {
        querySelector: (sel) => (sel === '.sidebar-content' ? content : null),
        querySelectorAll: () => [],
      };
      const origUP = SidebarLogic.updatePosition;
      SidebarLogic.updatePosition = () => {};
      SidebarLogic._setupCollapseHandlers(root);
      return {
        fireClick: () =>
          clickHandler({
            target: {
              closest: (sel) => (sel === '.sidebar-edge-row' ? row : null),
            },
          }),
        restore: () => {
          SidebarLogic.updatePosition = origUP;
        },
      };
    }

    test('row click routes to _onEdgeClick and applies the returned expand state', () => {
      const head = makeHead();
      const row = {
        dataset: { arcId: 'x-y' },
        classList: { toggle() {} },
        querySelector: (sel) => (sel === '.sidebar-edge-head' ? head : null),
        addEventListener() {},
      };
      let calledWith = null;
      SidebarLogic._onEdgeClick = (arcId, expandable, expanded) => {
        calledWith = { arcId, expandable, expanded };
        return true; // pinned -> ends expanded
      };
      SidebarLogic._resolvedFocusArc = () => 'x-y';
      const h = harness(row);
      h.fireClick();
      expect(calledWith).toEqual({
        arcId: 'x-y',
        expandable: true,
        expanded: false,
      });
      expect(head.hasAttribute('data-collapsed')).toBe(false); // expanded in place
      expect(head._toggle.innerHTML).toBe('▾');
      h.restore();
      SidebarLogic._onEdgeClick = null;
      SidebarLogic._resolvedFocusArc = null;
    });

    test('row click marks only the resolved focus row', () => {
      const head = makeHead();
      let focusedOn = null;
      const row = {
        dataset: { arcId: 'x-y' },
        classList: {
          toggle(_cls, on) {
            focusedOn = on;
          },
        },
        querySelector: (sel) => (sel === '.sidebar-edge-head' ? head : null),
        addEventListener() {},
      };
      SidebarLogic._onEdgeClick = () => false; // unpinned -> collapses
      SidebarLogic._resolvedFocusArc = () => null; // overview, no focus
      const h = harness(row);
      h.fireClick();
      expect(focusedOn).toBe(false); // row not focused when nothing resolved
      expect(head.hasAttribute('data-collapsed')).toBe(true); // collapsed
      h.restore();
      SidebarLogic._onEdgeClick = null;
      SidebarLogic._resolvedFocusArc = null;
    });
  });

  // Minimal real DOM built from the actual HTML strings the sidebar renders
  // (no jsdom in this project). Supports exactly what production selectors
  // use: descendant `.class`/`[attr]` lookups and `:scope > a > b` child
  // chains, plus classList/attr/dataset/closest. This is what lets a test
  // exercise the real CSS selector strings in sidebar.js instead of a
  // hand-rolled querySelectorAll mock that echoes back canned answers
  // regardless of the selector text (which is why the ca-0372 nesting
  // regression slipped past the pre-existing collapse-all tests).
  function parseCompound(compound) {
    const attrs = [];
    let rest = compound.replace(/\[([^\]]+)\]/g, (_, a) => {
      attrs.push(a.split('=')[0].trim());
      return '';
    });
    const classes = [];
    rest = rest.replace(/\.([-a-zA-Z0-9_]+)/g, (_, c) => {
      classes.push(c);
      return '';
    });
    const tag = rest.trim() || null;
    return { tag, classes, attrs };
  }

  function matchesCompound(node, compound) {
    const { tag, classes, attrs } = parseCompound(compound);
    if (tag && node.tagName.toLowerCase() !== tag.toLowerCase()) return false;
    for (const c of classes) if (!node.classList.contains(c)) return false;
    for (const a of attrs) if (!node.hasAttribute(a)) return false;
    return true;
  }

  function descendants(node) {
    const out = [];
    const stack = [...node.children];
    while (stack.length) {
      const n = stack.pop();
      out.push(n);
      stack.push(...n.children);
    }
    return out;
  }

  function queryAll(root, selector) {
    const parts = selector.split('>').map((s) => s.trim());
    let level;
    if (parts[0] === ':scope') {
      level = [root];
      for (const compound of parts.slice(1)) {
        level = level.flatMap((n) =>
          n.children.filter((c) => matchesCompound(c, compound)),
        );
      }
    } else if (selector.startsWith(':scope ')) {
      // `:scope <compound>` (descendant combinator, no `>`): any descendant.
      const compound = selector.slice(':scope '.length).trim();
      level = descendants(root).filter((n) => matchesCompound(n, compound));
    } else {
      level = descendants(root).filter((n) => matchesCompound(n, parts[0]));
      for (const compound of parts.slice(1)) {
        level = level.flatMap((n) =>
          n.children.filter((c) => matchesCompound(c, compound)),
        );
      }
    }
    return level;
  }

  function makeNode(tag, attrs) {
    const classSet = new Set((attrs.class || '').split(/\s+/).filter(Boolean));
    const listeners = {};
    const style = {};
    for (const decl of (attrs.style || '').split(';')) {
      const [k, v] = decl.split(':');
      if (k && v) style[k.trim()] = v.trim();
    }
    const node = {
      tagName: tag,
      children: [],
      parentNode: null,
      innerHTML: '',
      style,
      get dataset() {
        const ds = {};
        for (const k of Object.keys(attrs)) {
          if (k.startsWith('data-')) {
            const camel = k
              .slice(5)
              .replace(/-([a-z])/g, (_, c) => c.toUpperCase());
            ds[camel] = attrs[k];
          }
        }
        return ds;
      },
      get nextElementSibling() {
        if (!node.parentNode) return null;
        const idx = node.parentNode.children.indexOf(node);
        return node.parentNode.children[idx + 1] ?? null;
      },
      getAttribute: (name) => (Object.hasOwn(attrs, name) ? attrs[name] : null),
      hasAttribute: (name) => Object.hasOwn(attrs, name),
      setAttribute: (name, v) => {
        attrs[name] = String(v);
      },
      removeAttribute: (name) => {
        delete attrs[name];
      },
      classList: {
        contains: (c) => classSet.has(c),
        add: (c) => classSet.add(c),
        remove: (c) => classSet.delete(c),
        toggle: (c, on) => {
          if (on) classSet.add(c);
          else classSet.delete(c);
        },
      },
      addEventListener(evt, fn) {
        if (!listeners[evt]) listeners[evt] = [];
        listeners[evt].push(fn);
      },
      _fire(evt, ev) {
        for (const fn of listeners[evt] || []) fn(ev);
      },
      querySelector(sel) {
        return queryAll(node, sel)[0] ?? null;
      },
      querySelectorAll(sel) {
        return queryAll(node, sel);
      },
      closest(sel) {
        let n = node;
        while (n) {
          if (matchesCompound(n, sel)) return n;
          n = n.parentNode;
        }
        return null;
      },
    };
    return node;
  }

  // Parses the sidebar's own generated HTML (div/details/summary/span/button,
  // always balanced, double-quoted attrs) into the node tree above.
  function parseFragment(html) {
    const root = makeNode('ROOT', {});
    const stack = [root];
    const tagRe = /<(\/)?([a-zA-Z][a-zA-Z0-9-]*)([^<>]*)>/g;
    let m = tagRe.exec(html);
    while (m) {
      const [, closing, tag, rawAttrs] = m;
      if (closing) {
        stack.pop();
      } else {
        const attrs = {};
        const attrRe = /([a-zA-Z_:][-a-zA-Z0-9_:.]*)(?:\s*=\s*"([^"]*)")?/g;
        let am = attrRe.exec(rawAttrs);
        while (am) {
          attrs[am[1]] = am[2] ?? '';
          am = attrRe.exec(rawAttrs);
        }
        const node = makeNode(tag.toUpperCase(), attrs);
        const parent = stack[stack.length - 1];
        parent.children.push(node);
        node.parentNode = parent;
        stack.push(node);
      }
      m = tagRe.exec(html);
    }
    return root;
  }

  describe('collapse-all controls cycle blocks (ca-0372)', () => {
    let savedClusters;
    let savedNodes;
    let savedArcs;

    beforeEach(() => {
      savedClusters = globalThis.STATIC_DATA.clusters;
      savedNodes = globalThis.STATIC_DATA.nodes;
      savedArcs = globalThis.STATIC_DATA.arcs;
      globalThis.STATIC_DATA.nodes = { ...savedNodes };
      for (const name of ['a', 'b']) {
        globalThis.STATIC_DATA.nodes[name] = {
          type: 'module',
          name,
          parent: 'crate_a',
          x: 0,
          y: 0,
          width: 100,
          height: 30,
          hasChildren: false,
        };
      }
      globalThis.STATIC_DATA.arcs = {
        ...savedArcs,
        'a-b': {
          from: 'a',
          to: 'b',
          usages: [
            {
              symbol: 'Foo',
              modulePath: null,
              locations: [{ file: 'a.rs', line: 1 }],
            },
          ],
        },
        'b-a': {
          from: 'b',
          to: 'a',
          usages: [
            {
              symbol: 'Bar',
              modulePath: null,
              locations: [{ file: 'b.rs', line: 2 }],
            },
          ],
        },
      };
      globalThis.STATIC_DATA.clusters = {
        0: {
          crate: 'my_crate',
          moduleCount: 2,
          cycleCount: 1,
          cycles: [
            [
              { fromId: 'a', toId: 'b', refs: 1 },
              { fromId: 'b', toId: 'a', refs: 1 },
            ],
          ],
        },
      };
      SidebarLogic._isClusterMode = () => true;
    });

    afterEach(() => {
      globalThis.STATIC_DATA.clusters = savedClusters;
      globalThis.STATIC_DATA.nodes = savedNodes;
      globalThis.STATIC_DATA.arcs = savedArcs;
      SidebarLogic._isClusterMode = null;
    });

    test('clicking collapse-all opens then closes every cycle block', () => {
      const html = SidebarLogic._buildClusterContent('0');
      const root = parseFragment(`<div class="sidebar-root">${html}</div>`);
      const blocks = root.querySelectorAll('.cycle-block');
      expect(blocks.length).toBeGreaterThan(0); // sanity: blocks exist
      expect(blocks.every((b) => b.hasAttribute('open'))).toBe(false); // start closed

      SidebarLogic._setupCollapseHandlers(root);
      const btn = root.querySelector('.sidebar-collapse-all');

      btn._fire('click'); // any closed → open all
      expect(blocks.every((b) => b.hasAttribute('open'))).toBe(true);
      expect(btn.innerHTML).toBe('−');

      btn._fire('click'); // all open → close all
      expect(blocks.some((b) => b.hasAttribute('open'))).toBe(false);
      expect(btn.innerHTML).toBe('+');
    });

    test('button glyph syncs when a block is opened manually', () => {
      const html = SidebarLogic._buildClusterContent('0');
      const root = parseFragment(`<div class="sidebar-root">${html}</div>`);
      SidebarLogic._setupCollapseHandlers(root);
      const content = root.querySelector('.sidebar-content');
      const btn = root.querySelector('.sidebar-collapse-all');
      const block = root.querySelectorAll('.cycle-block')[0];

      block.setAttribute('open', ''); // the only block is now open
      SidebarLogic._syncCollapseAllButton(root, content);
      expect(btn.innerHTML).toBe('−'); // all blocks open
    });
  });

  describe('cluster row interaction across duplicate arc ids (ca-0372 Phase 6)', () => {
    let savedClusters;
    let savedNodes;

    beforeEach(() => {
      savedClusters = globalThis.STATIC_DATA.clusters;
      savedNodes = globalThis.STATIC_DATA.nodes;
      globalThis.STATIC_DATA.nodes = { ...savedNodes };
      for (const name of ['a', 'b', 'c']) {
        globalThis.STATIC_DATA.nodes[name] = {
          type: 'module',
          name,
          parent: 'crate_a',
          x: 0,
          y: 0,
          width: 100,
          height: 30,
          hasChildren: false,
        };
      }
      // 'a-b' is a non-closing edge in both blocks.
      globalThis.STATIC_DATA.clusters = {
        0: {
          crate: 'my_crate',
          moduleCount: 3,
          cycleCount: 2,
          cycles: [
            [
              { fromId: 'a', toId: 'b', refs: 1 },
              { fromId: 'b', toId: 'a', refs: 1 },
            ],
            [
              { fromId: 'a', toId: 'b', refs: 1 },
              { fromId: 'b', toId: 'c', refs: 1 },
              { fromId: 'c', toId: 'a', refs: 1 },
            ],
          ],
        },
      };
      SidebarLogic._isClusterMode = () => true;
    });

    afterEach(() => {
      globalThis.STATIC_DATA.clusters = savedClusters;
      globalThis.STATIC_DATA.nodes = savedNodes;
      SidebarLogic._isClusterMode = null;
    });

    test('clicking one occurrence of a shared arc id focus-marks every occurrence', () => {
      const html = SidebarLogic._buildClusterContent('0');
      const root = parseFragment(`<div class="sidebar-root">${html}</div>`);
      const content = root.querySelector('.sidebar-content');
      const origUP = SidebarLogic.updatePosition;
      SidebarLogic.updatePosition = () => {};
      SidebarLogic._onEdgeClick = () => true; // pinned
      SidebarLogic._resolvedFocusArc = () => 'a-b';
      SidebarLogic._setupCollapseHandlers(root);

      const abRows = content
        .querySelectorAll('.sidebar-edge-row')
        .filter((r) => r.dataset.arcId === 'a-b');
      expect(abRows.length).toBe(2); // sanity: the shared edge renders twice

      content._fire('click', {
        target: {
          closest: (sel) => (sel === '.sidebar-edge-row' ? abRows[0] : null),
        },
      });

      expect(
        abRows.every((r) => r.classList.contains('sidebar-edge-row-focus')),
      ).toBe(true);

      SidebarLogic.updatePosition = origUP;
      SidebarLogic._onEdgeClick = null;
      SidebarLogic._resolvedFocusArc = null;
    });

    test('a summary click is not intercepted by the row/symbol click handler', () => {
      const html = SidebarLogic._buildClusterContent('0');
      const root = parseFragment(`<div class="sidebar-root">${html}</div>`);
      const content = root.querySelector('.sidebar-content');
      const origUP = SidebarLogic.updatePosition;
      SidebarLogic.updatePosition = () => {};
      let called = false;
      SidebarLogic._onEdgeClick = () => {
        called = true;
        return false;
      };
      SidebarLogic._setupCollapseHandlers(root);

      const summary = content.querySelector('.block-head');
      expect(summary).not.toBeNull();
      content._fire('click', { target: summary });

      expect(called).toBe(false); // native <details> toggle handles it, not our handler

      SidebarLogic.updatePosition = origUP;
      SidebarLogic._onEdgeClick = null;
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
          if (sel === ':scope .sidebar-symbol[data-collapsible]') return [];
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

  describe('_renderCollapseIndicator', () => {
    afterEach(() => {
      SidebarLogic._isNodeCollapsed = null;
    });

    test('returns empty string for leaf node (no children)', () => {
      SidebarLogic._isNodeCollapsed = () => false;
      // crate_a has hasChildren: false in STATIC_DATA
      expect(SidebarLogic._renderCollapseIndicator('crate_a')).toBe('');
    });

    test('returns + for collapsed parent node', () => {
      SidebarLogic._isNodeCollapsed = () => true;
      // Temporarily make node a parent
      const saved = globalThis.STATIC_DATA.nodes.crate_a.hasChildren;
      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = true;

      const html = SidebarLogic._renderCollapseIndicator('crate_a');
      expect(html).toContain('sidebar-collapse-indicator');
      expect(html).toContain('data-collapse-target="crate_a"');
      expect(html).toContain('>+<');

      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = saved;
    });

    test('returns \u2212 for expanded parent node', () => {
      SidebarLogic._isNodeCollapsed = () => false;
      const saved = globalThis.STATIC_DATA.nodes.crate_a.hasChildren;
      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = true;

      const html = SidebarLogic._renderCollapseIndicator('crate_a');
      expect(html).toContain('sidebar-collapse-indicator');
      expect(html).toContain('data-collapse-target="crate_a"');
      expect(html).toContain('>\u2212<');

      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = saved;
    });

    test('returns empty string when _isNodeCollapsed callback is not set', () => {
      SidebarLogic._isNodeCollapsed = null;
      const saved = globalThis.STATIC_DATA.nodes.crate_a.hasChildren;
      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = true;

      expect(SidebarLogic._renderCollapseIndicator('crate_a')).toBe('');

      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = saved;
    });

    test('returns empty string for unknown node', () => {
      SidebarLogic._isNodeCollapsed = () => false;
      expect(SidebarLogic._renderCollapseIndicator('nonexistent')).toBe('');
    });
  });

  describe('collapse indicator in badge rendering', () => {
    afterEach(() => {
      SidebarLogic._isNodeCollapsed = null;
    });

    test('buildNodeContent contains indicator for parent node', () => {
      const saved = globalThis.STATIC_DATA.nodes.crate_a.hasChildren;
      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = true;
      SidebarLogic._isNodeCollapsed = () => false;

      const html = SidebarLogic.buildNodeContent('crate_a', {
        incoming: [],
        outgoing: [],
      });
      expect(html).toContain('sidebar-collapse-indicator');
      expect(html).toContain('data-collapse-target="crate_a"');

      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = saved;
    });

    test('buildNodeContent contains no indicator for leaf node', () => {
      // crate_a has hasChildren: false
      SidebarLogic._isNodeCollapsed = () => false;

      const html = SidebarLogic.buildNodeContent('crate_a', {
        incoming: [],
        outgoing: [],
      });
      expect(html).not.toContain('sidebar-collapse-indicator');
    });

    test('buildContent header contains indicators for parent nodes', () => {
      const savedA = globalThis.STATIC_DATA.nodes.crate_a.hasChildren;
      const savedB = globalThis.STATIC_DATA.nodes.crate_b.hasChildren;
      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = true;
      globalThis.STATIC_DATA.nodes.crate_b.hasChildren = true;
      SidebarLogic._isNodeCollapsed = (id) => id === 'crate_a';

      const html = SidebarLogic.buildContent('crate_a-crate_b');
      // Both from and to should have indicators
      expect(html).toContain('data-collapse-target="crate_a"');
      expect(html).toContain('data-collapse-target="crate_b"');
      // crate_a is collapsed → +, crate_b is expanded → −
      const indicatorMatches = html.match(
        /sidebar-collapse-indicator[^>]*>([^<]+)</g,
      );
      expect(indicatorMatches).not.toBeNull();
      expect(indicatorMatches.length).toBeGreaterThanOrEqual(2);

      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = savedA;
      globalThis.STATIC_DATA.nodes.crate_b.hasChildren = savedB;
    });

    test('buildContent header has no indicators for leaf nodes', () => {
      // Both crate_a and crate_b have hasChildren: false by default
      SidebarLogic._isNodeCollapsed = () => false;

      const html = SidebarLogic.buildContent('crate_a-crate_b');
      expect(html).not.toContain('sidebar-collapse-indicator');
    });

    test('_buildRelationSection contains indicators for parent nodes', () => {
      const savedA = globalThis.STATIC_DATA.nodes.crate_a.hasChildren;
      const savedB = globalThis.STATIC_DATA.nodes.crate_b.hasChildren;
      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = true;
      globalThis.STATIC_DATA.nodes.crate_b.hasChildren = true;
      SidebarLogic._isNodeCollapsed = () => false;

      const relations = {
        incoming: [],
        outgoing: [
          {
            targetId: 'crate_b',
            weight: 2,
            arcId: 'crate_a-crate_b',
            usages: [
              {
                symbol: 'Foo',
                modulePath: null,
                locations: [{ file: 'a.rs', line: 1 }],
              },
            ],
          },
        ],
      };
      const html = SidebarLogic.buildNodeContent('crate_a', relations);
      expect(html).toContain('data-collapse-target="crate_a"');
      expect(html).toContain('data-collapse-target="crate_b"');

      globalThis.STATIC_DATA.nodes.crate_a.hasChildren = savedA;
      globalThis.STATIC_DATA.nodes.crate_b.hasChildren = savedB;
    });
  });

  describe('collapse indicator click handler', () => {
    function makeIndicatorMock(collapseTarget) {
      const listeners = new Map();
      return {
        dataset: { collapseTarget },
        addEventListener(evt, fn) {
          if (!listeners.has(evt)) listeners.set(evt, []);
          listeners.get(evt).push(fn);
        },
        _fire(evt, event) {
          for (const fn of listeners.get(evt) || []) fn(event);
        },
      };
    }

    function makeIndicatorHandlerDom(indicators, badges = []) {
      const contentListeners = new Map();
      const content = {
        querySelectorAll(sel) {
          if (sel === ':scope .sidebar-symbol[data-collapsible]') return [];
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
          if (sel === '.sidebar-collapse-indicator') return indicators;
          if (sel === '[data-node-id]') return badges;
          return [];
        },
      };
      return { root, content };
    }

    afterEach(() => {
      SidebarLogic._onCollapseToggle = null;
    });

    test('_onCollapseToggle is null by default', () => {
      // Reset in case previous test changed it
      const saved = SidebarLogic._onCollapseToggle;
      SidebarLogic._onCollapseToggle = null;
      expect(SidebarLogic._onCollapseToggle).toBeNull();
      SidebarLogic._onCollapseToggle = saved;
    });

    test('indicator click calls _onCollapseToggle with correct nodeId', () => {
      const calls = [];
      SidebarLogic._onCollapseToggle = (id) => calls.push(id);

      const indicator = makeIndicatorMock('parent_crate');
      const dom = makeIndicatorHandlerDom([indicator]);
      SidebarLogic._setupCollapseHandlers(dom.root);

      indicator._fire('click', { stopPropagation() {} });
      expect(calls).toEqual(['parent_crate']);
    });

    test('indicator click calls stopPropagation', () => {
      SidebarLogic._onCollapseToggle = () => {};

      const indicator = makeIndicatorMock('parent_crate');
      const dom = makeIndicatorHandlerDom([indicator]);
      SidebarLogic._setupCollapseHandlers(dom.root);

      let stopped = false;
      indicator._fire('click', {
        stopPropagation() {
          stopped = true;
        },
      });
      expect(stopped).toBe(true);
    });

    test('indicator click does not trigger badge navigation', () => {
      const badgeCalls = [];
      const collapseCalls = [];
      SidebarLogic._onBadgeClick = (id) => badgeCalls.push(id);
      SidebarLogic._onCollapseToggle = (id) => collapseCalls.push(id);

      const indicator = makeIndicatorMock('parent_crate');
      // Inline badge mock (makeBadgeMock is scoped to sibling describe)
      const badgeListeners = new Map();
      const badge = {
        dataset: { nodeId: 'parent_crate' },
        addEventListener(evt, fn) {
          if (!badgeListeners.has(evt)) badgeListeners.set(evt, []);
          badgeListeners.get(evt).push(fn);
        },
      };
      const dom = makeIndicatorHandlerDom([indicator], [badge]);
      SidebarLogic._setupCollapseHandlers(dom.root);

      // Click the indicator — should only trigger collapse, not badge nav
      indicator._fire('click', { stopPropagation() {} });
      expect(collapseCalls).toEqual(['parent_crate']);
      expect(badgeCalls).toEqual([]);

      SidebarLogic._onBadgeClick = null;
    });

    test('indicator click does nothing when _onCollapseToggle is null', () => {
      SidebarLogic._onCollapseToggle = null;

      const indicator = makeIndicatorMock('parent_crate');
      const dom = makeIndicatorHandlerDom([indicator]);
      SidebarLogic._setupCollapseHandlers(dom.root);

      expect(() => {
        indicator._fire('click', { stopPropagation() {} });
      }).not.toThrow();
    });
  });

  describe('_computeMaxBadgeLengths', () => {
    test('returns correct max lengths for incoming and outgoing', () => {
      // incoming: targets are "render" (6), "cli" (3) — these become fromName
      // selected node "crate_a" (7) is toName for all incoming
      // outgoing: selected "crate_a" (7) is fromName, target "crate_b" (7) is toName
      const relations = {
        incoming: [
          { targetId: 'mod_render', weight: 5, arcId: 'r-a', usages: [] },
          { targetId: 'mod_cli', weight: 3, arcId: 'c-a', usages: [] },
        ],
        outgoing: [
          { targetId: 'crate_b', weight: 2, arcId: 'a-b', usages: [] },
        ],
      };
      const result = SidebarLogic._computeMaxBadgeLengths(
        relations,
        'crate_a',
        'crate_a',
      );
      // incoming fromNames: "render"(6), "cli"(3) → maxFrom=6
      // incoming toName: always "crate_a"(7) → maxTo=7
      expect(result.incoming.maxFrom).toBe(6);
      expect(result.incoming.maxTo).toBe(7);
      // outgoing fromName: always "crate_a"(7) → maxFrom=7
      // outgoing toNames: "crate_b"(7) → maxTo=7
      expect(result.outgoing.maxFrom).toBe(7);
      expect(result.outgoing.maxTo).toBe(7);
    });

    test('empty sections return 0', () => {
      const relations = { incoming: [], outgoing: [] };
      const result = SidebarLogic._computeMaxBadgeLengths(
        relations,
        'crate_a',
        'crate_a',
      );
      expect(result.incoming.maxFrom).toBe(0);
      expect(result.incoming.maxTo).toBe(0);
      expect(result.outgoing.maxFrom).toBe(0);
      expect(result.outgoing.maxTo).toBe(0);
    });

    test('external node with version includes version in length', () => {
      globalThis.STATIC_DATA.nodes.ext_serde = {
        type: 'crate',
        name: 'serde',
        version: '1.0.0',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 30,
        hasChildren: false,
      };
      const relations = {
        incoming: [],
        outgoing: [
          { targetId: 'ext_serde', weight: 1, arcId: 'a-s', usages: [] },
        ],
      };
      const result = SidebarLogic._computeMaxBadgeLengths(
        relations,
        'crate_a',
        'crate_a',
      );
      // "serde v1.0.0" = 12 characters
      expect(result.outgoing.maxTo).toBe(12);
      delete globalThis.STATIC_DATA.nodes.ext_serde;
    });
  });

  describe('_buildRelationSection min-width', () => {
    test('applies min-width to from and to badge spans', () => {
      const rel = {
        targetId: 'mod_render',
        weight: 2,
        arcId: 'r-a',
        usages: [],
      };
      const html = SidebarLogic._buildRelationSection(
        rel,
        'crate_a',
        'crate_a',
        'crate',
        'incoming',
        10,
        8,
      );
      expect(html).toContain('min-width: 10ch');
      expect(html).toContain('min-width: 8ch');
    });

    test('applies min-width to badges with usages (L1 header)', () => {
      const rel = {
        targetId: 'mod_render',
        weight: 3,
        arcId: 'r-a',
        usages: [
          {
            symbol: 'Config',
            modulePath: 'config',
            locations: [{ file: 'src/render.rs', line: 10 }],
          },
        ],
      };
      const html = SidebarLogic._buildRelationSection(
        rel,
        'crate_a',
        'crate_a',
        'crate',
        'incoming',
        12,
        9,
      );
      expect(html).toContain('min-width: 12ch');
      expect(html).toContain('min-width: 9ch');
    });

    test('maxFromLen=0 omits min-width style on from badge', () => {
      const rel = {
        targetId: 'mod_render',
        weight: 2,
        arcId: 'r-a',
        usages: [],
      };
      const html = SidebarLogic._buildRelationSection(
        rel,
        'crate_a',
        'crate_a',
        'crate',
        'incoming',
        0,
        5,
      );
      // from badge should not have min-width
      const fromBadge = html.match(/sidebar-node-from[^>]*>render/);
      expect(fromBadge).not.toBeNull();
      expect(fromBadge[0]).not.toContain('min-width');
      // to badge should still have min-width
      expect(html).toContain('min-width: 5ch');
    });
  });

  describe('buildNodeContent badge width normalization', () => {
    test('incoming badges get normalized min-width from longest name', () => {
      // incoming targets: "render" (6ch), "cli" (3ch)
      // selected node "crate_a" (7ch) is toName for all incoming
      const relations = {
        incoming: [
          {
            targetId: 'mod_render',
            weight: 5,
            arcId: 'r-a',
            usages: [
              {
                symbol: 'Config',
                modulePath: null,
                locations: [{ file: 'src/render.rs', line: 10 }],
              },
            ],
          },
          {
            targetId: 'mod_cli',
            weight: 3,
            arcId: 'c-a',
            usages: [
              {
                symbol: 'run',
                modulePath: null,
                locations: [{ file: 'src/cli.rs', line: 5 }],
              },
            ],
          },
        ],
        outgoing: [],
      };
      const html = SidebarLogic.buildNodeContent('crate_a', relations);
      // All from-badges in incoming should have min-width: 6ch (max of "render", "cli")
      expect(html).toContain('min-width: 6ch');
      // All to-badges in incoming should have min-width: 7ch ("crate_a")
      expect(html).toContain('min-width: 7ch');
    });

    test('outgoing badges normalized independently from incoming', () => {
      // incoming: "render"(6) → "crate_a"(7)
      // outgoing: "crate_a"(7) → "crate_b"(7)
      const relations = {
        incoming: [
          {
            targetId: 'mod_render',
            weight: 2,
            arcId: 'r-a',
            usages: [
              {
                symbol: 'X',
                modulePath: null,
                locations: [{ file: 'a.rs', line: 1 }],
              },
            ],
          },
        ],
        outgoing: [
          {
            targetId: 'crate_b',
            weight: 1,
            arcId: 'a-b',
            usages: [
              {
                symbol: 'Y',
                modulePath: null,
                locations: [{ file: 'b.rs', line: 1 }],
              },
            ],
          },
        ],
      };
      const html = SidebarLogic.buildNodeContent('crate_a', relations);
      // incoming from: "render"(6) → min-width: 6ch
      // outgoing from: "crate_a"(7) and outgoing to: "crate_b"(7) → min-width: 7ch
      // Both 6ch and 7ch must appear
      expect(html).toContain('min-width: 6ch');
      expect(html).toContain('min-width: 7ch');
    });

    test('no relations: no min-width styles', () => {
      const html = SidebarLogic.buildNodeContent('crate_a', {
        incoming: [],
        outgoing: [],
      });
      expect(html).not.toContain('min-width');
    });
  });
});
