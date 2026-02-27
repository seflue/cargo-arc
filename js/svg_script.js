// @module SvgScript
// @deps ArcLogic, StaticData, AppState, Selectors, DomAdapter, LayerManager, TreeLogic, DerivedState, HighlightRenderer, VirtualEdgeLogic, TextMetrics, SidebarLogic, SearchLogic
// @config ROW_HEIGHT, MARGIN, TOOLBAR_HEIGHT
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

if (typeof module !== 'undefined') {
  module.exports = { createHighlightDebouncer };
}

// IIFE for SVG embedding (DOM-code) - only runs in browser with placeholders replaced
if (typeof document !== 'undefined') {
  (() => {
    const ROW_HEIGHT = __ROW_HEIGHT__;
    const MARGIN = __MARGIN__;
    const TOOLBAR_HEIGHT = __TOOLBAR_HEIGHT__;
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
    }

    const highlightTiming = createHighlightDebouncer(rerenderHighlights, 30);

    // Shared toggle core for all clickable elements (edge, node, virtual edge).
    // The showSidebar callback provides the type-specific sidebar display logic.
    function toggleHighlight(type, id, showSidebar) {
      const isPinned = AppState.togglePinned(appState, type, id);
      highlightTiming.immediate();
      if (!isPinned) {
        SidebarLogic.hide();
        return;
      }
      showSidebar();
    }

    function highlightEdge(from, to) {
      const edgeId = `${from}-${to}`;
      toggleHighlight('arc', edgeId, () => SidebarLogic.show(edgeId));
    }

    function highlightNode(nodeId) {
      toggleHighlight('node', nodeId, () => {
        const relations = collectNodeRelations(nodeId);
        SidebarLogic.showNode(nodeId, relations);
      });
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
      base.outgoing.sort((a, b) => b.weight - a.weight);
      base.incoming.sort((a, b) => b.weight - a.weight);
      return base;
    }

    let _isNavigating = false;

    function scrollToNode(nodeId) {
      const nodeRect = DomAdapter.getNode(nodeId);
      if (!nodeRect) return;
      const svg = DomAdapter.getSvgRoot();
      if (!svg) return;
      const svgRect = svg.getBoundingClientRect();
      const vb = svg.viewBox.baseVal;
      const scaleY = vb.height / svgRect.height;
      const nodeY = parseFloat(nodeRect.getAttribute('y'));
      const nodeH = parseFloat(nodeRect.getAttribute('height'));
      const nodeCenterSvg = nodeY + nodeH / 2;
      const nodeCenterPage =
        nodeCenterSvg / scaleY + svgRect.top + window.scrollY;
      const targetScroll = nodeCenterPage - window.innerHeight / 2;
      const maxScroll =
        document.documentElement.scrollHeight - window.innerHeight;
      _isNavigating = true;
      window.addEventListener(
        'scrollend',
        () => {
          _isNavigating = false;
          if (SidebarLogic.isVisible()) SidebarLogic.updatePosition();
        },
        { once: true },
      );
      setTimeout(() => {
        _isNavigating = false;
        if (SidebarLogic.isVisible()) SidebarLogic.updatePosition();
      }, 600);
      window.scrollTo({
        top: Math.max(0, Math.min(targetScroll, maxScroll)),
        behavior: 'smooth',
      });
    }

    SidebarLogic._onBadgeClick = (nodeId) => scrollToNode(nodeId);

    function handleMouseEnter(type, id) {
      if (AppState.hasPinnedSelection(appState)) return;
      AppState.setHover(appState, type, id);
      highlightTiming.debounced();
      if (type === 'node') {
        const relations = collectNodeRelations(id);
        SidebarLogic.showTransientNode(id, relations);
      } else if (type === 'arc') {
        SidebarLogic.showTransient(id);
      }
    }

    function handleMouseLeave() {
      if (AppState.hasPinnedSelection(appState)) return;
      AppState.clearHover(appState);
      highlightTiming.debounced();
      SidebarLogic.hideTransient();
    }

    function handleVirtualMouseEnter(arcId, fromId, toId) {
      if (AppState.hasPinnedSelection(appState)) return;
      AppState.setHover(appState, 'arc', arcId);
      highlightTiming.debounced();
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

    // Relayout visible nodes
    function relayout() {
      let currentY = MARGIN + TOOLBAR_HEIGHT;

      // Get all node IDs sorted by original Y position (no DOM query for list)
      const sortedIds = StaticData.getAllNodeIds().sort((a, b) => {
        const posA = StaticData.getOriginalPosition(a);
        const posB = StaticData.getOriginalPosition(b);
        return posA.y - posB.y;
      });

      sortedIds.forEach((nodeId) => {
        const node = DomAdapter.getNode(nodeId);
        if (!node) return;
        if (node.classList.contains(C.collapsed)) return;
        if (node.classList.contains(C.hiddenByFilter)) return;

        // Get height from StaticData (no DOM read)
        const height = StaticData.getOriginalPosition(nodeId).height;

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

      // Re-apply highlights after edges were recreated
      highlightTiming.immediate();

      // Invalidate sidebar layout cache after arc positions changed
      SidebarLogic.invalidateLayout();
      if (SidebarLogic.isVisible()) SidebarLogic.updatePosition();
    }

    // Helper: Calculate arc path from position objects (no DOM read)
    function calculateArcPathFromPositions(fromPos, toPos, yOffset, maxRight) {
      return ArcLogic.calculateArcPathFromPositions(
        fromPos,
        toPos,
        yOffset,
        maxRight,
        ROW_HEIGHT,
      );
    }

    // Helper: Extract edge data from DOM hitareas to pure objects
    // Uses DerivedState for visibility instead of per-edge getVisibleAncestor calls
    function extractEdgeData(hitareas, visibleNodes) {
      const edges = [];
      hitareas.forEach((hitarea) => {
        const fromId = hitarea.dataset.from;
        const toId = hitarea.dataset.to;

        // A node is hidden if it's not in the visibleNodes set
        // (computed once via DerivedState.deriveNodeVisibility)
        const fromIsHidden = !visibleNodes.has(fromId);
        const toIsHidden = !visibleNodes.has(toId);

        const arcId = hitarea.dataset.arcId;
        edges.push({
          hitarea,
          arcId,
          fromId,
          toId,
          fromHidden: fromIsHidden,
          toHidden: toIsHidden,
          sourceLocations: StaticData.getArcUsages(arcId),
          // Compute direction from hierarchy (no DOM read)
          direction: DerivedState._determineDirection(fromId, toId, parentMap),
        });
      });
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
          // Hide original elements
          hitarea.style.display = 'none';
          const visibleArc = DomAdapter.getVisibleArc(arcId);
          if (visibleArc) visibleArc.style.display = 'none';
          DomAdapter.getArrows(`${fromId}-${toId}`).forEach((arr) => {
            arr.style.display = 'none';
          });
        } else {
          // Update visible arc paths using computed positions (no DOM read)
          const fromPos = currentPositions.get(fromId);
          const toPos = currentPositions.get(toId);
          if (fromPos && toPos) {
            const arc = calculateArcPathFromPositions(
              fromPos,
              toPos,
              3,
              maxRight,
            );
            hitarea.setAttribute('d', arc.path);
            const visibleArc = DomAdapter.getVisibleArc(arcId);
            if (visibleArc) visibleArc.setAttribute('d', arc.path);

            const strokeWidth = StaticData.getArcStrokeWidth(arcId);
            const scale = ArcLogic.scaleFromStrokeWidth(strokeWidth);
            // Update ALL arrow positions (even hidden ones) so they have correct position when shown
            DomAdapter.getArrows(`${fromId}-${toId}`).forEach((arrow) => {
              arrow.setAttribute(
                'points',
                ArcLogic.getArrowPoints({ x: arc.toX, y: arc.toY }, scale),
              );
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

      const hitareas = DomAdapter.getAllHitareas();
      const edgeData = extractEdgeData(hitareas, visibleNodes);

      updateOriginalEdges(edgeData, currentPositions, maxRight);

      const virtualEdges = VirtualEdgeLogic.aggregateHiddenEdges(
        edgeData,
        getVisibleAncestor,
      );
      const mergedEdges = VirtualEdgeLogic.prepareVirtualEdgeData(
        virtualEdges,
        currentPositions,
        maxRight,
        ArcLogic,
        ROW_HEIGHT,
      );

      const layers = {
        baseArcs: DomAdapter.getElementById(LayerManager.LAYERS.BASE_ARCS),
        baseLabels: DomAdapter.getElementById(LayerManager.LAYERS.BASE_LABELS),
        hitareas: DomAdapter.getElementById(LayerManager.LAYERS.HITAREAS),
      };
      renderVirtualElements(mergedEdges, layers);
    }

    // Create virtual arc elements (arcs, arrows, labels, hitareas)
    function renderVirtualElements(mergedEdges, layers) {
      // Pass 1: Arcs + Arrows (bottom layer)
      mergedEdges.forEach((data, _key) => {
        const { fromId, toId, arc, strokeWidth, direction } = data;
        const arcId = `${fromId}-${toId}`;

        // Visible path (styled, no pointer events)
        const path = DomAdapter.createSvgElement('path');
        path.setAttribute('class', `${C.virtualArc} ${direction}`);
        path.setAttribute('d', arc.path);
        path.setAttribute('data-arc-id', arcId);
        path.setAttribute('data-from', fromId);
        path.setAttribute('data-to', toId);
        path.style.strokeWidth = `${strokeWidth}px`;
        layers.baseArcs.appendChild(path);

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
        layers.baseArcs.appendChild(arrow);
      });

      // Pass 2: Labels (middle layer, above arcs)
      mergedEdges.forEach((data, _key) => {
        const { fromId, toId, arc, count } = data;

        if (count > 1) {
          const labelGroup = DomAdapter.createSvgElement('g');
          labelGroup.setAttribute('class', C.arcCountGroup);
          labelGroup.setAttribute('data-vedge', `${fromId}-${toId}`);
          labelGroup.setAttribute('data-from', fromId);
          labelGroup.setAttribute('data-to', toId);
          const text = `(${count})`;
          const x = arc.ctrlX + 5;
          const y = arc.midY + 3;

          // Background rect (2-3px padding)
          const padding = 2;
          const textWidth = text.length * 6; // ~6px per char at 10px font
          const bg = DomAdapter.createSvgElement('rect');
          bg.setAttribute('class', C.arcCountBg);
          bg.setAttribute('x', x - textWidth / 2 - padding);
          bg.setAttribute('y', y - 8 - padding);
          bg.setAttribute('width', textWidth + padding * 2);
          bg.setAttribute('height', 12 + padding * 2);
          bg.setAttribute('rx', '2');

          // Text label
          const countLabel = DomAdapter.createSvgElement('text');
          countLabel.setAttribute('class', C.arcCount);
          countLabel.setAttribute('data-vedge', `${fromId}-${toId}`);
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
            handleMouseEnter('arc', `${fromId}-${toId}`),
          );
          labelGroup.addEventListener('mouseleave', handleMouseLeave);

          layers.baseLabels.appendChild(labelGroup);
        }
      });

      // Pass 3: Hitareas (hitareas layer, always on top)
      mergedEdges.forEach((data, _key) => {
        const { fromId, toId, arc, hiddenEdgeData, originalArcs } = data;
        const arcId = `${fromId}-${toId}`;

        const hitarea = DomAdapter.createSvgElement('path');
        hitarea.setAttribute('class', `${C.virtualHitarea} ${C.arcHitarea}`);
        hitarea.setAttribute('d', arc.path);
        hitarea.setAttribute('data-arc-id', arcId);
        hitarea.setAttribute('data-from', fromId);
        hitarea.setAttribute('data-to', toId);
        // Store structured usages and originalArcs in runtime Map (not DOM attribute)
        if (hiddenEdgeData.length > 0) {
          const allUsages = hiddenEdgeData.flat();
          virtualArcUsages.set(arcId, allUsages);
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
        layers.hitareas.appendChild(hitarea);
      });

      // Cache virtual arc elements for O(1) lookups during highlight
      mergedEdges.forEach((data, _key) => {
        const { fromId, toId } = data;
        const arcId = `${fromId}-${toId}`;
        const arcs = DomAdapter.querySelectorAll(
          `.${C.virtualArc}[data-arc-id="${arcId}"]`,
        );
        const arrows = Array.from(
          DomAdapter.querySelectorAll(
            `.${C.virtualArrow}[data-vedge="${arcId}"]`,
          ),
        );
        const labelGroup = DomAdapter.querySelector(
          `.${C.arcCountGroup}[data-vedge="${arcId}"]`,
        );
        DomAdapter.cacheArcElements(
          arcId,
          arcs.length === 1 ? arcs[0] : null,
          arrows,
          labelGroup,
        );
      });
    }

    function highlightVirtualEdge(fromId, toId) {
      const edgeId = `${fromId}-${toId}`;
      toggleHighlight('arc', edgeId, () => {
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
      if (collapsed) {
        node?.classList.add(C.collapsed);
        label?.classList.add(C.collapsed);
        toggle?.classList.add(C.collapsed);
      } else {
        node?.classList.remove(C.collapsed);
        label?.classList.remove(C.collapsed);
        toggle?.classList.remove(C.collapsed);
      }
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
      if (collapsed) {
        countLabel.textContent = ` (+${countDescendants(nodeId)})`;
        const labelText = countLabel.parentElement;
        if (labelText) {
          const textWidth = TextMetrics.estimateWidth(
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

    // Toggle collapse state
    function toggleCollapse(nodeId) {
      const collapsed = AppState.toggleCollapsed(appState, nodeId);

      getDescendants(nodeId).forEach((descId) => {
        if (collapsed || !hasCollapsedAncestor(descId, nodeId)) {
          updateDescendantVisibility(descId, collapsed);
        }
      });

      updateParentNodeUI(nodeId, collapsed);
      relayout();
      SearchLogic.refresh();
    }

    // Toggle collapse/expand all parent nodes
    function toggleCollapseAll() {
      const parentNodeIds = StaticData.getAllNodeIds().filter((id) =>
        StaticData.hasChildren(id),
      );
      const allExpanded = parentNodeIds.every(
        (id) => !AppState.isCollapsed(appState, id),
      );
      const collapsed = allExpanded;

      parentNodeIds.forEach((nodeId) => {
        AppState.setCollapsed(appState, nodeId, collapsed);
        getDescendants(nodeId).forEach((descId) => {
          updateDescendantVisibility(descId, collapsed);
        });
        updateParentNodeUI(nodeId, collapsed);
      });

      const btn = DomAdapter.getElementById('collapse-toggle-btn');
      if (btn) btn.textContent = collapsed ? 'Expand All' : 'Collapse All';

      relayout();
      SearchLogic.refresh();
    }

    // Generic toggle for arc-class visibility (shared by crate-dep and module-dep)
    function toggleArcClassVisibility(checkboxSelector, arcClass) {
      const checkbox = DomAdapter.querySelector(checkboxSelector);
      if (!checkbox) return;

      const isChecked = checkbox.classList.toggle(C.checked);

      DomAdapter.querySelectorAll(`.${arcClass}`).forEach((arc) => {
        if (isChecked) {
          arc.classList.remove(C.hiddenByFilter);
        } else {
          arc.classList.add(C.hiddenByFilter);
        }
      });

      // Also hide/show associated hitareas and arrows, track in AppState
      DomAdapter.querySelectorAll(
        `.${C.arcHitarea}:not(.${C.virtualHitarea})`,
      ).forEach((hitarea) => {
        const arcId = hitarea.dataset.arcId;
        const visibleArc = DomAdapter.querySelector(
          `.${arcClass}[data-arc-id="${arcId}"]`,
        );
        if (visibleArc) {
          if (isChecked) {
            hitarea.classList.remove(C.hiddenByFilter);
            AppState.showArc(appState, arcId);
          } else {
            hitarea.classList.add(C.hiddenByFilter);
            AppState.hideArc(appState, arcId);
          }
          DomAdapter.getArrows(arcId).forEach((arrow) => {
            if (isChecked) {
              arrow.classList.remove(C.hiddenByFilter);
            } else {
              arrow.classList.add(C.hiddenByFilter);
            }
          });
        }
      });

      invalidateFilterHiddenNodeIds();
      highlightTiming.immediate();
    }

    // Toggle visibility of crate-to-crate dependency arcs
    function toggleCrateDepVisibility() {
      toggleArcClassVisibility('#crate-dep-checkbox', C.crateDepArc);
    }

    // Toggle visibility of module-to-module dependency arcs
    function toggleModuleDepVisibility() {
      toggleArcClassVisibility('#module-dep-checkbox', C.moduleDepArc);

      // Virtual arcs are aggregated module-deps — toggle them too
      const checkbox = DomAdapter.querySelector('#module-dep-checkbox');
      const isChecked = checkbox?.classList.contains(C.checked);
      DomAdapter.querySelectorAll(Selectors.allVirtualElements()).forEach(
        (el) => {
          if (isChecked) {
            el.classList.remove(C.hiddenByFilter);
          } else {
            el.classList.add(C.hiddenByFilter);
          }
        },
      );
    }

    // Toggle visibility of external dependency nodes and their arcs
    function toggleExternalDepVisibility() {
      const checkbox = DomAdapter.querySelector('#external-dep-checkbox');
      if (!checkbox) return;

      const isChecked = checkbox.classList.toggle(C.checked);

      // Toggle external-section and external nodes
      StaticData.getAllNodeIds().forEach((nodeId) => {
        const node = StaticData.getNode(nodeId);
        if (!node) return;
        if (!StaticData.isExternalNode(nodeId)) return;
        const rect = DomAdapter.getNode(nodeId);
        if (!rect) return;
        const label = rect.nextElementSibling;
        const toggle = DomAdapter.getCollapseToggle(nodeId);
        if (isChecked) {
          rect.classList.remove(C.hiddenByFilter);
          label?.classList.remove(C.hiddenByFilter);
          toggle?.classList.remove(C.hiddenByFilter);
        } else {
          rect.classList.add(C.hiddenByFilter);
          label?.classList.add(C.hiddenByFilter);
          toggle?.classList.add(C.hiddenByFilter);
        }
        // Toggle tree lines for external nodes
        DomAdapter.getTreeLines(nodeId, 'child').forEach((line) => {
          if (isChecked) {
            line.classList.remove(C.hiddenByFilter);
          } else {
            line.classList.add(C.hiddenByFilter);
          }
        });
        DomAdapter.getTreeLines(nodeId, 'parent').forEach((line) => {
          if (isChecked) {
            line.classList.remove(C.hiddenByFilter);
          } else {
            line.classList.add(C.hiddenByFilter);
          }
        });
      });

      // Toggle arcs connected to external nodes
      DomAdapter.querySelectorAll(
        `.${C.arcHitarea}:not(.${C.virtualHitarea})`,
      ).forEach((hitarea) => {
        const arcId = hitarea.dataset.arcId;
        if (!StaticData.isExternalArc(arcId)) return;

        if (isChecked) {
          hitarea.classList.remove(C.hiddenByFilter);
          AppState.showArc(appState, arcId);
        } else {
          hitarea.classList.add(C.hiddenByFilter);
          AppState.hideArc(appState, arcId);
        }
        const visibleArc = DomAdapter.getArc(arcId);
        if (visibleArc) {
          if (isChecked) {
            visibleArc.classList.remove(C.hiddenByFilter);
          } else {
            visibleArc.classList.add(C.hiddenByFilter);
          }
        }
        DomAdapter.getArrows(arcId).forEach((arrow) => {
          if (isChecked) {
            arrow.classList.remove(C.hiddenByFilter);
          } else {
            arrow.classList.add(C.hiddenByFilter);
          }
        });
      });

      // Sync transitive checkbox state when externals are toggled off
      const transitiveCheckbox = DomAdapter.querySelector(
        '#transitive-dep-checkbox',
      );
      if (transitiveCheckbox) {
        if (!isChecked) {
          // Externals hidden → force transitive unchecked
          transitiveCheckbox.classList.remove(C.checked);
        } else {
          // Externals shown → restore transitive to checked
          transitiveCheckbox.classList.add(C.checked);
        }
      }

      invalidateFilterHiddenNodeIds();
      relayout();
    }

    // Toggle visibility of transitive external dependency nodes and their arcs
    function toggleTransitiveDepVisibility() {
      const checkbox = DomAdapter.querySelector('#transitive-dep-checkbox');
      if (!checkbox) return;

      // If external deps are hidden, transitive toggle is a no-op
      const extCheckbox = DomAdapter.querySelector('#external-dep-checkbox');
      if (extCheckbox && !extCheckbox.classList.contains(C.checked)) return;

      const isChecked = checkbox.classList.toggle(C.checked);

      // Toggle transitive external nodes
      StaticData.getAllNodeIds().forEach((nodeId) => {
        if (!StaticData.isTransitiveNode(nodeId)) return;
        const rect = DomAdapter.getNode(nodeId);
        if (!rect) return;
        const label = rect.nextElementSibling;
        const toggle = DomAdapter.getCollapseToggle(nodeId);
        if (isChecked) {
          rect.classList.remove(C.hiddenByFilter);
          label?.classList.remove(C.hiddenByFilter);
          toggle?.classList.remove(C.hiddenByFilter);
        } else {
          rect.classList.add(C.hiddenByFilter);
          label?.classList.add(C.hiddenByFilter);
          toggle?.classList.add(C.hiddenByFilter);
        }
        DomAdapter.getTreeLines(nodeId, 'child').forEach((line) => {
          if (isChecked) {
            line.classList.remove(C.hiddenByFilter);
          } else {
            line.classList.add(C.hiddenByFilter);
          }
        });
        DomAdapter.getTreeLines(nodeId, 'parent').forEach((line) => {
          if (isChecked) {
            line.classList.remove(C.hiddenByFilter);
          } else {
            line.classList.add(C.hiddenByFilter);
          }
        });
      });

      // Toggle arcs connected to transitive nodes
      DomAdapter.querySelectorAll(
        `.${C.arcHitarea}:not(.${C.virtualHitarea})`,
      ).forEach((hitarea) => {
        const arcId = hitarea.dataset.arcId;
        if (!StaticData.isTransitiveArc(arcId)) return;

        if (isChecked) {
          hitarea.classList.remove(C.hiddenByFilter);
          AppState.showArc(appState, arcId);
        } else {
          hitarea.classList.add(C.hiddenByFilter);
          AppState.hideArc(appState, arcId);
        }
        const visibleArc = DomAdapter.getArc(arcId);
        if (visibleArc) {
          if (isChecked) {
            visibleArc.classList.remove(C.hiddenByFilter);
          } else {
            visibleArc.classList.add(C.hiddenByFilter);
          }
        }
        DomAdapter.getArrows(arcId).forEach((arrow) => {
          if (isChecked) {
            arrow.classList.remove(C.hiddenByFilter);
          } else {
            arrow.classList.add(C.hiddenByFilter);
          }
        });
      });

      invalidateFilterHiddenNodeIds();
      relayout();
    }

    // Sync foreignObject height with actual toolbar content height (flex-wrap may grow)
    function syncToolbarHeight() {
      const fo = DomAdapter.getElementById('toolbar-fo');
      const root = DomAdapter.querySelector(`.${C.toolbarRoot}`);
      const graph = DomAdapter.getElementById('graph-content');
      if (!fo || !root) return;
      const baseH = root.offsetHeight;
      if (baseH > 0) {
        // Include dropdown panel height when visible (absolutely positioned, outside normal flow)
        const panel = DomAdapter.querySelector(`.${C.toolbarDropdownPanel}`);
        const panelH =
          panel && panel.style.display !== 'none' ? panel.offsetHeight : 0;
        fo.setAttribute('height', baseH + panelH);
        // Shift graph content down by the delta between actual and default toolbar height
        if (graph) {
          const delta = baseH - TOOLBAR_HEIGHT;
          if (delta !== 0) {
            graph.setAttribute('transform', `translate(0, ${delta})`);
          } else {
            graph.removeAttribute('transform');
          }
        }
      }
    }

    // Update toolbar position to stay at top when scrolling
    function updateToolbarPosition() {
      const fo = DomAdapter.getElementById('toolbar-fo');
      const svg = DomAdapter.getSvgRoot();
      if (!fo || !svg) return;

      const rect = svg.getBoundingClientRect();
      const scrollTop = Math.max(0, -rect.top);
      fo.setAttribute('y', scrollTop);

      // Keep toolbar aligned to visible viewport during horizontal scroll
      const scrollLeft = Math.max(0, -rect.left);
      const visibleWidth = Math.min(window.innerWidth, rect.width - scrollLeft);
      fo.setAttribute('x', scrollLeft);
      fo.setAttribute('width', visibleWidth);

      if (SidebarLogic.isVisible() && !_isNavigating)
        SidebarLogic.updatePosition();
    }

    window.addEventListener('scroll', updateToolbarPosition);
    window.addEventListener('resize', updateToolbarPosition);

    // Observe toolbar content height changes (flex-wrap grows/shrinks)
    const toolbarRoot = DomAdapter.querySelector(`.${C.toolbarRoot}`);
    if (toolbarRoot && typeof ResizeObserver !== 'undefined') {
      new ResizeObserver(() => {
        syncToolbarHeight();
        updateToolbarPosition();
      }).observe(toolbarRoot);
    }

    // === Event handlers ===
    // Iterate via StaticData instead of DOM query
    StaticData.getAllNodeIds().forEach((nodeId) => {
      const node = DomAdapter.getNode(nodeId);
      if (!node) return;

      node.addEventListener('click', (e) => {
        e.stopPropagation();
        highlightNode(nodeId);
      });

      node.addEventListener('mouseenter', () =>
        handleMouseEnter('node', nodeId),
      );
      node.addEventListener('mouseleave', handleMouseLeave);

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
        toggleCollapseAll();
        updateToolbarPosition();
      },
    );
    DomAdapter.querySelector('#crate-dep-checkbox')
      ?.closest('label')
      ?.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleCrateDepVisibility();
      });

    DomAdapter.querySelector('#module-dep-checkbox')
      ?.closest('label')
      ?.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleModuleDepVisibility();
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

      hitarea.addEventListener('mouseleave', () => {
        handleMouseLeave();
      });
    });

    DomAdapter.getSvgRoot().addEventListener('click', () => {
      AppState.clearPinned(appState);
      AppState.clearHover(appState);
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
        if (e.target.classList.contains('sidebar-close')) {
          AppState.clearPinned(appState);
          AppState.clearHover(appState);
          highlightTiming.immediate();
          SidebarLogic.hide();
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

    // Sync toolbar foreignObject height with actual content
    syncToolbarHeight();
  })();
}
