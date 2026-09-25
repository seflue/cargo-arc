// @module SvgScript
// @deps ArcLogic, StaticData, AppState, Selectors, DomAdapter, LayerManager, TreeLogic, DerivedState, HighlightRenderer, VirtualEdgeLogic, TextMeasure, SidebarLogic, SearchLogic, Jump, JumpIcons, Follow, Theme, SwitchToggles, OnSaveToggle, ViewSnapshot, PageLink, CanvasSize
// @config ROW_HEIGHT, MARGIN, TOOLBAR_HEIGHT, SIDEBAR_SHADOW_PAD
// svg_script.js - DOM code for interactive SVG
// ArcLogic is loaded from arc_logic.js before this file
// Placeholders replaced at runtime: __ROW_HEIGHT__, __MARGIN__, __TOOLBAR_HEIGHT__

function createHighlightDebouncer(renderFn, delay) {
  let timer = null;
  return {
    debounced() {
      clearTimeout(timer);
      timer = setTimeout(renderFn, delay);
    },
    immediate() {
      clearTimeout(timer);
      renderFn();
    },
  };
}

function createPinnedSidebarRefresher(getPinned, collectRelations, showNode) {
  return function refreshPinnedSidebar() {
    const pinned = getPinned();
    if (!pinned || pinned.type !== 'node') return;
    const relations = collectRelations(pinned.id);
    showNode(pinned.id, relations);
  };
}

function deriveHoverKey(type, id, clusterMode, sccId) {
  if (type === 'arc' && clusterMode && sccId != null) return `scc:${sccId}`;
  return `${type}:${id}`;
}

function createHoverKeyTracker() {
  let hoverKey = null;
  return {
    // true = new hover identity (caller should render); false = same as current (no-op).
    enter(key) {
      if (key === hoverKey) return false;
      hoverKey = key;
      return true;
    },
    reset() {
      hoverKey = null;
    },
  };
}

// Resolve a jump id from a sidebar click: the closest location row or
// definition chip carrying data-jump, or null when the click landed elsewhere.
function jumpIdFromClick(target) {
  const el = target.closest(
    '.sidebar-location[data-jump], .sidebar-definition[data-jump]',
  );
  return el ? Number(el.dataset.jump) : null;
}

// The vertical middle of the span covered by the given rects, in their own
// coordinates; null without rects.
/** @param {{ y: number, height: number }[]} rects */
function spanCenter(rects) {
  if (rects.length === 0) return null;
  const top = Math.min(...rects.map((r) => r.y));
  const bottom = Math.max(...rects.map((r) => r.y + r.height));
  return (top + bottom) / 2;
}

if (typeof module !== 'undefined') {
  module.exports = {
    createHighlightDebouncer,
    createPinnedSidebarRefresher,
    deriveHoverKey,
    createHoverKeyTracker,
    jumpIdFromClick,
    spanCenter,
  };
}

