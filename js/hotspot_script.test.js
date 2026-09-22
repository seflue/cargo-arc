import { describe, expect, test } from 'bun:test';
import { createFakeElement, createMockDomAdapter } from './dom_adapter.js';
import { Follow } from './follow.js';
import { HotspotBars } from './hotspot_bars.js';
import { HotspotHover } from './hotspot_hover.js';
import { HotspotJumpIcon } from './hotspot_jump_icon.js';
import { HotspotLabels } from './hotspot_labels.js';
import { HotspotLayout } from './hotspot_layout.js';
import { HotspotSelection } from './hotspot_selection.js';
import { HotspotTree } from './hotspot_tree.js';
import { HotspotZoom } from './hotspot_zoom.js';
import { Jump } from './jump.js';
import { JumpSymbol } from './jump_symbol.js';
import { PageLink } from './page_link.js';
import { TextMeasure } from './text_metrics.js';
import { Theme } from './theme.js';

// A <select> stub with enough behavior for bootstrapHotspotPage: appendChild
// (from createFakeElement) plus a no-op addEventListener.
function createFakeSelect() {
  const el = createFakeElement('select');
  el.addEventListener = () => {};
  return el;
}

// An element with real addEventListener/_fire, so tests can trigger the
// handlers hotspot_script.js attaches (pattern: makeBadgeMock in
// sidebar.test.js, createFakeSvgElement in jump_icons.test.js).
function createFakeInteractiveElement(tag) {
  const el = createFakeElement(tag);
  const listeners = new Map();
  el.addEventListener = (evt, fn) => {
    if (!listeners.has(evt)) listeners.set(evt, []);
    listeners.get(evt).push(fn);
  };
  el._fire = (evt, event = {}) => {
    for (const fn of listeners.get(evt) || []) fn(event);
  };
  return el;
}

// A minimal but structurally real map: a crate root with one ranked leaf
// (so the details panel, the hotspot list and the bars view all have real
// content to render), plus the sidebar and toolbar elements this task
// wires. Built with the same mock adapter production code and other tests
// use (ADR-010), not a one-off hand-rolled double.
function createFakeMap() {
  const dom = createMockDomAdapter();
  const svg = createFakeElement('svg');
  // The root SVG sizes itself to its container (width="100%" height="100%");
  // the viewBox carries the actual pixel dimensions HotspotLayout works with.
  svg.viewBox = { baseVal: { width: 400, height: 400 } };
  // buildHotspotMap measures the svg's own rendered box, not the window
  // (a served page can size it to a different aspect ratio than the
  // window). 400x400 matches this fixture's own pre-existing pixel
  // assertions.
  svg.getBoundingClientRect = () => ({ width: 400, height: 400 });
  svg.addEventListener = () => {};
  dom._registerSelector('svg', svg);

  const mapContent = createFakeElement('g');
  dom._registerElement('map-content', mapContent);

  const rootCircle = createFakeElement('circle');
  rootCircle.setAttribute('class', 'hotspot-circle');
  rootCircle.setAttribute('data-file', 'root');
  const hotCircle = createFakeElement('circle');
  hotCircle.setAttribute('class', 'hotspot-circle');
  hotCircle.setAttribute('data-file', 'hot');
  const warmCircle = createFakeElement('circle');
  warmCircle.setAttribute('class', 'hotspot-circle');
  warmCircle.setAttribute('data-file', 'warm');
  dom._registerSelector('.hotspot-circle', [rootCircle, hotCircle, warmCircle]);

  // One `<text data-label>` per circle, as `render::hotspots::circle_svg`
  // emits it - found by that attribute, not DOM sibling order.
  const rootLabel = createFakeElement('text');
  rootLabel.setAttribute('data-label', 'root');
  const hotLabel = createFakeElement('text');
  hotLabel.setAttribute('data-label', 'hot');
  const warmLabel = createFakeElement('text');
  warmLabel.setAttribute('data-label', 'warm');
  dom._registerSelector('[data-label]', [rootLabel, hotLabel, warmLabel]);

  const detailsEl = createFakeElement('div');
  const listEl = createFakeInteractiveElement('ul');
  listEl.innerHTML =
    '<li class="hotspot-list-item" data-file="hot">1. hot.rs — 100×4</li>';
  const listToggleEl = createFakeInteractiveElement('button');
  const sidebarEl = createFakeElement('div');
  const arcLinkEl = createFakeElement('a');
  // Starts hidden, the same `style="display:none"` the static SVG carries
  // on `#toolbar-fo` (so the file still opens sensibly outside a browser).
  const toolbarFoEl = createFakeElement('foreignObject');
  toolbarFoEl.style.display = 'none';

  return {
    dom,
    svg,
    mapContent,
    rootCircle,
    hotCircle,
    warmCircle,
    hotLabel,
    warmLabel,
    detailsEl,
    listEl,
    listToggleEl,
    sidebarEl,
    arcLinkEl,
    toolbarFoEl,
  };
}

