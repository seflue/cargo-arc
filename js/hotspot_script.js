// @module HotspotScript
// @deps Theme, DomAdapter, HotspotTree, HotspotZoom, HotspotLabels, HotspotHover, HotspotSelection, HotspotBars, PageLink, Follow, OnSaveToggle, HotspotJumpIcon, Jump, HotspotLayout, PathFit
// @config
// hotspot_script.js - entry module for the hotspot map page. Applies the
// theme STATIC_DATA carries, wires the map's zoom with its breadcrumb,
// labels and hover, its
// selection (click, sidebar details, the hotspot list/bars, editor follow
// and `?select` on load), and the jump icon at the selected leaf.

function bootstrapHotspotPage() {
  return Theme.bootstrapControls();
}

// Named once, in `render::hotspots::render`'s own `CSS.hotspots`; STATIC_DATA
// carries the names so JS never repeats them as a separate literal.
const CIRCLE_CLASS = STATIC_DATA.classes.circle;
const LIST_ITEM_CLASS = STATIC_DATA.classes.listItem;
const HOVER_CLASS = STATIC_DATA.classes.hover;
const SELECTED_CLASS = STATIC_DATA.classes.selected;

/** The `data-file` key a circle (or its click target) belongs to, or `null` outside any circle. */
function circleKeyAt(event) {
  const circle = event.target.closest?.(`.${CIRCLE_CLASS}`);
  return circle ? circle.getAttribute('data-file') : null;
}

/** The `data-file` key a hotspot list row (or its click target) belongs to, or `null` outside any row. */
function rowKeyAt(event) {
  const target = /** @type {Element | null} */ (event.target);
  const row = target?.closest?.('[data-file]');
  return row ? row.getAttribute('data-file') : null;
}

/** The `data-crumb` key a breadcrumb entry (or its click target) zooms to, or `null` outside any entry. */
function crumbKeyAt(event) {
  const target = /** @type {Element | null} */ (event.target);
  const crumb = target?.closest?.('[data-crumb]');
  return crumb ? crumb.getAttribute('data-crumb') : null;
}

/** The path spans in the sidebar: rows and bar labels carrying `data-full`. */
function sidebarPaths(sidebarFo) {
  const inner = sidebarFo?.firstElementChild;
  return inner ? [...inner.querySelectorAll('[data-full]')] : [];
}

/**
 * The sidebar's natural content width with every path at full length,
 * measured the way `SidebarLogic.updatePosition` measures the arc page's;
 * `undefined` when there is nothing to measure.
 */
function measureSidebarWidth(sidebarFo) {
  const inner = sidebarFo?.firstElementChild;
  if (!inner) return undefined;
  for (const span of sidebarPaths(sidebarFo)) PathFit.resetPath(span);
  sidebarFo.setAttribute('width', '9999');
  inner.style.width = 'max-content';
  const natural = inner.offsetWidth;
  inner.style.width = '';
  return natural;
}

/** Shorten every sidebar path that overflows its cell, keeping the file name. */
function fitSidebarPaths(sidebarFo) {
  for (const span of sidebarPaths(sidebarFo)) {
    PathFit.fitPath(span, '/', (s) => s.scrollWidth > s.clientWidth);
  }
}

/** Escapes text before it is interpolated into `innerHTML`, matching the
 * Rust side's own `escape_xml`. */
function escapeHtml(text) {
  return String(text)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;');
}

/**
 * Wires the map's zoom, label visibility and placement, hover tooltip and
 * selection (click, sidebar details, the hotspot list/bars, editor follow
 * and `?select` on load) onto the already-drawn circles and sidebar, and
 * the window-sized layout (`HotspotLayout`) that keeps the toolbar, the
 * sidebar and the outer `<svg>`'s own viewBox sized to the browser window.
 * The zoom itself never touches that viewBox - it is shared with the
 * toolbar and sidebar `foreignObject`s - so it is a `transform` on
 * `g#map-content` instead; the pure zoom math (`HotspotZoom.viewFor`,
 * `.viewBoxAt`) is the same either way, only where it is applied differs.
 * @param {ReturnType<typeof Theme.bootstrapControls> | undefined} themeControl -
 *   forwards the editor's theme events to the same control the appearance
 *   toolbar drives, so `arc theme` events keep working while following is on.
 */