// IIFE for SVG embedding (DOM-code) - only runs in browser with placeholders replaced
if (typeof document !== 'undefined') {
  (() => {
    const ROW_HEIGHT = __ROW_HEIGHT__;
    const MARGIN = __MARGIN__;
    const TOOLBAR_HEIGHT = __TOOLBAR_HEIGHT__;
    const SIDEBAR_SHADOW_PAD =
      typeof __SIDEBAR_SHADOW_PAD__ !== 'undefined'
        ? __SIDEBAR_SHADOW_PAD__
        : 12;
    const TOGGLE_OFFSET = 14;
    const C = STATIC_DATA.classes;

    // === Arc weight scaling ===

    function applyInitialArcWeights() {
      for (const arcId of StaticData.getAllArcIds()) {
        const width = StaticData.getArcStrokeWidth(arcId);
        const visibleArc = DomAdapter.getVisibleArc(arcId);
        if (visibleArc) visibleArc.style.strokeWidth = `${width}px`;
        ArcLogic.scaleArrow(DomAdapter, arcId, width);
      }
    }

    // Runtime map for virtual arc usages (structured objects, no DOM serialization)
    const virtualArcUsages = new Map();
    const virtualArcOriginals = new Map();

    // === Highlight functionality ===
    // Use AppState module for unified state management
    const appState = AppState.create();

    // The toolbar's link to the hotspot map, carrying the current node
    // selection as `?select=<file>` - an arc selection or no selection
    // carries nothing, since only a node resolves to one file.
    const hotspotLinkEl = DomAdapter.getElementById('hotspot-page-link');
    function updateHotspotLink() {
      if (!hotspotLinkEl) return;
      const selection = appState.clickSelection;
      const file =
        selection?.type === 'node'
          ? (StaticData.getNode(selection.id)?.file ?? null)
          : null;
      hotspotLinkEl.setAttribute('href', PageLink.buildLink('/hotspots', file));
    }

    // === View kept across the reload after a recomputation ===
    // The page stores a snapshot under this key before it reloads and
    // applies it once, at the end of this initialisation.
    const VIEW_KEY = 'cargo-arc-view';
    const restoredView = readStoredView();

    function readStoredView() {
      try {
        const stored = sessionStorage.getItem(VIEW_KEY);
        if (!stored) return null;
        sessionStorage.removeItem(VIEW_KEY);
        return ViewSnapshot.restore(JSON.parse(stored), STATIC_DATA.nodes);
      } catch {
        return null;
      }
    }

    // === Initial expand-level: populate collapsed state from Rust-rendered view ===
    const expandLevel = StaticData.getExpandLevel();
    if (expandLevel !== null) {
      for (const nodeId of StaticData.getAllNodeIds()) {
        const nesting = StaticData.getNodeNesting(nodeId);
        if (
          StaticData.hasChildren(nodeId) &&
          nesting !== undefined &&
          nesting >= expandLevel
        ) {
          AppState.setCollapsed(appState, nodeId, true);
        }
      }
    }

    /**
     * Central highlight rerender: derive state from AppState, apply via HighlightRenderer.
     * Single entry point for all highlight updates (click, hover, collapse, filter toggle).
     */
    function rerenderHighlights() {
      const widthOverrides = new Map();
      for (const nodeId of appState.collapsed) {
        const rect = DomAdapter.getNode(nodeId);
        if (rect) {
          widthOverrides.set(nodeId, parseFloat(rect.getAttribute('width')));
        }
      }
      const hiddenByFilter = getFilterHiddenNodeIds();
      const positions = DerivedState.computeCurrentPositions(
        appState.collapsed,
        StaticData,
        MARGIN,
        TOOLBAR_HEIGHT,
        ROW_HEIGHT,
        widthOverrides,
        hiddenByFilter,
      );
      const state = DerivedState.deriveHighlightState(
        appState,
        StaticData,
        virtualArcUsages,
        appState.hiddenArcIds,
        positions,
        ROW_HEIGHT,
      );
      HighlightRenderer.apply(DomAdapter, StaticData, virtualArcUsages, state);
      updateHotspotLink();
    }

    const highlightTiming = createHighlightDebouncer(rerenderHighlights, 30);

    const hoverTracker = createHoverKeyTracker();
    let hideGraceTimer = null;
    const HOVER_HIDE_GRACE = 90; // ms; small fixed value, browser-verified

    // Shared toggle core for all clickable elements (edge, node, virtual edge).
    // The showSidebar callback provides the type-specific sidebar display logic.
    function toggleHighlight(type, id, showSidebar) {
      const isPinned = AppState.toggleSelection(appState, type, id);
      highlightTiming.immediate();
      if (!isPinned) {
        SidebarLogic.hide();
        return;
      }
      showSidebar();
    }

    // Cycle-aware click handling for edges (real or virtual), routed through
    // the SCC state machine (AppState.clickEdge). Non-cycle edges keep the
    // old pin-shows/unpin-hides contract; cycle edges are governed by
    // selectedScc + inner-edge pin, so we just re-render and show whichever
    // edge/cluster the state now resolves to.
    function handleEdgeClick(edgeId, showPinnedSidebar) {
      const sccId = StaticData.getArc(edgeId)?.sccId;
      AppState.clickEdge(appState, edgeId, sccId);
      highlightTiming.immediate();
      if (sccId == null) {
        if (AppState.isSelected(appState, 'arc', edgeId)) {
          showPinnedSidebar();
        } else {
          SidebarLogic.hide();
        }
        return;
      }
      SidebarLogic.show(edgeId);
    }

    function highlightEdge(from, to) {
      const edgeId = `${from}-${to}`;
      handleEdgeClick(edgeId, () => SidebarLogic.show(edgeId));
    }

    function highlightNode(nodeId) {
      toggleHighlight('node', nodeId, () => {
        const relations = collectNodeRelations(nodeId);
        SidebarLogic.showNode(nodeId, relations);
      });
    }

    // Expands every collapsed ancestor of the node, so the node itself is
    // on screen.
    function expandTo(nodeId) {
      for (
        let ancestor = parentMap.get(nodeId);
        ancestor !== undefined;
        ancestor = parentMap.get(ancestor)
      ) {
        if (AppState.isCollapsed(appState, ancestor)) toggleCollapse(ancestor);
      }
    }

    // The editor's cursor moved into a node: select it as a click would,
    // without the click's toggle, and bring it and its relations into view.
    // A node already selected keeps the view where the user left it; only
    // the sidebar rows of the cursor line open.
    function focusNode(nodeId, jumps) {
      if (!DomAdapter.getNode(nodeId)) return;
      if (!AppState.isSelected(appState, 'node', nodeId)) {
        expandTo(nodeId);
        AppState.setSelection(appState, 'node', nodeId);
        highlightTiming.immediate();
        const relations = collectNodeRelations(nodeId);
        SidebarLogic.showNode(nodeId, relations);
        scrollToSpan([
          nodeId,
          ...relations.incoming.map((r) => r.targetId),
          ...relations.outgoing.map((r) => r.targetId),
        ]);
      }
      SidebarLogic.expandLocations(jumps);
    }

    function collectNodeRelations(nodeId) {
      const base = StaticData.getNodeRelations(nodeId);
      // Filter base arcs to hidden nodes — virtual arcs already represent them
      base.outgoing = base.outgoing.filter(
        (e) => getVisibleAncestor(e.targetId) === e.targetId,
      );
      base.incoming = base.incoming.filter(
        (e) => getVisibleAncestor(e.targetId) === e.targetId,
      );
      for (const [key, usages] of virtualArcUsages) {
        const [fromId, toId] = key.split('-');
        const origArcs = virtualArcOriginals.get(key) || [];
        const weight = usages.reduce((s, g) => s + g.locations.length, 0);
        const merged = SidebarLogic.mergeSymbolGroups(usages);
        const entry = {
          targetId: fromId === nodeId ? toId : fromId,
          weight,
          usages: merged,
          arcId: key,
          originalArcs: origArcs,
        };
        if (fromId === nodeId) base.outgoing.push(entry);
        else if (toId === nodeId) base.incoming.push(entry);
      }
      const byTreeOrder = (a, b) => {
        const yA = StaticData.getOriginalPosition(a.targetId)?.y ?? Infinity;
        const yB = StaticData.getOriginalPosition(b.targetId)?.y ?? Infinity;
        return yA - yB;
      };
      base.outgoing.sort(byTreeOrder);
      base.incoming.sort(byTreeOrder);
      return base;
    }

    const refreshPinnedSidebar = createPinnedSidebarRefresher(
      () => AppState.getPinned(appState),
      collectNodeRelations,
      (id, rels) => SidebarLogic.showNode(id, rels),
    );

    let _isNavigating = false;

    function scrollToNode(nodeId) {
      scrollToSpan([nodeId]);
    }

    // Scrolls the window so the middle of the span the nodes cover sits at
    // the middle of the viewport, as far as the page allows.
    function scrollToSpan(nodeIds) {
      const rects = nodeIds
        .map((id) => DomAdapter.getNode(id))
        .filter((rect) => rect !== null)
        .map((rect) => ({
          y: parseFloat(rect.getAttribute('y')),
          height: parseFloat(rect.getAttribute('height')),
        }));
      const centerSvg = spanCenter(rects);
      if (centerSvg === null) return;
      const svg = DomAdapter.getSvgRoot();
      if (!svg) return;
      const svgRect = svg.getBoundingClientRect();
      const vb = svg.viewBox.baseVal;
      const scaleY = vb.height / svgRect.height;
      const centerPage = centerSvg / scaleY + svgRect.top + window.scrollY;
      const targetScroll = centerPage - window.innerHeight / 2;
      const maxScroll =
        document.documentElement.scrollHeight - window.innerHeight;
      const clampedTarget = Math.max(0, Math.min(targetScroll, maxScroll));
      const scrollDistance = Math.abs(clampedTarget - window.scrollY);
      const isLargeScroll = scrollDistance >= window.innerHeight / 4;

      if (isLargeScroll) {
        _isNavigating = true;
        const sidebarEl = SidebarLogic._getElement();
        if (sidebarEl) sidebarEl.style.visibility = 'hidden';
        function finishNavigation() {
          _isNavigating = false;
          if (sidebarEl) sidebarEl.style.visibility = '';
          if (SidebarLogic.isVisible()) SidebarLogic.updatePosition();
        }
        window.addEventListener(
          'scrollend',
          () => {
            clearTimeout(navTimeout);
            finishNavigation();
          },
          { once: true },
        );
        const navTimeout = setTimeout(() => finishNavigation(), 600);
      }

      window.scrollTo({ top: clampedTarget, behavior: 'smooth' });
    }

    /** @type {{ _onBadgeClick: ((nodeId: any) => void) | null }} */ (
      SidebarLogic
    )._onBadgeClick = (nodeId) => scrollToNode(nodeId);
    /** @type {{ _onCollapseToggle: ((nodeId: any) => void) | null }} */ (
      SidebarLogic
    )._onCollapseToggle = (nodeId) => {
      toggleCollapse(nodeId);
      updateToolbarPosition();
    };
    /** @type {{ _isNodeCollapsed: ((nodeId: any) => boolean) | null }} */ (
      SidebarLogic
    )._isNodeCollapsed = (nodeId) => AppState.isCollapsed(appState, nodeId);
    /** @type {{ _isClusterMode: (() => boolean) | null }} */ (
      SidebarLogic
    )._isClusterMode = () => AppState.isClusterMode(appState);
    // Resolved inner-edge focus (pin-or-hover, restricted to the selected SCC),
    // matching derived_state's isFocusArc. The sidebar marks a row focused from
    // this, not from whichever edge opened the cluster view.
    /** @type {{ _resolvedFocusArc: (() => string | null) | null }} */ (
      SidebarLogic
    )._resolvedFocusArc = () => {
      const sel = AppState.getSelection(appState);
      const scc = AppState.getSelectedScc(appState);
      if (
        sel.type !== 'arc' ||
        sel.id == null ||
        !AppState.isClusterMode(appState) ||
        scc == null
      ) {
        return null;
      }
      return StaticData.getArc(sel.id)?.sccId === scc ? sel.id : null;
    };
    // Sidebar cluster row hover focuses the corresponding graph edge (mirror
    // of handleMouseEnter/handleMouseLeave's pin-beats-hover gate).
    /** @type {{ _onEdgeHover: ((arcId: any) => void) | null }} */ (
      SidebarLogic
    )._onEdgeHover = (arcId) => {
      if (AppState.hasPinnedSelection(appState)) return;
      AppState.setHover(appState, 'arc', arcId);
      highlightTiming.debounced();
    };
    /** @type {{ _onEdgeHoverEnd: (() => void) | null }} */ (
      SidebarLogic
    )._onEdgeHoverEnd = () => {
      if (AppState.hasPinnedSelection(appState)) return;
      AppState.clearHover(appState);
      highlightTiming.debounced();
    };
    // Sidebar cluster row click: pin couples to expansion (AppState.clickCluster-
    // Row decides). Returns the new expand state so the sidebar syncs the row.
    /** @type {{ _onEdgeClick: ((arcId: string, expandable: boolean, expanded: boolean) => boolean) | null }} */ (
      SidebarLogic
    )._onEdgeClick = (arcId, expandable, expanded) => {
      const before = AppState.isSelected(appState, 'arc', arcId);
      const sccId = StaticData.getArc(arcId)?.sccId;
      const endExpanded = AppState.clickClusterRow(
        appState,
        arcId,
        sccId,
        expandable,
        expanded,
      );
      // Pure re-expand (collapsed+pinned) leaves the selection untouched; only
      // re-render the graph when the pin actually changed.
      if (AppState.isSelected(appState, 'arc', arcId) !== before) {
        highlightTiming.immediate();
      }
      return endExpanded;
    };

    // Cancels a pending hide and updates hoverKey if the derived identity
    // changed. Returns false when the hover is a same-cluster/element no-op
    // (caller should bail without touching AppState/sidebar).
    function enterHover(type, id, sccId) {
      clearTimeout(hideGraceTimer);
      const key = deriveHoverKey(
        type,
        id,
        AppState.isClusterMode(appState),
        sccId,
      );
      return hoverTracker.enter(key);
    }

    // A selected SCC keeps the cluster sidebar pinned open even without an
    // inner-edge pin: hover then only moves the focused row, it neither shows a
    // transient sidebar nor hides it on leave.
    function clusterActive() {
      return (
        AppState.isClusterMode(appState) &&
        AppState.getSelectedScc(appState) != null
      );
    }

    function handleMouseEnter(type, id) {
      if (AppState.hasPinnedSelection(appState)) {
        return;
      }
      const sccId = type === 'arc' ? StaticData.getArc(id)?.sccId : null;
      if (!enterHover(type, id, sccId)) return;
      AppState.setHover(appState, type, id);
      highlightTiming.debounced();
      if (clusterActive()) {
        SidebarLogic.refreshClusterFocus();
      } else if (type === 'node') {
        const relations = collectNodeRelations(id);
        SidebarLogic.showTransientNode(id, relations);
      } else if (type === 'arc') {
        SidebarLogic.showTransient(id);
      }
    }

    function handleMouseLeave() {
      if (AppState.hasPinnedSelection(appState)) {
        return;
      }
      clearTimeout(hideGraceTimer);
      hideGraceTimer = setTimeout(() => {
        AppState.clearHover(appState);
        hoverTracker.reset();
        highlightTiming.debounced();
        if (clusterActive()) {
          SidebarLogic.refreshClusterFocus();
        } else {
          SidebarLogic.hideTransient();
        }
      }, HOVER_HIDE_GRACE);
    }

    function handleVirtualMouseEnter(arcId, fromId, toId) {
      if (AppState.hasPinnedSelection(appState)) return;
      if (!enterHover('arc', arcId, null)) return;
      AppState.setHover(appState, 'arc', arcId);
      highlightTiming.debounced();
      if (clusterActive()) {
        SidebarLogic.refreshClusterFocus();
        return;
      }
      const usages = virtualArcUsages.get(arcId) || [];
      const originalArcs = virtualArcOriginals.get(arcId) || [];
      const mergedUsages = SidebarLogic.mergeSymbolGroups(usages);
      SidebarLogic.showTransient(arcId, {
        from: fromId,
        to: toId,
        usages: mergedUsages,
        originalArcs,
      });
    }

    // === Collapse functionality ===
    // Build parentMap from STATIC_DATA (no DOM read needed)
    const parentMap = StaticData.buildParentMap();

    // Note: Original positions now come from STATIC_DATA, no need to store them

    // Wrapper functions for TreeLogic (use parentMap from closure)
    function getDescendants(nodeId) {
      return TreeLogic.getDescendants(nodeId, parentMap);
    }

    function getVisibleAncestor(nodeId) {
      return TreeLogic.getVisibleAncestor(
        nodeId,
        appState.collapsed,
        parentMap,
      );
    }

    function countDescendants(nodeId) {
      return TreeLogic.countDescendants(nodeId, parentMap);
    }

    // Update tree lines for a node at new Y position
    function updateTreeLines(nodeId, newY, nodeHeight) {
      // Update lines where this node is the child
      DomAdapter.getTreeLines(nodeId, 'child').forEach((line) => {
        const midY = newY + nodeHeight / 2;
        if (line.getAttribute('x1') === line.getAttribute('x2')) {
          // Vertical line - update y2
          line.setAttribute('y2', midY);
        } else {
          // Horizontal line - update both y1 and y2
          line.setAttribute('y1', midY);
          line.setAttribute('y2', midY);
        }
      });

      // Update lines where this node is the parent (vertical line y1)
      DomAdapter.getTreeLines(nodeId, 'parent').forEach((line) => {
        if (line.getAttribute('x1') === line.getAttribute('x2')) {
          // Vertical line - update y1 (parent bottom)
          line.setAttribute('y1', newY + nodeHeight);
        }
      });
    }

    // Update SVG viewport dimensions to fit visible content.
    // Mirrors the Rust calculate_canvas_size formula so expand/collapse
    // keeps the SVG large enough for all visible nodes and arcs.
    function updateSvgViewport(currentY, visiblePositionY) {
      const svg = DomAdapter.getSvgRoot();
      if (!svg) return;

      const neededHeight = currentY + MARGIN + SIDEBAR_SHADOW_PAD;

      // Width: maxNodeRight + arcSpace + sidebarSpace + margin
      let maxNodeRight = 0;
      for (const nodeId of visiblePositionY.keys()) {
        const orig = StaticData.getOriginalPosition(nodeId);
        if (!orig) continue;
        maxNodeRight = Math.max(maxNodeRight, orig.x + orig.width);
      }

      let maxArcWidth = 50; // arc_min_space
      for (const arcId of StaticData.getAllArcIds()) {
        const arc = StaticData.getArc(arcId);
        if (!arc) continue;
        const fromY = visiblePositionY.get(arc.from);
        const toY = visiblePositionY.get(arc.to);
        if (fromY === undefined || toY === undefined) continue;
        const rowsSpanned = Math.max(
          1,
          Math.round(Math.abs(toY - fromY) / ROW_HEIGHT),
        );
        // arc_base (20) + rowsSpanned * arc_scale (15) + arrow_length (8)
        maxArcWidth = Math.max(maxArcWidth, 20 + rowsSpanned * 15 + 8);
      }

      const sidebarSpace = 280 + SIDEBAR_SHADOW_PAD;
      const neededWidth =
        maxNodeRight + Math.max(maxArcWidth, 50) + sidebarSpace + MARGIN;

      contentSize = { width: neededWidth, height: neededHeight };
      applyCanvasSize();
    }

    // The diagram's own size, from the render until the first relayout.
    const renderedViewBox = DomAdapter.getSvgRoot()?.viewBox.baseVal;
    let contentSize = {
      width: renderedViewBox?.width ?? 0,
      height: renderedViewBox?.height ?? 0,
    };
    // The toolbar's measured height; more than TOOLBAR_HEIGHT once it wraps.
    let toolbarHeight = TOOLBAR_HEIGHT;

    function applyCanvasSize() {
      const svg = DomAdapter.getSvgRoot();
      if (!svg) return;
      const vb = svg.viewBox.baseVal;
      // Height first: it decides whether the page gets a vertical
      // scrollbar, which narrows the visible width read below.
      const height = CanvasSize.svgHeight(
        contentSize.height,
        toolbarHeight - TOOLBAR_HEIGHT,
      );
      vb.height = height;
      svg.setAttribute('height', String(height));
      const width = CanvasSize.svgWidth(
        contentSize.width,
        CanvasSize.visibleArea().width,
      );
      vb.width = width;
      svg.setAttribute('width', String(width));
      placeToolbar();

      // Base SVG size changed — sidebar must recapture on next show/update
      SidebarLogic.resetStoredViewBox();
    }

    // Relayout visible nodes
    function relayout() {
      let currentY = MARGIN + TOOLBAR_HEIGHT;
      const visiblePositionY = new Map();

      // Get all node IDs sorted by original Y position (no DOM query for list)
      const sortedIds = StaticData.getAllNodeIds().sort((a, b) => {
        const posA = StaticData.getOriginalPosition(a);
        const posB = StaticData.getOriginalPosition(b);
        return (posA?.y ?? 0) - (posB?.y ?? 0);
      });

      sortedIds.forEach((nodeId) => {
        const node = DomAdapter.getNode(nodeId);
        if (!node) return;
        if (node.classList.contains(C.collapsed)) return;
        if (node.classList.contains(C.hiddenByFilter)) return;

        // Get height from StaticData (no DOM read)
        const origPos = StaticData.getOriginalPosition(nodeId);
        if (!origPos) return;
        const height = origPos.height;

        // Track position for viewport calculation
        visiblePositionY.set(nodeId, currentY);

        // Update rect position
        node.setAttribute('y', currentY);

        // Update label position (next text sibling)
        const label = node.nextElementSibling;
        if (
          label &&
          label.tagName === 'text' &&
          label.classList.contains(C.label)
        ) {
          label.setAttribute('y', currentY + height / 2 + 4);
        }

        // Update toggle icon position (if exists)
        const toggle = DomAdapter.getCollapseToggle(nodeId);
        if (toggle) {
          toggle.setAttribute('y', currentY + height / 2 + 4);
          const nodeX = parseFloat(node.getAttribute('x'));
          const nodeW = parseFloat(node.getAttribute('width'));
          toggle.setAttribute('x', nodeX + nodeW - TOGGLE_OFFSET);
        }

        // Update tree lines
        updateTreeLines(nodeId, currentY, height);

        currentY += ROW_HEIGHT;
      });

      recalculateVirtualEdges();

      // Node positions just changed; a group built from the old rect would
      // point at the wrong spot.
      jumpIcons.hide();

      // Resize SVG to fit visible content
      updateSvgViewport(currentY, visiblePositionY);

      // Re-apply highlights after edges were recreated
      highlightTiming.immediate();

      // Invalidate sidebar layout cache after arc positions changed
      SidebarLogic.invalidateLayout();
      if (SidebarLogic.isVisible()) SidebarLogic.updatePosition();
    }

    // Helper: Extract edge data from STATIC_DATA to pure objects
    // Single code path for all scenarios (with/without expand-level, interactive collapse)
    function extractEdgeData(visibleNodes) {
      const edges = [];
      for (const arcId of StaticData.getAllArcIds()) {
        const arc = StaticData.getArc(arcId);
        if (!arc) continue;
        const fromId = arc.from;
        const toId = arc.to;
        edges.push({
          hitarea: DomAdapter.getHitarea(arcId),
          arcId,
          fromId,
          toId,
          fromHidden: !visibleNodes.has(fromId),
          toHidden: !visibleNodes.has(toId),
          sourceLocations: StaticData.getArcUsages(arcId),
          direction: DerivedState._determineDirection(fromId, toId, parentMap),
        });
      }
      return edges;
    }

    // Remove virtual elements and reset original edge display
    function cleanupVirtualElements() {
      DomAdapter.querySelectorAll(Selectors.allVirtualElements()).forEach(
        (el) => {
          el.remove();
        },
      );
      DomAdapter.querySelectorAll(Selectors.allBaseEdges()).forEach((edge) => {
        edge.style.display = '';
      });
      DomAdapter.querySelectorAll(Selectors.allBaseArrows()).forEach(
        (arrow) => {
          arrow.style.display = '';
        },
      );
      // Evict virtual arc cache entries before clearing usage maps
      for (const arcId of virtualArcUsages.keys()) {
        DomAdapter.evictArcCache(arcId);
      }
      virtualArcUsages.clear();
      virtualArcOriginals.clear();
    }

    // Hide original elements when from/to hidden, update visible arc paths
    function updateOriginalEdges(edgeData, currentPositions, maxRight) {
      edgeData.forEach((edge) => {
        const { hitarea, arcId, fromId, toId, fromHidden, toHidden } = edge;

        if (window.DEBUG_ARCS) {
          console.log(
            `Arc ${arcId}: from=${fromId}(${fromHidden ? 'hidden' : 'visible'}), to=${toId}(${toHidden ? 'hidden' : 'visible'})`,
          );
        }

        if (fromHidden || toHidden) {
          // Hide original elements (hitarea may be null for expand-level hidden arcs).
          // The collapsed class mirrors the inline display so a Rust-baked or
          // previously-set class never outlives the state it was set for.
          if (hitarea) {
            hitarea.style.display = 'none';
            hitarea.classList.add(C.collapsed);
          }
          const visibleArc = DomAdapter.getVisibleArc(arcId);
          if (visibleArc) {
            visibleArc.style.display = 'none';
            visibleArc.classList.add(C.collapsed);
          }
          DomAdapter.getArrows(`${fromId}-${toId}`).forEach((arr) => {
            arr.style.display = 'none';
            arr.classList.add(C.collapsed);
          });
        } else {
          // Update visible arc paths using computed positions (no DOM read)
          const fromPos = currentPositions.get(fromId);
          const toPos = currentPositions.get(toId);
          if (fromPos && toPos) {
            const arc = ArcLogic.calculateArcPathFromPositions(
              fromPos,
              toPos,
              3,
              maxRight,
              ROW_HEIGHT,
            );
            if (hitarea) {
              hitarea.setAttribute('d', arc.path);
              hitarea.classList.remove(C.collapsed);
            }
            const visibleArc = DomAdapter.getVisibleArc(arcId);
            if (visibleArc) {
              visibleArc.setAttribute('d', arc.path);
              visibleArc.classList.remove(C.collapsed);
            }

            const strokeWidth = StaticData.getArcStrokeWidth(arcId);
            const scale = ArcLogic.scaleFromStrokeWidth(strokeWidth);
            // Update ALL arrow positions (even hidden ones) so they have correct position when shown
            DomAdapter.getArrows(`${fromId}-${toId}`).forEach((arrow) => {
              arrow.setAttribute(
                'points',
                ArcLogic.getArrowPoints({ x: arc.toX, y: arc.toY }, scale),
              );
              arrow.classList.remove(C.collapsed);
            });
          }
        }
      });
    }

    // Collect node IDs hidden by active filters (external-dep toggle etc.)
    // Cached: recomputed only after filter toggles invalidate via invalidateFilterHiddenNodeIds().
    let _filterHiddenNodeIds = null;

    function getFilterHiddenNodeIds() {
      if (_filterHiddenNodeIds !== null) return _filterHiddenNodeIds;
      const hidden = new Set();
      for (const nodeId of StaticData.getAllNodeIds()) {
        const node = DomAdapter.getNode(nodeId);
        if (node?.classList.contains(C.hiddenByFilter)) {
          hidden.add(nodeId);
        }
      }
      _filterHiddenNodeIds = hidden;
      return hidden;
    }

    function invalidateFilterHiddenNodeIds() {
      _filterHiddenNodeIds = null;
    }

    // Recalculate and show virtual edges for collapsed nodes
    function recalculateVirtualEdges() {
      cleanupVirtualElements();

      const hiddenByFilter = getFilterHiddenNodeIds();
      const visibleNodes = DerivedState.deriveNodeVisibility(
        appState.collapsed,
        StaticData,
        hiddenByFilter,
      );
      // Read current DOM widths for collapsed nodes whose boxes were expanded
      const widthOverrides = new Map();
      for (const nodeId of appState.collapsed) {
        const rect = DomAdapter.getNode(nodeId);
        if (rect) {
          widthOverrides.set(nodeId, parseFloat(rect.getAttribute('width')));
        }
      }
      const currentPositions = DerivedState.computeCurrentPositions(
        appState.collapsed,
        StaticData,
        MARGIN,
        TOOLBAR_HEIGHT,
        ROW_HEIGHT,
        widthOverrides,
        hiddenByFilter,
      );
      const maxRight = DerivedState.computeMaxRight(currentPositions);

      const edgeData = extractEdgeData(visibleNodes);

      updateOriginalEdges(edgeData, currentPositions, maxRight);

      const layers = {
        baseArcs: DomAdapter.getElementById(LayerManager.LAYERS.BASE_ARCS),
        baseLabels: DomAdapter.getElementById(LayerManager.LAYERS.BASE_LABELS),
        hitareas: DomAdapter.getElementById(LayerManager.LAYERS.HITAREAS),
      };

      const virtualEdges = VirtualEdgeLogic.aggregateHiddenEdges(
        edgeData,
        /** @type {(nodeId: string) => string} */ (getVisibleAncestor),
      );
      const mergedEdges = VirtualEdgeLogic.prepareVirtualEdgeData(
        virtualEdges,
        currentPositions,
        maxRight,
        ArcLogic,
        ROW_HEIGHT,
      );
      renderVirtualElements(mergedEdges, layers);
    }

    // Create virtual arc elements (arcs, arrows, labels, hitareas)
    function renderVirtualElements(mergedEdges, layers) {
      const arcElements = [];
      const labelElements = [];
      const hitareaElements = [];

      mergedEdges.forEach((data, _key) => {
        const {
          fromId,
          toId,
          arc,
          strokeWidth,
          direction,
          count,
          hiddenEdgeData,
          originalArcs,
        } = data;
        const arcId = `${fromId}-${toId}`;

        // Arc path
        const path = DomAdapter.createSvgElement('path');
        path.setAttribute('class', `${C.virtualArc} ${direction}`);
        path.setAttribute('d', arc.path);
        path.setAttribute('data-arc-id', arcId);
        path.setAttribute('data-from', fromId);
        path.setAttribute('data-to', toId);
        path.style.strokeWidth = `${strokeWidth}px`;
        arcElements.push(path);

        // Arrow (scaled to match stroke width)
        const scale = strokeWidth / 1.5;
        const arrow = DomAdapter.createSvgElement('polygon');
        arrow.setAttribute('class', `${C.virtualArrow} ${direction}`);
        arrow.setAttribute('data-vedge', arcId);
        arrow.setAttribute('data-from', fromId);
        arrow.setAttribute('data-to', toId);
        arrow.setAttribute(
          'points',
          ArcLogic.getArrowPoints({ x: arc.toX, y: arc.toY }, scale),
        );
        arrow.addEventListener('click', (e) => {
          e.stopPropagation();
          highlightVirtualEdge(fromId, toId);
        });
        arcElements.push(arrow);

        // Label (only for multi-edge arcs)
        let labelGroup = null;
        if (count > 1) {
          labelGroup = DomAdapter.createSvgElement('g');
          labelGroup.setAttribute('class', C.arcCountGroup);
          labelGroup.setAttribute('data-vedge', arcId);
          labelGroup.setAttribute('data-from', fromId);
          labelGroup.setAttribute('data-to', toId);
          const text = `(${count})`;
          const x = arc.ctrlX + 5;
          const y = arc.midY + 3;

          const padding = 2;
          const textWidth = text.length * 6; // ~6px per char at 10px font
          const bg = DomAdapter.createSvgElement('rect');
          bg.setAttribute('class', C.arcCountBg);
          bg.setAttribute('x', x - textWidth / 2 - padding);
          bg.setAttribute('y', y - 8 - padding);
          bg.setAttribute('width', textWidth + padding * 2);
          bg.setAttribute('height', 12 + padding * 2);
          bg.setAttribute('rx', '2');

          const countLabel = DomAdapter.createSvgElement('text');
          countLabel.setAttribute('class', C.arcCount);
          countLabel.setAttribute('data-vedge', arcId);
          countLabel.setAttribute('x', x);
          countLabel.setAttribute('y', y);
          countLabel.textContent = text;

          labelGroup.appendChild(bg);
          labelGroup.appendChild(countLabel);
          labelGroup.style.cursor = 'pointer';
          labelGroup.addEventListener('click', (e) => {
            e.stopPropagation();
            highlightVirtualEdge(fromId, toId);
          });
          labelGroup.addEventListener('mouseenter', () =>
            handleMouseEnter('arc', arcId),
          );
          labelGroup.addEventListener('mouseleave', handleMouseLeave);

          labelElements.push(labelGroup);
        }

        // Hitarea
        const hitarea = DomAdapter.createSvgElement('path');
        hitarea.setAttribute('class', `${C.virtualHitarea} ${C.arcHitarea}`);
        hitarea.setAttribute('d', arc.path);
        hitarea.setAttribute('data-arc-id', arcId);
        hitarea.setAttribute('data-from', fromId);
        hitarea.setAttribute('data-to', toId);
        if (hiddenEdgeData.length > 0) {
          virtualArcUsages.set(arcId, hiddenEdgeData.flat());
        }
        if (originalArcs && originalArcs.length > 0) {
          virtualArcOriginals.set(arcId, originalArcs);
        }
        hitarea.addEventListener('click', (e) => {
          e.stopPropagation();
          highlightVirtualEdge(fromId, toId);
        });
        hitarea.addEventListener('mouseenter', () =>
          handleVirtualMouseEnter(arcId, fromId, toId),
        );
        hitarea.addEventListener('mouseleave', () => {
          handleMouseLeave();
        });
        hitareaElements.push(hitarea);

        // Cache with direct references (no DOM queries needed)
        DomAdapter.cacheArcElements(arcId, path, [arrow], labelGroup);
      });

      for (const el of arcElements) layers.baseArcs.appendChild(el);
      for (const el of labelElements) layers.baseLabels.appendChild(el);
      for (const el of hitareaElements) layers.hitareas.appendChild(el);
    }

    function highlightVirtualEdge(fromId, toId) {
      const edgeId = `${fromId}-${toId}`;
      handleEdgeClick(edgeId, () => {
        const usages = virtualArcUsages.get(edgeId) || [];
        const originalArcs = virtualArcOriginals.get(edgeId) || [];
        const mergedUsages = SidebarLogic.mergeSymbolGroups(usages);
        SidebarLogic.show(edgeId, {
          from: fromId,
          to: toId,
          usages: mergedUsages,
          originalArcs,
        });
      });
    }

    // --- Collapse helpers ---

    function updateDescendantVisibility(descId, collapsed) {
      const node = DomAdapter.getNode(descId);
      const label = node?.nextElementSibling;
      const toggle = DomAdapter.getCollapseToggle(descId);
      node?.classList.toggle(C.collapsed, collapsed);
      label?.classList.toggle(C.collapsed, collapsed);
      toggle?.classList.toggle(C.collapsed, collapsed);
      DomAdapter.getTreeLines(descId, 'child').forEach((line) => {
        if (collapsed) {
          line.classList.add(C.collapsed);
        } else if (!node?.classList.contains(C.collapsed)) {
          line.classList.remove(C.collapsed);
        }
      });
    }

    function hasCollapsedAncestor(descId, excludeNodeId) {
      let checkId = descId;
      while (true) {
        const parentId = parentMap.get(checkId);
        if (!parentId) return false;
        if (
          AppState.isCollapsed(appState, parentId) &&
          parentId !== excludeNodeId
        ) {
          return true;
        }
        checkId = parentId;
      }
    }

    function updateParentNodeUI(nodeId, collapsed) {
      const toggleIcon = DomAdapter.getCollapseToggle(nodeId);
      if (toggleIcon) {
        toggleIcon.textContent = collapsed ? '+' : '−';
      }
      const countLabel = DomAdapter.getCountLabel(nodeId);
      if (!countLabel) return;
      const nodeRect = DomAdapter.getNode(nodeId);
      if (!nodeRect) return;
      if (!nodeRect.hasAttribute('data-original-width')) {
        nodeRect.setAttribute(
          'data-original-width',
          nodeRect.getAttribute('width'),
        );
      }
      // A collapsed parent hiding a cycle node shows the marker glyph (same
      // loop-closer as the sidebar's closing edge); CSS reveals it and turns
      // the label red only under cluster mode.
      const hidesCycle =
        collapsed &&
        TreeLogic.hasCycleDescendant(
          nodeId,
          parentMap,
          StaticData.getCycleNodeIds(),
        );
      const cycleMarker = DomAdapter.getCycleMarker(nodeId);
      if (cycleMarker) cycleMarker.textContent = hidesCycle ? ' ↺' : '';
      countLabel.parentElement?.classList.toggle(C.hidesCycle, hidesCycle);
      if (collapsed) {
        countLabel.textContent = ` (+${countDescendants(nodeId)})`;
        const labelText = countLabel.parentElement;
        if (labelText) {
          const textWidth = TextMeasure.estimateWidth(
            labelText.textContent,
            12,
          );
          const padding = 20;
          const neededWidth = textWidth + padding;
          const originalWidth = parseFloat(
            nodeRect.getAttribute('data-original-width'),
          );
          const newWidth = Math.max(originalWidth, neededWidth);
          nodeRect.setAttribute('width', newWidth);
          // Reposition toggle icon to right edge of expanded box
          if (toggleIcon) {
            const nodeX = parseFloat(nodeRect.getAttribute('x'));
            toggleIcon.setAttribute('x', nodeX + newWidth - TOGGLE_OFFSET);
          }
        }
      } else {
        countLabel.textContent = '';
        const originalWidth = nodeRect.getAttribute('data-original-width');
        if (originalWidth) {
          nodeRect.setAttribute('width', originalWidth);
          // Restore toggle icon to original position
          if (toggleIcon) {
            const nodeX = parseFloat(nodeRect.getAttribute('x'));
            toggleIcon.setAttribute(
              'x',
              nodeX + parseFloat(originalWidth) - TOGGLE_OFFSET,
            );
          }
        }
      }
    }

    // Collapses or expands one node in state and DOM, without a relayout.
    // A descendant under another collapsed ancestor stays hidden.
    function setCollapsed(nodeId, collapsed) {
      AppState.setCollapsed(appState, nodeId, collapsed);
      getDescendants(nodeId).forEach((descId) => {
        if (collapsed || !hasCollapsedAncestor(descId, nodeId)) {
          updateDescendantVisibility(descId, collapsed);
        }
      });
      updateParentNodeUI(nodeId, collapsed);
    }

    // Toggle collapse state
    function toggleCollapse(nodeId) {
      setCollapsed(nodeId, !AppState.isCollapsed(appState, nodeId));
      relayout();
      SearchLogic.refresh();
      refreshPinnedSidebar();
    }

    // Toggle collapse/expand all parent nodes.
    // Action follows the button label: "Collapse All" collapses, "Expand All" expands.
    // Returns true when nodes were collapsed, false when expanded.
    function toggleCollapseAll() {
      const btn = DomAdapter.getElementById('collapse-toggle-btn');
      const collapsed = btn?.textContent?.trim() === 'Collapse All';

      const parentNodeIds = StaticData.getAllNodeIds().filter((id) =>
        StaticData.hasChildren(id),
      );

      parentNodeIds.forEach((nodeId) => {
        AppState.setCollapsed(appState, nodeId, collapsed);
        getDescendants(nodeId).forEach((descId) => {
          updateDescendantVisibility(descId, collapsed);
        });
        updateParentNodeUI(nodeId, collapsed);
      });

      if (btn) btn.textContent = collapsed ? 'Expand All' : 'Collapse All';

      relayout();
      SearchLogic.refresh();
      refreshPinnedSidebar();

      return collapsed;
    }

    // Read which arc filters are currently active from their checkboxes.
    function readActiveArcFilters() {
      const isOn = (selector) =>
        !!DomAdapter.querySelector(selector)?.classList.contains(C.checked);
      return {
        crateDep: isOn('#crate-dep-checkbox'),
        moduleDep: isOn('#module-dep-checkbox'),
        reexport: isOn('#reexport-dep-checkbox'),
        cycle: isOn('#cycles-checkbox'),
      };
    }

    // Derive from an arc's classes which filters cover it.
    function arcFilters(arc) {
      return {
        isCrateDep: arc.classList.contains(C.crateDepArc),
        isModuleDep: arc.classList.contains(C.moduleDepArc),
        isReexport: arc.classList.contains(C.reexportArc),
        isCycle: arc.classList.contains(C.cycleArc),
      };
    }

    // Recompute visibility of every arc from the active filters. An arc is
    // visible when any filter covering it (its type, plus the cycles filter for
    // cycle arcs) is active. Replaces per-class toggling so the outcome is
    // deterministic regardless of toggle order. Arcs with an endpoint hidden by
    // a node filter (external/transitive) stay hidden: both sides share the
    // hidden-by-filter class, so the arc recompute must not resurrect
    // node-filtered arcs.
    function recomputeArcVisibility() {
      const activeFilters = readActiveArcFilters();
      const filterHiddenNodes = getFilterHiddenNodeIds();

      DomAdapter.querySelectorAll(
        `.${C.arcHitarea}:not(.${C.virtualHitarea})`,
      ).forEach((hitarea) => {
        const arcId = hitarea.dataset.arcId;
        const arc = DomAdapter.getArc(arcId);
        if (!arc) return;

        const endpoints = StaticData.getArc(arcId);
        const nodeFilteredOut =
          !!endpoints &&
          (filterHiddenNodes.has(endpoints.from) ||
            filterHiddenNodes.has(endpoints.to));
        const visible =
          !nodeFilteredOut &&
          ArcLogic.matchesActiveFilters(arcFilters(arc), activeFilters);
        arc.classList.toggle(C.hiddenByFilter, !visible);
        hitarea.classList.toggle(C.hiddenByFilter, !visible);
        DomAdapter.getArrows(arcId).forEach((arrow) => {
          arrow.classList.toggle(C.hiddenByFilter, !visible);
        });
        if (visible) {
          AppState.showArc(appState, arcId);
        } else {
          AppState.hideArc(appState, arcId);
        }
      });

      // Virtual arcs are aggregated module-deps — they follow module-dep.
      DomAdapter.querySelectorAll(Selectors.allVirtualElements()).forEach(
        (el) => {
          el.classList.toggle(C.hiddenByFilter, !activeFilters.moduleDep);
        },
      );

      invalidateFilterHiddenNodeIds();
      highlightTiming.immediate();
    }

    // Flip one arc filter's checkbox, then recompute arc visibility.
    function toggleFilterCheckbox(checkboxSelector) {
      DomAdapter.querySelector(checkboxSelector)?.classList.toggle(C.checked);
      recomputeArcVisibility();
    }

    // Flip the cycles checkbox. Cluster mode gates the two-level cluster
    // interaction (a click selects the cluster, an inner edge then focuses) and
    // CSS-gates the red cycle styling on the root class. It doubles as the
    // cycles filter, so arc visibility is recomputed like the other filters.
    function toggleClusterMode() {
      const checkbox = DomAdapter.querySelector('#cycles-checkbox');
      const isChecked = checkbox ? checkbox.classList.toggle(C.checked) : false;
      DomAdapter.getSvgRoot()?.classList.toggle(C.clusterModeOn, isChecked);
      AppState.setClusterMode(appState, isChecked);
      recomputeArcVisibility();
    }

    function toggleFilteredNodes(isChecked, isTargetNode, isTargetArc) {
      StaticData.getAllNodeIds().forEach((nodeId) => {
        if (!isTargetNode(nodeId)) return;
        const rect = DomAdapter.getNode(nodeId);
        if (!rect) return;
        const label = rect.nextElementSibling;
        const toggle = DomAdapter.getCollapseToggle(nodeId);
        rect.classList.toggle(C.hiddenByFilter, !isChecked);
        label?.classList.toggle(C.hiddenByFilter, !isChecked);
        toggle?.classList.toggle(C.hiddenByFilter, !isChecked);
        DomAdapter.getTreeLines(nodeId, 'child').forEach((line) => {
          line.classList.toggle(C.hiddenByFilter, !isChecked);
        });
        DomAdapter.getTreeLines(nodeId, 'parent').forEach((line) => {
          line.classList.toggle(C.hiddenByFilter, !isChecked);
        });
      });

      DomAdapter.querySelectorAll(
        `.${C.arcHitarea}:not(.${C.virtualHitarea})`,
      ).forEach((hitarea) => {
        const arcId = hitarea.dataset.arcId;
        if (!isTargetArc(arcId)) return;

        if (isChecked) {
          hitarea.classList.remove(C.hiddenByFilter);
          AppState.showArc(appState, arcId);
        } else {
          hitarea.classList.add(C.hiddenByFilter);
          AppState.hideArc(appState, arcId);
        }
        const visibleArc = DomAdapter.getArc(arcId);
        visibleArc?.classList.toggle(C.hiddenByFilter, !isChecked);
        DomAdapter.getArrows(arcId).forEach((arrow) => {
          arrow.classList.toggle(C.hiddenByFilter, !isChecked);
        });
      });

      invalidateFilterHiddenNodeIds();
      relayout();
    }

    function toggleExternalDepVisibility() {
      const checkbox = DomAdapter.querySelector('#external-dep-checkbox');
      if (!checkbox) return;

      const isChecked = checkbox.classList.toggle(C.checked);

      toggleFilteredNodes(
        isChecked,
        (nodeId) => StaticData.isExternalNode(nodeId),
        (arcId) => StaticData.isExternalArc(arcId),
      );

      const transitiveCheckbox = DomAdapter.querySelector(
        '#transitive-dep-checkbox',
      );
      transitiveCheckbox?.classList.toggle(C.checked, isChecked);
    }

    function toggleTransitiveDepVisibility() {
      const checkbox = DomAdapter.querySelector('#transitive-dep-checkbox');
      if (!checkbox) return;

      const extCheckbox = DomAdapter.querySelector('#external-dep-checkbox');
      if (extCheckbox && !extCheckbox.classList.contains(C.checked)) return;

      const isChecked = checkbox.classList.toggle(C.checked);

      toggleFilteredNodes(
        isChecked,
        (nodeId) => StaticData.isTransitiveNode(nodeId),
        (arcId) => StaticData.isTransitiveArc(arcId),
      );
    }

    // Sync foreignObject height with actual toolbar content height (flex-wrap may grow).
    // The toolbar foreignObject starts with display:none in the static SVG
    // so it doesn't appear when viewing the file outside a browser.
    function syncToolbarHeight() {
      const fo = DomAdapter.getElementById('toolbar-fo');
      const root = DomAdapter.querySelector(`.${C.toolbarRoot}`);
      const graph = DomAdapter.getElementById('graph-content');
      if (!fo || !root) return;
      fo.style.display = '';
      const baseH = root.offsetHeight;
      if (baseH > 0) {
        // Include dropdown panel height when visible (absolutely positioned, outside normal flow)
        const panel = DomAdapter.querySelector(`.${C.toolbarDropdownPanel}`);
        const panelH =
          panel && panel.style.display !== 'none' ? panel.offsetHeight : 0;
        fo.setAttribute('height', baseH + panelH);
        // Shift graph content down by the delta between actual and default toolbar height
        const delta = baseH - TOOLBAR_HEIGHT;
        if (graph) {
          if (delta !== 0) {
            graph.setAttribute('transform', `translate(0, ${delta})`);
          } else {
            graph.removeAttribute('transform');
          }
        }
        if (baseH !== toolbarHeight) {
          toolbarHeight = baseH;
          SidebarLogic.setToolbarHeight(baseH);
          applyCanvasSize();
        }
      }
    }

    // Update toolbar and sidebar position to stay at top when scrolling
    function updateToolbarPosition() {
      placeToolbar();
      if (SidebarLogic.isVisible() && !_isNavigating)
        SidebarLogic.updatePosition();
    }

    // Keep the toolbar at the top-left of the visible area, as wide as it.
    function placeToolbar() {
      const fo = DomAdapter.getElementById('toolbar-fo');
      const svg = DomAdapter.getSvgRoot();
      if (!fo || !svg) return;

      const rect = svg.getBoundingClientRect();
      const scrollTop = Math.max(0, -rect.top);
      fo.setAttribute('y', String(scrollTop));

      // Keep toolbar aligned to visible viewport during horizontal scroll
      const scrollLeft = Math.max(0, -rect.left);
      const visibleWidth = Math.min(
        CanvasSize.visibleArea().width,
        rect.width - scrollLeft,
      );
      fo.setAttribute('x', String(scrollLeft));
      fo.setAttribute('width', String(visibleWidth));
    }

    window.addEventListener('scroll', updateToolbarPosition);
    window.addEventListener('resize', () => {
      applyCanvasSize();
      updateToolbarPosition();
    });

    // Observe toolbar content height changes (flex-wrap grows/shrinks)
    const toolbarRoot = DomAdapter.querySelector(`.${C.toolbarRoot}`);
    if (toolbarRoot && typeof ResizeObserver !== 'undefined') {
      new ResizeObserver(() => {
        syncToolbarHeight();
        updateToolbarPosition();
      }).observe(toolbarRoot);
    }

    function showJumpStatus(text) {
      const statusEl = DomAdapter.getElementById('jump-status');
      if (statusEl) statusEl.textContent = text;
    }
    const jumper = Jump.createJump((url) => fetch(url), showJumpStatus);

    // Theme: the root is the svg element in a file and the html element in
    // the served page; both carry the attributes the stylesheet reads.
    const themeControl = Theme.bootstrapControls();

    // The view menu's checkboxes, in the order they are restored: the
    // transitive one needs the external one on.
    const CHECKBOXES = [
      'crate-dep-checkbox',
      'module-dep-checkbox',
      'reexport-dep-checkbox',
      'cycles-checkbox',
      'external-dep-checkbox',
      'transitive-dep-checkbox',
    ];

    function readChecks() {
      /** @type {Record<string, boolean>} */
      const checks = {};
      for (const id of CHECKBOXES) {
        const box = DomAdapter.getElementById(id);
        if (box) checks[id] = box.classList.contains(C.checked);
      }
      return checks;
    }

    // Flips a checkbox through the same handler a click would use, so the
    // arcs and nodes it governs follow.
    function applyCheck(id, on) {
      const box = DomAdapter.getElementById(id);
      if (!box || box.classList.contains(C.checked) === on) return;
      if (id === 'cycles-checkbox') toggleClusterMode();
      else if (id === 'external-dep-checkbox') toggleExternalDepVisibility();
      else if (id === 'transitive-dep-checkbox')
        toggleTransitiveDepVisibility();
      else toggleFilterCheckbox(`#${id}`);
    }

    /** @type {ReturnType<typeof Follow.createFollow> | null} */
    let follow = null;

    function captureView() {
      const input = /** @type {HTMLInputElement | null} */ (
        DomAdapter.getElementById('search-input')
      );
      const scope = /** @type {HTMLElement | null} */ (
        DomAdapter.querySelector(`#scope-selector .${C.toolbarScopeActive}`)
      );
      return ViewSnapshot.capture(
        {
          collapsed: appState.collapsed,
          selection: appState.clickSelection,
          checks: readChecks(),
          search: {
            query: input?.value ?? '',
            scope: scope?.dataset.scope ?? 'all',
          },
          follow: follow ? follow.isEnabled() : true,
          scroll: { x: window.scrollX, y: window.scrollY },
        },
        STATIC_DATA.nodes,
      );
    }

    // Applies a restored view on top of the freshly rendered page: the
    // collapse state, one relayout, then the checkboxes, the pinned
    // selection, the search and the scroll.
    function applyRestoredView(view) {
      const wanted = new Set(view.collapsed);
      for (const nodeId of StaticData.getAllNodeIds()) {
        if (!StaticData.hasChildren(nodeId)) continue;
        const collapsed = wanted.has(nodeId);
        if (AppState.isCollapsed(appState, nodeId) !== collapsed) {
          setCollapsed(nodeId, collapsed);
        }
      }
      relayout();
      for (const id of CHECKBOXES) {
        if (id in view.checks) applyCheck(id, view.checks[id]);
      }
      if (view.selection.type === 'node' && view.selection.id !== null) {
        highlightNode(view.selection.id);
      } else if (view.selection.type === 'arc' && view.selection.id !== null) {
        const [from, to] = view.selection.id.split('-');
        highlightEdge(from, to);
      }
      const input = /** @type {HTMLInputElement | null} */ (
        DomAdapter.getElementById('search-input')
      );
      if (input && view.search.query) {
        input.value = view.search.query;
        const clearBtn = DomAdapter.getElementById('search-clear');
        if (clearBtn) clearBtn.style.display = 'block';
        SearchLogic.executeSearch(view.search.query, view.search.scope);
      } else {
        SearchLogic.setScope(view.search.scope);
      }
      window.scrollTo(view.scroll.x, view.scroll.y);
    }

    // The follow and switch toggles are rendered only for a page served by
    // `cargo arc ui`; a file written by `cargo arc -o` has no event stream
    // to open and no service to switch.
    const followToggle = DomAdapter.getElementById('follow-toggle');
    if (followToggle) {
      /** @type {Record<'externals' | 'tests', HTMLElement | null>} */
      const switchButtons = {
        externals: DomAdapter.getElementById('externals-toggle'),
        tests: DomAdapter.getElementById('tests-toggle'),
      };
      const switches = SwitchToggles.createSwitchToggles({
        post: SwitchToggles.postCommand,
        reload: () => {
          try {
            sessionStorage.setItem(VIEW_KEY, JSON.stringify(captureView()));
          } catch {
            // The view is lost; the page still shows the new diagram.
          }
          location.reload();
        },
        showStatus: showJumpStatus,
        showBusy: (name, busy) =>
          switchButtons[name]?.setAttribute('aria-busy', String(busy)),
        isOn: (name) =>
          switchButtons[name]?.getAttribute('aria-pressed') === 'true',
      });
      for (const name of /** @type {const} */ (['externals', 'tests'])) {
        switchButtons[name]?.addEventListener('click', (e) => {
          e.stopPropagation();
          switches.click(name);
        });
      }
      const onSaveButton = DomAdapter.getElementById('on-save-toggle');
      const onSave = OnSaveToggle.createOnSaveToggle({
        post: SwitchToggles.postCommand,
        showState: (on) =>
          onSaveButton?.setAttribute('aria-pressed', String(on)),
        showStatus: showJumpStatus,
        isOn: () => onSaveButton?.getAttribute('aria-pressed') === 'true',
      });
      onSaveButton?.addEventListener('click', (e) => {
        e.stopPropagation();
        onSave.click();
      });
      follow = Follow.createFollow({
        // One stream serves them all: the theme event is the editor's and
        // goes to the theme control, the follow module, the switches and
        // the on-save button take theirs.
        connect: (handler) =>
          Follow.connectEventSource((name, data) => {
            if (name === 'theme') themeControl.handleEditorMode(data);
            else {
              handler(name, data);
              switches.handleEvent(name, data);
              onSave.handleEvent(name, data);
            }
          }),
        apply: focusNode,
        showState: (on) =>
          followToggle.setAttribute('aria-pressed', String(on)),
      });
      if (restoredView) follow.setEnabled(restoredView.follow);
      followToggle.addEventListener('click', (e) => {
        e.stopPropagation();
        if (follow) follow.setEnabled(!follow.isEnabled());
      });
      follow.start();
    }

    // Jump popover shown next to a hovered node; positioned from the node
    // rect's live attributes, so it tracks collapse-driven moves.
    const jumpIcons = JumpIcons.createJumpIcons({
      layer: /** @type {Element} */ (
        DomAdapter.getElementById('jump-popover-layer')
      ),
      defsHost: /** @type {Element} */ (DomAdapter.getSvgRoot()),
      onJump: (id) => jumper.jump(id),
      onEnter: (nodeId) => handleMouseEnter('node', nodeId),
      onLeave: handleMouseLeave,
      setTimeout,
      clearTimeout,
      grace: HOVER_HIDE_GRACE,
    });

    // === Event handlers ===
    // Iterate via StaticData instead of DOM query
    StaticData.getAllNodeIds().forEach((nodeId) => {
      const node = DomAdapter.getNode(nodeId);
      if (!node) return;
      const targets = StaticData.getNode(nodeId)?.targets;
      const hasTargets = targets && targets.length > 0;

      node.addEventListener('click', (e) => {
        e.stopPropagation();
        highlightNode(nodeId);
      });

      node.addEventListener('mouseenter', () => {
        handleMouseEnter('node', nodeId);
        if (hasTargets) jumpIcons.show(nodeId, node, targets);
      });
      node.addEventListener('mouseleave', () => {
        handleMouseLeave();
        // Own timer: handleMouseLeave returns early while a selection is
        // pinned, so the popover cannot ride on the highlight grace.
        if (hasTargets) jumpIcons.scheduleHide();
      });

      // Double-click to toggle collapse (only for parents)
      if (StaticData.hasChildren(nodeId)) {
        node.addEventListener('dblclick', (e) => {
          e.stopPropagation();
          toggleCollapse(nodeId);
          updateToolbarPosition();
        });
      }
    });

    DomAdapter.querySelectorAll(`.${C.collapseToggle}`).forEach((toggle) => {
      toggle.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleCollapse(toggle.dataset.target);
        updateToolbarPosition();
      });
    });

    // Toolbar button event handlers
    DomAdapter.querySelector('#collapse-toggle-btn')?.addEventListener(
      'click',
      (e) => {
        e.stopPropagation();
        const collapsed = toggleCollapseAll();
        updateToolbarPosition();
        if (collapsed) window.scrollTo(0, 0);
      },
    );
    DomAdapter.querySelector('#crate-dep-checkbox')
      ?.closest('label')
      ?.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleFilterCheckbox('#crate-dep-checkbox');
      });

    DomAdapter.querySelector('#module-dep-checkbox')
      ?.closest('label')
      ?.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleFilterCheckbox('#module-dep-checkbox');
      });

    DomAdapter.querySelector('#reexport-dep-checkbox')
      ?.closest('label')
      ?.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleFilterCheckbox('#reexport-dep-checkbox');
      });

    DomAdapter.querySelector('#cycles-checkbox')
      ?.closest('label')
      ?.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleClusterMode();
      });

    DomAdapter.querySelector('#external-dep-checkbox')
      ?.closest('label')
      ?.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleExternalDepVisibility();
      });

    DomAdapter.querySelector('#transitive-dep-checkbox')
      ?.closest('label')
      ?.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleTransitiveDepVisibility();
      });

    // Dropdown toggle for "View" button
    const viewDropdownBtn = DomAdapter.getElementById('view-dropdown-btn');
    const dropdownPanel = DomAdapter.querySelector(
      `.${C.toolbarDropdownPanel}`,
    );
    if (viewDropdownBtn && dropdownPanel) {
      viewDropdownBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        const isOpen = dropdownPanel.style.display !== 'none';
        dropdownPanel.style.display = isOpen ? 'none' : '';
        syncToolbarHeight();
      });
      dropdownPanel.addEventListener('click', (e) => {
        e.stopPropagation();
      });
    }

    // Event handlers on hit-area paths (invisible, 12px wide) — regular arcs only
    DomAdapter.querySelectorAll(
      `.${C.arcHitarea}:not(.${C.virtualHitarea})`,
    ).forEach((hitarea) => {
      const edgeId = `${hitarea.dataset.from}-${hitarea.dataset.to}`;

      hitarea.addEventListener('click', (e) => {
        e.stopPropagation();
        highlightEdge(hitarea.dataset.from, hitarea.dataset.to);
      });

      hitarea.addEventListener('mouseenter', () =>
        handleMouseEnter('arc', edgeId),
      );

      hitarea.addEventListener('mouseleave', handleMouseLeave);
    });

    DomAdapter.getSvgRoot().addEventListener('click', () => {
      AppState.clickEmpty(appState);
      highlightTiming.immediate();
      SidebarLogic.hide();
      if (dropdownPanel) {
        dropdownPanel.style.display = 'none';
        syncToolbarHeight();
      }
    });

    // Close-button and click isolation for sidebar foreignObject
    const sidebarEl = DomAdapter.getElementById('relation-sidebar');
    if (sidebarEl) {
      sidebarEl.addEventListener('click', (e) => {
        e.stopPropagation(); // Prevent SVG background click
        const target = /** @type {Element} */ (e.target);
        if (target.classList.contains('sidebar-close')) {
          AppState.clickEmpty(appState);
          highlightTiming.immediate();
          SidebarLogic.hide();
        }
        // Sidebar rows are only clickable while pinned, a selected SCC
        // included; the transient sidebar closes on its own hover-leave
        // grace period.
        if (AppState.getPinned(appState) || clusterActive()) {
          const jumpId = jumpIdFromClick(target);
          if (jumpId !== null) jumper.jump(jumpId);
        }
      });
    }

    // Apply initial arc weights based on source location counts
    applyInitialArcWeights();

    // Populate arc element cache for O(1) lookups during highlight
    for (const arcId of StaticData.getAllArcIds()) {
      DomAdapter.cacheArcElements(
        arcId,
        DomAdapter.querySelector(Selectors.baseArc(arcId)),
        Array.from(DomAdapter.querySelectorAll(Selectors.arrows(arcId))),
        DomAdapter.querySelector(Selectors.labelGroup(arcId)),
      );
    }

    // Initialize search module
    SearchLogic.init(appState);

    // Widen the SVG and the toolbar to the window before the toolbar's
    // height is measured, or the measurement counts the rows the toolbar
    // wraps into at the diagram's width.
    applyCanvasSize();
    // Sync toolbar foreignObject height with actual content
    syncToolbarHeight();

    if (restoredView) {
      applyRestoredView(restoredView);
    } else if (expandLevel !== null) {
      // Bootstrap virtual arcs for initially collapsed nodes (expand-level)
      relayout();
    }
    // `?select=<file>`: the same node identity the hotspot map's own
    // toolbar link carries, treated like an editor focus event. Applied
    // whether or not a view was restored above: a stored view exists only
    // between a recompute and its reload, and reading it just cleared it,
    // so one left over from a recompute the user abandoned for the map
    // reflects an older moment than a `?select=` arriving now - the newer,
    // more specific signal wins over its own captured selection.
    const selectedFile = PageLink.parseSelect(location.search);
    if (selectedFile) {
      const nodeId = StaticData.findNodeIdByFile(selectedFile);
      if (nodeId) focusNode(nodeId, []);
    }
    updateHotspotLink();
  })();
}