const fixture = createFakeMap();
const {
  dom,
  svg,
  mapContent,
  hotCircle,
  hotLabel,
  warmLabel,
  detailsEl,
  listEl,
  listToggleEl,
  sidebarEl,
  arcLinkEl,
  toolbarFoEl,
} = fixture;
const jumpStatusEl = createFakeElement('span');
const elements = {
  'theme-mode': createFakeSelect(),
  'theme-light': createFakeSelect(),
  'theme-dark': createFakeSelect(),
  'hotspot-details': detailsEl,
  'hotspot-list': listEl,
  'hotspot-list-toggle': listToggleEl,
  'hotspot-sidebar': sidebarEl,
  'arc-page-link': arcLinkEl,
  'jump-status': jumpStatusEl,
  'toolbar-fo': toolbarFoEl,
};
// hotspot_script.js reads DomAdapter, document, window, location, Theme and
// STATIC_DATA as bare globals, the way the concatenated bundle provides
// them on a real page. Set them up before requiring it, so its own
// module-scope bootstrap (guarded on `document`) runs against these stubs
// rather than whatever an earlier test file left behind.
global.DomAdapter = {
  ...dom,
  getElementById: (id) => elements[id] ?? dom.getElementById(id),
  // HotspotJumpIcon's glyph adds its own click listener (dom_adapter.js's
  // plain createFakeElement does not carry one), the same fixture pattern
  // jump_icons.test.js uses for its own fake SVG elements.
  createSvgElement: (tag) => createFakeInteractiveElement(tag),
};
global.document = {
  documentElement: { dataset: {} },
  createElement: (tag) => createFakeElement(tag),
};
global.window = {
  matchMedia: () => ({ matches: false, addEventListener() {} }),
  addEventListener: () => {},
};
// No `?select` in this fixture's location: buildHotspotMap must not zoom on
// load, since requestAnimationFrame (stubbed below) never runs a real frame.
global.location = { search: '' };
global.requestAnimationFrame = () => 0;
global.cancelAnimationFrame = () => {};
// Records every observer buildHotspotMap creates and what it observes, so a
// test can check it is the svg root, not `window`, without a real browser.
const resizeObservers = [];
global.ResizeObserver = class {
  constructor(callback) {
    this.callback = callback;
    resizeObservers.push(this);
  }
  observe(target) {
    this.target = target;
  }
  disconnect() {}
};
global.STATIC_DATA = {
  theme: {
    light: [{ name: 'latte', label: 'Latte' }],
    dark: [{ name: 'mocha', label: 'Mocha' }],
  },
  // The same names `render::hotspots::render` builds from `CSS.hotspots`.
  classes: {
    circle: 'hotspot-circle',
    listItem: 'hotspot-list-item',
    hover: 'hotspot-hover',
    selected: 'hotspot-selected',
    barLabel: 'hotspot-bar-label',
    barTrack: 'hotspot-bar-track',
    barFill: 'hotspot-bar-fill',
    detailsTitle: 'hotspot-details-title',
    tooltip: 'hotspot-tooltip',
    // A value only this fixture would produce, so the test below only
    // passes if `hotspot_script.js` actually forwards it into the icon.
    jumpIcon: 'fixture-jump-icon',
  },
  hotspots: ['hot'],
  maxFileCommits: 4,
  // Every constant at zero: with this fixture's 400x400 window, the layout
  // this produces is exactly the fixed 400x400 canvas the tests below were
  // written against. `hotspot_layout.test.js` covers the arithmetic itself
  // with realistic, non-zero constants.
  layout: { sidebarWidth: 0, sidebarGap: 0, mapMargin: 0, toolbarHeight: 0 },
  nodes: {
    // The root sits centred in its own square footprint (200, 200 of a
    // 400×400 area), the way `render::hotspots::render` draws it - the
    // page's own SVG width may exceed that footprint (it also carries the
    // always-visible sidebar beside the circle), so buildHotspotMap derives
    // its viewport from the root's own position, not the SVG width.
    root: {
      kind: 'crate',
      name: 'root',
      file: 'Cargo.toml',
      cx: 200,
      cy: 200,
      r: 50,
      lines: 110,
      commits: 4,
      fillPercent: 100,
    },
    hot: {
      kind: 'file',
      name: 'hot.rs',
      file: 'src/hot.rs',
      parent: 'root',
      cx: 210,
      cy: 200,
      r: 15,
      lines: 100,
      commits: 4,
      rank: 1,
      fillPercent: 100,
      targets: [{ kind: 'module', name: 'hot.rs', jump: 3 }],
    },
    // Small and far enough from `hot` that both circles' labels are shown
    // at once, at font sizes that differ because their radii do.
    warm: {
      kind: 'file',
      name: 'warm.rs',
      file: 'src/warm.rs',
      parent: 'root',
      cx: 170,
      cy: 200,
      r: 6,
      lines: 10,
      commits: 1,
      fillPercent: 50,
    },
  },
};
global.Theme = Theme;
global.HotspotTree = HotspotTree;
global.HotspotZoom = HotspotZoom;
global.HotspotLabels = HotspotLabels;
global.HotspotHover = HotspotHover;
global.TextMeasure = TextMeasure;
global.HotspotSelection = HotspotSelection;
global.HotspotBars = HotspotBars;
global.PageLink = PageLink;
global.Follow = Follow;
global.HotspotJumpIcon = HotspotJumpIcon;
global.JumpSymbol = JumpSymbol;
global.HotspotLayout = HotspotLayout;
global.Jump = Jump;
// A click on a jump chip would call this; no test here exercises a click.
global.fetch = () => Promise.resolve({ ok: true });