function buildHotspotMap(themeControl) {
  const svg = DomAdapter.getSvgRoot();
  const mapContent = DomAdapter.getElementById('map-content');
  if (!svg || !mapContent) return null;

  // STATIC_DATA's ambient type is shared with the arc page; this page's own
  // nodes are `render::hotspots::HotspotCircleData`, not `StaticNodeData`.
  const nodes = /** @type {Record<string, HotspotCircleData>} */ (
    /** @type {unknown} */ (STATIC_DATA.nodes)
  );
  const rootKey = HotspotTree.rootKey(nodes);
  // The window-sized layout: the viewBox, the toolbar and sidebar rects,
  // and the square area left over for the packed circles. Measured off the
  // root svg's own box, not `window.innerWidth`/`innerHeight`: the served
  // page can size that box to a different aspect ratio than the window
  // (`render::html_page`'s doc comment), so only the box the svg actually
  // occupies gives a `viewBox` that keeps one SVG unit equal to one CSS
  // pixel. `resize()` below recomputes it for a new box and reapplies it;
  // the initial values live in the same `let`s so both paths update them
  // the same way. The sidebar takes its content's width, as on the arc
  // page, so it must be shown before it is measured.
  const sidebarEl = DomAdapter.getElementById('hotspot-sidebar');
  if (sidebarEl) sidebarEl.style.display = 'block';
  function layoutFor(box) {
    const sidebarWidth = HotspotLayout.sidebarWidthFor(
      measureSidebarWidth(sidebarEl),
      box.width,
      STATIC_DATA.layout.sidebarWidth,
    );
    const next = HotspotLayout.computeLayout(
      box,
      STATIC_DATA.layout,
      sidebarWidth,
    );
    HotspotLayout.apply(
      {
        svg,
        toolbarFo: DomAdapter.getElementById('toolbar-fo'),
        sidebarFo: sidebarEl,
      },
      next,
    );
    fitSidebarPaths(sidebarEl);
    return next;
  }
  let layout = layoutFor(svg.getBoundingClientRect());
  const canvasWidth = layout.viewBox.width;
  let canvasHeight = layout.viewBox.height;
  let mapAreaSize = layout.mapAreaSize;
  if (!mapAreaSize || !canvasHeight) return null;

  const tooltipLayer = DomAdapter.createSvgElement('g');
  tooltipLayer.setAttribute('id', 'hotspot-tooltip-layer');
  svg.appendChild(tooltipLayer);
  const tooltip = HotspotHover.createHoverTooltip({
    layer: tooltipLayer,
    canvasWidth,
    tooltipClass: STATIC_DATA.classes.tooltip,
  });

  function showJumpStatus(text) {
    const statusEl = DomAdapter.getElementById('jump-status');
    if (statusEl) statusEl.textContent = text;
  }
  const jumper = Jump.createJump((url) => fetch(url), showJumpStatus);

  // Same idiom as the tooltip layer: a sibling of `g#map-content`, in the
  // outer `<svg>`'s own coordinate space, so the icon does not scale with
  // the zoom transform.
  const jumpLayer = DomAdapter.createSvgElement('g');
  jumpLayer.setAttribute('id', 'hotspot-jump-layer');
  svg.appendChild(jumpLayer);
  const jumpIcon = HotspotJumpIcon.createHotspotJumpIcon({
    layer: jumpLayer,
    defsHost: svg,
    onJump: (id) => jumper.jump(id),
    iconClass: STATIC_DATA.classes.jumpIcon,
  });

  const circles = new Map();
  for (const circle of DomAdapter.querySelectorAll(`.${CIRCLE_CLASS}`)) {
    circles.set(circle.getAttribute('data-file'), circle);
  }
  // Found by `data-label`, not `circle.nextElementSibling`: sibling order is
  // an implicit contract with `render::hotspots::circle_svg` that a future
  // change there could silently break.
  const labelEls = new Map();
  for (const label of DomAdapter.querySelectorAll('[data-label]')) {
    labelEls.set(label.getAttribute('data-label'), label);
  }

  const detailsEl = DomAdapter.getElementById('hotspot-details');
  const listEl = DomAdapter.getElementById('hotspot-list');
  const showListEl = DomAdapter.getElementById('hotspot-show-list');
  const showBarsEl = DomAdapter.getElementById('hotspot-show-bars');
  const pageLinkEl = DomAdapter.getElementById('arc-page-link');
  const breadcrumbEl = DomAdapter.getElementById('hotspot-breadcrumb');
  // The static SVG starts the toolbar hidden so a file opens sensibly
  // outside a browser. This reveals it, as `svg_script.js` does for the arc page.
  const toolbarFoEl = DomAdapter.getElementById('toolbar-fo');
  if (toolbarFoEl) toolbarFoEl.style.display = '';
  const originalListHtml = listEl ? listEl.innerHTML : '';

  let targetKey = rootKey;
  let hoveredKey = null;
  let selectedKey = null;
  // Which of a selection or a zoom the cross-page link should carry, per
  // whichever happened last (`updatePageLink`'s own doc comment); `null`
  // before either has happened, carrying nothing.
  let lastLinkSource = null;
  let view = HotspotZoom.viewFor(nodes[targetKey]);
  let animationHandle = null;
  let scale = 1;
  let tx = 0;
  let ty = 0;

  function render() {
    const labels = HotspotLabels.placeLabels(
      nodes,
      targetKey,
      hoveredKey,
      scale,
    );
    for (const key of circles.keys()) {
      const label = labelEls.get(key);
      if (!label) continue;
      const placement = labels.get(key);
      // The stylesheet hides every label by default (so the file opens
      // sensibly without JS); an empty inline value would fall back to
      // that rule, so a shown label needs an explicit non-'none' value.
      // Font size is set the same way: `.hotspot-label`'s own CSS rule
      // would otherwise win over a plain attribute.
      label.style.display = placement ? 'inline' : 'none';
      if (!placement) continue;
      // `px` inside `g#map-content`'s own scaled coordinate system: a value
      // of `placement.fontSize` renders, once the group's `scale` transform
      // applies, at exactly `placement.fontSize * scale` screen pixels - the
      // fixpoint `HotspotLabels` computed it for. A bare number is not a
      // valid CSS length and the declaration is dropped silently.
      label.style.fontSize = `${placement.fontSize}px`;
      label.setAttribute('y', placement.y);
    }
  }

  function applyView() {
    scale = mapAreaSize / (2 * view.radius);
    tx = mapAreaSize / 2 - view.x * scale;
    ty = canvasHeight / 2 - view.y * scale;
    mapContent.setAttribute(
      'transform',
      `translate(${tx} ${ty}) scale(${scale})`,
    );
    render();
    updateJumpIcon();
  }

  /**
   * The jump icon at the selected leaf: shown only while it carries a jump
   * target (`config.with_jump_ids` on the server, decision 12's second
   * half), placed right after its own label (or, while that label is not
   * shown, at the circle's edge - `HotspotJumpIcon.glyphPosition`'s own
   * fallback), and kept there across a zoom.
   */
  function updateJumpIcon() {
    const targets = selectedKey === null ? null : nodes[selectedKey]?.targets;
    if (!targets || targets.length === 0) {
      jumpIcon.hide();
      return;
    }
    const node = nodes[selectedKey];
    const placement = HotspotLabels.placeLabels(
      nodes,
      targetKey,
      hoveredKey,
      scale,
    ).get(selectedKey);
    const at = HotspotJumpIcon.glyphPosition(node, placement, scale, tx, ty);
    jumpIcon.show(at, targets[0]);
  }

  /** Name every ancestor of the zoom target as a button that zooms there, the target itself as plain text. */
  function renderBreadcrumb() {
    if (!breadcrumbEl) return;
    const path = HotspotTree.ancestorPath(nodes, targetKey);
    const ancestors = path
      .slice(0, -1)
      .map(
        (key) =>
          `<button data-crumb="${escapeHtml(key)}">${escapeHtml(nodes[key].name)}</button>` +
          '<span aria-hidden="true">›</span>',
      );
    breadcrumbEl.innerHTML =
      ancestors.join('') +
      `<span aria-current="location">${escapeHtml(nodes[targetKey].name)}</span>`;
  }

  function zoomTo(key) {
    targetKey = key;
    lastLinkSource = 'zoom';
    updatePageLink();
    renderBreadcrumb();
    const from = view;
    const to = HotspotZoom.viewFor(nodes[key]);
    const startedAt = performance.now();
    if (animationHandle !== null) cancelAnimationFrame(animationHandle);
    const step = (now) => {
      const at = HotspotZoom.viewBoxAt(from, to, now - startedAt);
      view = at.view;
      applyView();
      animationHandle = at.done ? null : requestAnimationFrame(step);
    };
    animationHandle = requestAnimationFrame(step);
  }

  /** Hover always wins over a plain selection; neither wins over the other's absence. */
  function classFor(key) {
    if (key === hoveredKey) return HOVER_CLASS;
    if (key === selectedKey) return SELECTED_CLASS;
    return null;
  }

  function paintCircle(key) {
    if (key == null) return;
    const circle = circles.get(key);
    if (!circle) return;
    circle.classList.remove(HOVER_CLASS);
    circle.classList.remove(SELECTED_CLASS);
    const cls = classFor(key);
    if (cls) circle.classList.add(cls);
  }

  /**
   * `key` is the circle under the pointer, or `null` outside any circle.
   * `point`, in the outer `<svg>`'s own coordinate space, positions the
   * tooltip beside the pointer; a hover with no point (the hotspot list's
   * own rows) anchors it at the node's own drawn centre instead.
   */
  function setHover(key, point) {
    const previous = hoveredKey;
    hoveredKey = key;
    paintCircle(previous);
    paintCircle(hoveredKey);
    if (key && circles.has(key)) {
      const node = nodes[key];
      const at = point ?? { x: tx + node.cx * scale, y: ty + node.cy * scale };
      tooltip.show(at, HotspotHover.tooltipRows(nodes, key));
    } else {
      tooltip.hide();
    }
    render();
  }

  function renderDetails() {
    if (!detailsEl) return;
    if (selectedKey === null) {
      detailsEl.innerHTML = '';
      return;
    }
    const { title, rows } = HotspotHover.tooltipRows(nodes, selectedKey);
    detailsEl.innerHTML =
      `<div class="${STATIC_DATA.classes.detailsTitle}">${escapeHtml(title)}</div><table>` +
      rows
        .map(
          ([label, value]) =>
            `<tr><td>${escapeHtml(label)}</td><td>${escapeHtml(value)}</td></tr>`,
        )
        .join('') +
      '</table>';
  }

  /**
   * The cross-page link: a selected leaf carries its file, a zoomed-into
   * container carries its, and whichever of the two happened last wins -
   * that is where the user last put their attention. A zoom to the root is
   * the absence of a zoomed container, not a cancelled selection, so it
   * falls back to a still-selected leaf's file instead of carrying nothing.
   */
  function updatePageLink() {
    if (!pageLinkEl) return;
    let file = null;
    if (lastLinkSource === 'select' && selectedKey !== null) {
      file = nodes[selectedKey].file;
    } else if (lastLinkSource === 'zoom' && targetKey !== rootKey) {
      file = nodes[targetKey].file;
    } else if (selectedKey !== null) {
      file = nodes[selectedKey].file;
    }
    pageLinkEl.setAttribute('href', PageLink.buildLink('/', file));
  }

  /** Selects `key`; driving the sidebar is this function's whole job, no jump on click. */
  function select(key) {
    const previous = selectedKey;
    selectedKey = key;
    lastLinkSource = 'select';
    paintCircle(previous);
    paintCircle(selectedKey);
    renderDetails();
    updatePageLink();
    updateJumpIcon();
  }

  /**
   * Selects a leaf and zooms to its parent circle, so it sits among its
   * siblings. Zooms first, so the leaf's own selection - not the parent
   * it's framed against - is what the cross-page link ends up carrying.
   */
  function focus(key) {
    zoomTo(nodes[key]?.parent ?? HotspotTree.rootKey(nodes));
    select(key);
  }

  /** An editor follow event naming a workspace-relative file: it arrives
   * with no idea where the map is already looking, so a leaf keeps zooming
   * to its parent, same as always. */
  function focusFile(file) {
    const key = HotspotSelection.leafKeyForFile(nodes, file);
    if (key === null) return;
    focus(key);
  }

  /** Show the file `?select=` names: zoom into a container, focus a leaf. */
  function applyIncomingSelect(file) {
    const place = HotspotSelection.placeForFile(nodes, file);
    if (!place) return;
    if (place.type === 'zoom') zoomTo(place.key);
    else focus(place.key);
  }

  function barsHtml() {
    const bars = HotspotBars.hotspotBars(nodes, STATIC_DATA.hotspots);
    const c = STATIC_DATA.classes;
    return bars
      .map(
        (bar) =>
          `<li class="${LIST_ITEM_CLASS}" data-file="${escapeHtml(bar.key)}">` +
          `<span class="${c.barLabel}" data-full="${escapeHtml(bar.file)}">${escapeHtml(bar.file)}</span>` +
          `<span class="${c.barTrack}" style="display:block;height:8px">` +
          `<span class="${c.barFill}" style="display:block;height:100%;` +
          `width:${bar.widthPercent.toFixed(1)}%;background:` +
          `color-mix(in srgb, var(--arc-hotspot-hot) ${bar.fillPercent}%, var(--arc-hotspot-cold))">` +
          '</span></span></li>',
      )
      .join('');
  }

  let showingBars = false;

  function showView(bars) {
    showingBars = bars;
    showListEl.setAttribute('aria-pressed', String(!bars));
    showBarsEl.setAttribute('aria-pressed', String(bars));
    listEl.innerHTML = bars ? barsHtml() : originalListHtml;
    fitSidebarPaths(sidebarEl);
  }

  if (showListEl && showBarsEl && listEl) {
    showListEl.addEventListener('click', () => showView(false));
    showBarsEl.addEventListener('click', () => showView(true));
  }

  if (listEl) {
    // Both events bubble past the foreignObject into the svg root, which
    // has its own click/pointerover listeners for the circles; stopped
    // here so a list row's own handling is not immediately undone by them.
    listEl.addEventListener('click', (event) => {
      event.stopPropagation?.();
      const key = rowKeyAt(event);
      if (key && nodes[key]) focus(key);
    });
    listEl.addEventListener('pointerover', (event) => {
      event.stopPropagation?.();
      setHover(rowKeyAt(event));
    });
    listEl.addEventListener('pointerleave', () => setHover(null));
  }

  if (breadcrumbEl) {
    // Stopped for the same reason as the list's own click: the svg root's
    // handler would read a click outside any circle as "zoom out".
    breadcrumbEl.addEventListener('click', (event) => {
      event.stopPropagation?.();
      const key = crumbKeyAt(event);
      if (key && nodes[key]) zoomTo(key);
    });
  }

  /** A pointer event's client position, converted into the outer `<svg>`'s
   * own coordinate space (the one `node.cx`/`cy` and `canvasWidth` share). */
  function pointerToSvg(event) {
    const rect = svg.getBoundingClientRect();
    const vb = svg.viewBox.baseVal;
    return {
      x: ((event.clientX - rect.left) * vb.width) / rect.width,
      y: ((event.clientY - rect.top) * vb.height) / rect.height,
    };
  }

  svg.addEventListener('click', (event) => {
    const clickedKey = circleKeyAt(event);
    const action = HotspotSelection.clickAction(
      nodes,
      targetKey,
      clickedKey,
      selectedKey,
    );
    if (action.type === 'select') select(action.key);
    else if (action.type === 'zoom') zoomTo(action.key);
  });
  svg.addEventListener('pointerover', (event) =>
    setHover(circleKeyAt(event), pointerToSvg(event)),
  );
  svg.addEventListener('pointerleave', () => setHover(null));

  // === View kept across the reload after a recomputation ===
  // The page stores the zoom target, the selection and the list view under
  // this key before it reloads, and applies them once when it loads again.
  const VIEW_KEY = 'cargo-arc-hotspot-view';

  function readStoredView() {
    try {
      const stored = sessionStorage.getItem(VIEW_KEY);
      if (!stored) return null;
      sessionStorage.removeItem(VIEW_KEY);
      return JSON.parse(stored);
    } catch {
      return null;
    }
  }

  function storeViewAndReload() {
    try {
      sessionStorage.setItem(
        VIEW_KEY,
        JSON.stringify({
          target: targetKey,
          selected: selectedKey,
          bars: showingBars,
        }),
      );
    } catch {
      // The view is lost; the page still shows the new run.
    }
    location.reload();
  }

  /** A key the new run no longer has is dropped. */
  function applyStoredView(stored) {
    if (typeof stored.target === 'string' && nodes[stored.target]) {
      targetKey = stored.target;
      view = HotspotZoom.viewFor(nodes[targetKey]);
      lastLinkSource = 'zoom';
    }
    if (typeof stored.selected === 'string' && nodes[stored.selected]) {
      select(stored.selected);
    }
    if (stored.bars === true && showListEl && showBarsEl && listEl) {
      showView(true);
    }
  }

  // The follow and on-save toggles are rendered only for a map served by
  // `cargo arc ui`; a written file has no event stream to open.
  const followEl = DomAdapter.getElementById('follow-toggle');
  if (followEl) {
    const onSaveEl = DomAdapter.getElementById('on-save-toggle');
    const onSave = OnSaveToggle.createOnSaveToggle({
      post: (line) => fetch('command', { method: 'POST', body: line }),
      showState: (on) => onSaveEl?.setAttribute('aria-pressed', String(on)),
      showStatus: showJumpStatus,
      isOn: () => onSaveEl?.getAttribute('aria-pressed') === 'true',
    });
    onSaveEl?.addEventListener('click', (event) => {
      event.stopPropagation?.();
      onSave.click();
    });
    const follow = Follow.createFollow({
      // One stream serves them all: the theme event goes to the theme
      // control, a finished run reloads the page, a failed one shows its
      // error, and the follow module and the on-save button take theirs.
      connect: (handler) =>
        Follow.connectEventSource((name, data) => {
          if (name === 'theme') themeControl?.handleEditorMode(data);
          else if (name === 'analysis') storeViewAndReload();
          else if (name === 'analysis-error') showJumpStatus(data);
          else {
            handler(name, data);
            onSave.handleEvent(name, data);
          }
        }),
      apply: (_node, _jumps, file) => {
        if (typeof file === 'string') focusFile(file);
      },
      showState: (on) => followEl.setAttribute('aria-pressed', String(on)),
    });
    followEl.addEventListener('click', (event) => {
      event.stopPropagation?.();
      follow.setEnabled(!follow.isEnabled());
    });
    follow.start();
  }

  const storedView = readStoredView();
  if (storedView) applyStoredView(storedView);
  applyView();
  renderDetails();
  updatePageLink();
  renderBreadcrumb();

  // A reload keeps `?select=` in the address; the stored view is newer.
  if (!storedView && typeof location !== 'undefined') {
    const file = PageLink.parseSelect(location.search);
    if (file) applyIncomingSelect(file);
  }

  /**
   * Recomputes the layout for a `width` x `height` box and applies it: the
   * viewBox, the toolbar and sidebar rects, the tooltip's clamp width, and
   * the map's own zoom scale. `view` (the current target's pan and zoom) is
   * left untouched, so a resize does not snap the map back to the root.
   */
  function resize(width, height) {
    layout = layoutFor({ width, height });
    canvasHeight = layout.viewBox.height;
    mapAreaSize = layout.mapAreaSize;
    tooltip.setCanvasWidth(layout.viewBox.width);
    applyView();
  }

  // Reacts to the svg's own box changing size, not the window: the served
  // page can size that box to a different aspect ratio than the window
  // (`render::html_page`'s doc comment), so a window resize event is
  // neither necessary nor sufficient here.
  if (typeof ResizeObserver !== 'undefined') {
    new ResizeObserver(() => {
      const rect = svg.getBoundingClientRect();
      resize(rect.width, rect.height);
    }).observe(svg);
  }

  return { zoomTo, setHover, select, focus, resize };
}

if (typeof module !== 'undefined') {
  module.exports = { bootstrapHotspotPage, buildHotspotMap };
}

// Only runs in a real browser, not under the test runner.
if (typeof document !== 'undefined') {
  const themeControl = bootstrapHotspotPage();
  buildHotspotMap(themeControl);
}