const {
  bootstrapHotspotPage,
  buildHotspotMap,
} = require('./hotspot_script.js');

describe('hotspot_script entry', () => {
  test('bootstraps the theme controls from STATIC_DATA', () => {
    // The module's own auto-invoke already ran once at require() time;
    // clear its effect so only this call's is asserted on.
    elements['theme-light'].children.length = 0;
    elements['theme-dark'].children.length = 0;

    const control = bootstrapHotspotPage();

    expect(elements['theme-light'].children.map((o) => o.value)).toEqual([
      'latte',
    ]);
    expect(elements['theme-dark'].children.map((o) => o.value)).toEqual([
      'mocha',
    ]);
    expect(document.documentElement.dataset.theme).toBe('latte');
    expect(typeof control.setMode).toBe('function');
  });

  test('buildHotspotMap wires the map without throwing and returns its api', () => {
    const map = buildHotspotMap();
    expect(map).not.toBeNull();
    expect(typeof map.zoomTo).toBe('function');
    expect(typeof map.setHover).toBe('function');
    expect(typeof map.select).toBe('function');
    expect(typeof map.focus).toBe('function');
    expect(typeof map.resize).toBe('function');
    // It frames the initial target (the root, margin included) on the
    // transform it sets on #map-content.
    const [, scaleText] =
      mapContent.getAttribute('transform').match(/scale\(([^)]+)\)/) ?? [];
    const expectedScale =
      400 / (2 * HotspotZoom.viewFor(STATIC_DATA.nodes.root).radius);
    expect(Number(scaleText)).toBeCloseTo(expectedScale, 5);
    // The sidebar's list/bars/details are always visible, unlike the arc
    // page's contextual relation sidebar.
    expect(sidebarEl.style.display).toBe('block');
    // The toolbar starts hidden in the static SVG, so a file opens sensibly
    // outside a browser. buildHotspotMap must reveal it once it runs.
    expect(toolbarFoEl.style.display).not.toBe('none');
  });

  test('observes the svg root for its own box changing size, not the window', () => {
    buildHotspotMap();
    const observer = resizeObservers.at(-1);
    expect(observer).toBeDefined();
    expect(observer.target).toBe(svg);
  });

  test('a viewport too narrow for the furniture still yields a map, never a permanent blank', () => {
    const savedLayout = STATIC_DATA.layout;
    const savedRect = svg.getBoundingClientRect;
    // The furniture this repo actually renders with: a viewport narrower
    // than sidebarGap + sidebarWidth + 2*mapMargin (a phone-width window)
    // used to leave the map area at zero and buildHotspotMap returning null.
    STATIC_DATA.layout = {
      sidebarWidth: 280,
      sidebarGap: 20,
      mapMargin: 20,
      toolbarHeight: 40,
    };
    svg.getBoundingClientRect = () => ({ width: 320, height: 800 });
    try {
      const map = buildHotspotMap();
      expect(map).not.toBeNull();
      const [, scaleText] =
        mapContent.getAttribute('transform').match(/scale\(([^)]+)\)/) ?? [];
      const expectedScale =
        HotspotLayout.MIN_MAP_AREA_SIZE /
        (2 * HotspotZoom.viewFor(STATIC_DATA.nodes.root).radius);
      expect(Number(scaleText)).toBeCloseTo(expectedScale, 5);
    } finally {
      STATIC_DATA.layout = savedLayout;
      svg.getBoundingClientRect = savedRect;
    }
  });

  test('positions the viewBox, the toolbar and the sidebar for the svg’s own measured box, from STATIC_DATA.layout', () => {
    // Realistic, non-zero furniture constants for this one test; every
    // other test in this file keeps the shared fixture's all-zero layout so
    // its pre-existing pixel assertions stay valid.
    const savedLayout = STATIC_DATA.layout;
    const savedRect = svg.getBoundingClientRect;
    STATIC_DATA.layout = {
      sidebarWidth: 100,
      sidebarGap: 10,
      mapMargin: 5,
      toolbarHeight: 30,
    };
    // A window a different size than the box below: buildHotspotMap must
    // size the page from the svg's own box, not from the window.
    window.innerWidth = 1600;
    window.innerHeight = 1200;
    svg.getBoundingClientRect = () => ({ width: 1000, height: 700 });
    try {
      const map = buildHotspotMap();
      expect(map).not.toBeNull();
      expect(svg.viewBox.baseVal).toEqual({ width: 1000, height: 700 });
      expect(toolbarFoEl.getAttribute('width')).toBe('1000');
      expect(sidebarEl.getAttribute('x')).toBe('900');
      expect(sidebarEl.getAttribute('height')).toBe('700');
    } finally {
      STATIC_DATA.layout = savedLayout;
      svg.getBoundingClientRect = savedRect;
    }
  });

  test('a resize rescales the map for its new box, framing the zoomed-to container, not the root', () => {
    // Zoom away from the root before resizing: a scale that merely tracks
    // the root's own radius would look identical before and after the
    // resize either way, so it cannot tell a real re-frame from a snap
    // back to the root that reads the root's data straight from STATIC_DATA.
    const container = {
      kind: 'crate',
      name: 'container',
      file: 'container',
      parent: 'root',
      cx: 220,
      cy: 190,
      r: 30,
      lines: 50,
      commits: 2,
      fillPercent: 80,
    };
    STATIC_DATA.nodes.container = container;
    const originalRaf = global.requestAnimationFrame;
    // Completes the animation in one synchronous frame, past its duration,
    // so `view` lands exactly on `container` instead of mid-flight. The
    // timestamp is relative to `performance.now()`, not to `zoomTo`'s own
    // start time (opaque to this test), so it adds a full duration of slack.
    global.requestAnimationFrame = (fn) =>
      fn(performance.now() + HotspotZoom.ZOOM_MS * 2);
    try {
      const map = buildHotspotMap();
      expect(map).not.toBeNull();
      map.zoomTo('container');

      map.resize(1200, 1200);

      const [, txText, tyText, scaleText] =
        mapContent
          .getAttribute('transform')
          .match(/translate\(([^ ]+) ([^)]+)\) scale\(([^)]+)\)/) ?? [];
      const expectedScale = 1200 / (2 * container.r * HotspotZoom.ZOOM_MARGIN);
      expect(Number(scaleText)).toBeCloseTo(expectedScale, 5);
      // The container's own centre still maps to the (square) map area's
      // centre, half the new box on each axis.
      const drawnCx = Number(txText) + container.cx * Number(scaleText);
      const drawnCy = Number(tyText) + container.cy * Number(scaleText);
      expect(drawnCx).toBeCloseTo(600, 5);
      expect(drawnCy).toBeCloseTo(600, 5);
    } finally {
      global.requestAnimationFrame = originalRaf;
      delete STATIC_DATA.nodes.container;
    }
  });

  test('a resize updates the tooltip’s clamp width, flipping a tooltip near the new right edge', () => {
    const map = buildHotspotMap();
    const tooltipLayer = svg.children
      .filter((child) => child.getAttribute('id') === 'hotspot-tooltip-layer')
      .pop();

    map.resize(150, 150);
    map.setHover('hot', { x: 140, y: 50 });

    const group = tooltipLayer.children.at(-1);
    const [bg] = group.children;
    const x = Number(bg.getAttribute('x'));
    const width = Number(bg.getAttribute('width'));
    expect(x).toBeLessThan(140);
    expect(x + width).toBeLessThanOrEqual(150);
  });

  test('gives circles of different radius different label font sizes, as a valid CSS length', () => {
    const map = buildHotspotMap();
    expect(map).not.toBeNull();

    expect(hotLabel.style.display).toBe('inline');
    expect(warmLabel.style.display).toBe('inline');
    expect(hotLabel.style.fontSize).not.toBe(warmLabel.style.fontSize);
    // A bare number is not a valid CSS <length>; the value must carry a unit.
    expect(hotLabel.style.fontSize).toMatch(/^[\d.]+px$/);
    expect(warmLabel.style.fontSize).toMatch(/^[\d.]+px$/);
    const placedFontSize = (label) =>
      HotspotLabels.placeLabels(
        STATIC_DATA.nodes,
        'root',
        null,
        Number(
          mapContent.getAttribute('transform').match(/scale\(([^)]+)\)/)[1],
        ),
      ).get(label === hotLabel ? 'hot' : 'warm').fontSize;
    expect(Number.parseFloat(hotLabel.style.fontSize)).toBeCloseTo(
      placedFontSize(hotLabel),
      5,
    );
    expect(Number.parseFloat(warmLabel.style.fontSize)).toBeCloseTo(
      placedFontSize(warmLabel),
      5,
    );
  });

  test('hovering a circle adds the hover class, clearing it on the previous one', () => {
    const map = buildHotspotMap();

    map.setHover('hot');
    expect(hotCircle.classList.contains('hotspot-hover')).toBe(true);

    map.setHover('warm');
    expect(hotCircle.classList.contains('hotspot-hover')).toBe(false);
    expect(fixture.warmCircle.classList.contains('hotspot-hover')).toBe(true);

    map.setHover(null);
    expect(fixture.warmCircle.classList.contains('hotspot-hover')).toBe(false);
  });

  test('selecting a circle adds the selected class, kept while hover moves elsewhere', () => {
    const map = buildHotspotMap();

    map.select('hot');
    expect(hotCircle.classList.contains('hotspot-selected')).toBe(true);

    map.setHover('warm');
    expect(hotCircle.classList.contains('hotspot-selected')).toBe(true);
    expect(fixture.warmCircle.classList.contains('hotspot-hover')).toBe(true);

    map.setHover(null);
    expect(hotCircle.classList.contains('hotspot-selected')).toBe(true);
  });

  test('select() renders the sidebar details and the cross-page link for the selected leaf', () => {
    const map = buildHotspotMap();

    map.select('hot');

    expect(detailsEl.innerHTML).toContain('src/hot.rs');
    expect(detailsEl.innerHTML).toContain('lines');
    expect(detailsEl.innerHTML).toContain('100');
    expect(detailsEl.innerHTML).toContain('<table>');
    expect(detailsEl.innerHTML).toContain('<td>lines</td><td>100</td>');
    expect(arcLinkEl.getAttribute('href')).toBe(
      PageLink.buildLink('/', 'src/hot.rs'),
    );
  });

  test('select(null) clears the details panel and the cross-page link', () => {
    const map = buildHotspotMap();

    map.select('hot');
    map.select(null);

    expect(detailsEl.innerHTML).toBe('');
    expect(arcLinkEl.getAttribute('href')).toBe('/');
  });

  test('selecting a leaf with a jump target shows a jump icon there, no icon otherwise', () => {
    const map = buildHotspotMap();
    // The shared `svg` fixture accumulates one jump layer per test's own
    // `buildHotspotMap()` call (nothing removes an earlier one); this one's
    // is the last appended.
    const jumpLayer = svg.children
      .filter((child) => child.getAttribute('id') === 'hotspot-jump-layer')
      .pop();
    expect(jumpLayer).toBeDefined();

    map.select('hot'); // carries a target in this fixture
    expect(jumpLayer.children.length).toBe(1);
    // One glyph at the circle's edge, not the arc page's popover: no chip,
    // no kind label, no background panel.
    const [icon] = jumpLayer.children;
    expect(icon.tagName).toBe('use');
    expect(icon.getAttribute('href')).toBe('#jump-icon');
    // STATIC_DATA.classes.jumpIcon forwarded through (see the fixture's
    // comment on that value).
    expect(icon.getAttribute('class')).toBe('fixture-jump-icon');
    expect(icon.children.find((c) => c.tagName === 'title').textContent).toBe(
      'hot.rs',
    );

    map.select(null);
    expect(jumpLayer.children.length).toBe(0);

    map.select('warm'); // no target in this fixture
    expect(jumpLayer.children.length).toBe(0);
  });

  test('focus() selects the leaf without throwing, ready to zoom to its parent', () => {
    const map = buildHotspotMap();

    expect(() => map.focus('hot')).not.toThrow();

    expect(detailsEl.innerHTML).toContain('src/hot.rs');
  });

  test('the list/bars toggle swaps the list content and its own label, and back', () => {
    buildHotspotMap();
    const original = listEl.innerHTML;

    listToggleEl._fire('click');
    expect(listToggleEl.getAttribute('aria-pressed')).toBe('true');
    expect(listToggleEl.textContent).toBe('List');
    expect(listEl.innerHTML).not.toBe(original);
    expect(listEl.innerHTML).toContain('data-file="hot"');

    listToggleEl._fire('click');
    expect(listToggleEl.getAttribute('aria-pressed')).toBe('false');
    expect(listToggleEl.textContent).toBe('Bars');
    expect(listEl.innerHTML).toBe(original);
  });

  test('clicking a hotspot list row selects and zooms to it', () => {
    buildHotspotMap();
    const row = {
      getAttribute: () => 'hot',
      closest: (sel) => (sel === '[data-file]' ? row : null),
    };

    listEl._fire('click', { target: row });

    expect(detailsEl.innerHTML).toContain('src/hot.rs');
  });

  test('a list row’s click and hover stop propagation, so the svg’s own circle handlers do not immediately undo them', () => {
    // buildHotspotMap() runs once more here, its own listeners stacking
    // onto listEl alongside every earlier test's in this shared fixture
    // (ADR-010's mock adapter, not reset between tests); each still calls
    // stopPropagation, which is what this test checks for.
    buildHotspotMap();
    const row = {
      getAttribute: () => 'hot',
      closest: (sel) => (sel === '[data-file]' ? row : null),
    };
    let pointeroverStopped = false;
    let clickStopped = false;

    listEl._fire('pointerover', {
      target: row,
      stopPropagation: () => {
        pointeroverStopped = true;
      },
    });
    listEl._fire('click', {
      target: row,
      stopPropagation: () => {
        clickStopped = true;
      },
    });

    expect(pointeroverStopped).toBe(true);
    expect(clickStopped).toBe(true);
  });
});
