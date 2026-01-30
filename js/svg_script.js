// @module SvgScript
// @deps ArcLogic, StaticData, AppState, Selectors, DomAdapter, LayerManager, TreeLogic, DerivedState, HighlightLogic, VirtualEdgeLogic, TextMetrics
// @config ROW_HEIGHT, MARGIN, TOOLBAR_HEIGHT
// svg_script.js - DOM code for interactive SVG
// ArcLogic is loaded from arc_logic.js before this file
// Placeholders replaced at runtime: __ROW_HEIGHT__, __MARGIN__, __TOOLBAR_HEIGHT__

// IIFE for SVG embedding (DOM-code) - only runs in browser with placeholders replaced
if (typeof document !== 'undefined') {
(function() {
  const ROW_HEIGHT = __ROW_HEIGHT__;
  const MARGIN = __MARGIN__;
  const TOOLBAR_HEIGHT = __TOOLBAR_HEIGHT__;

  // === Arc weight scaling ===

  /**
   * Scale existing arrows by updating their points attribute.
   * Parses current tip position from existing points.
   */
  function scaleArrow(edgeId, strokeWidth) {
    const scale = ArcLogic.scaleFromStrokeWidth(strokeWidth);
    DomAdapter.getVisibleArrows(edgeId).forEach(arrow => {
      const points = arrow.getAttribute('points');
      const tip = ArcLogic.parseTipFromPoints(points);
      if (tip) {
        arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, scale));
      }
    });
  }

  function applyInitialArcWeights() {
    // Apply stroke-width from StaticData (no state storage needed - values are calculated on demand)
    for (const arcId of StaticData.getAllArcIds()) {
      const width = StaticData.getArcStrokeWidth(arcId);
      const visibleArc = DomAdapter.getVisibleArc(arcId);
      if (visibleArc) visibleArc.style.strokeWidth = width + 'px';
      scaleArrow(arcId, width);
    }
  }

  // === Floating label for source locations ===
  let floatingLabel = null;

  function showFloatingLabel(text, x, y) {
    hideFloatingLabel();
    const svg = DomAdapter.getSvgRoot();
    floatingLabel = DomAdapter.createSvgElement('g');
    floatingLabel.setAttribute('class', 'floating-label');

    const padding = 6;
    const lineHeight = 14;
    const lines = text.split('|');

    const textEl = DomAdapter.createSvgElement('text');
    textEl.setAttribute('x', x + padding);
    textEl.setAttribute('y', y + lineHeight);

    // Create tspan for each line
    lines.forEach((line, i) => {
      const tspan = DomAdapter.createSvgElement('tspan');
      tspan.setAttribute('x', x + padding);
      tspan.setAttribute('dy', i === 0 ? 0 : lineHeight);
      tspan.textContent = line;
      textEl.appendChild(tspan);
    });

    // Estimate width using TextMetrics (no DOM read needed)
    const textWidth = TextMetrics.estimateMultilineWidth(text, 11);
    const labelWidth = textWidth + padding * 2;
    const labelHeight = lines.length * lineHeight + padding;

    const rect = DomAdapter.createSvgElement('rect');
    rect.setAttribute('x', x);
    rect.setAttribute('y', y);
    rect.setAttribute('width', labelWidth);
    rect.setAttribute('height', labelHeight);

    floatingLabel.appendChild(rect);
    floatingLabel.appendChild(textEl);
    svg.appendChild(floatingLabel);
  }

  function hideFloatingLabel() {
    if (floatingLabel) { floatingLabel.remove(); floatingLabel = null; }
  }

  // === Highlight functionality ===
  // Use AppState module for unified state management
  const appState = AppState.create();

  /**
   * Reset arc and arrows to original styling.
   * @param {string} arcId - Arc identifier
   * @param {boolean} isVirtual - True for virtual arcs (aggregated), false for regular
   * @param {string|undefined} aggregatedLocs - Pipe-separated locations for virtual arcs
   */
  function resetArcToOriginal(arcId, isVirtual, aggregatedLocs) {
    // Determine strokeWidth and scale from StaticData (no stored state needed)
    let strokeWidth, scale;
    if (isVirtual) {
      // Virtual arcs: calculate from aggregated locations
      const count = ArcLogic.countLocations(aggregatedLocs);
      strokeWidth = ArcLogic.calculateStrokeWidth(count);
      scale = strokeWidth / 1.5;
    } else {
      // Regular arcs: calculate from StaticData
      strokeWidth = StaticData.getArcStrokeWidth(arcId);
      scale = ArcLogic.scaleFromStrokeWidth(strokeWidth);
    }

    // Reset arc stroke-width
    const arcSelector = isVirtual ? `.virtual-arc[data-arc-id="${arcId}"]` : null;
    if (isVirtual) {
      DomAdapter.querySelectorAll(arcSelector).forEach(arc => {
        arc.style.strokeWidth = strokeWidth + 'px';
      });
    } else {
      const visibleArc = DomAdapter.getVisibleArc(arcId);
      if (visibleArc) visibleArc.style.strokeWidth = strokeWidth + 'px';
    }

    // Reset arrow scale (keep current position)
    const arrows = isVirtual
      ? DomAdapter.querySelectorAll(`.virtual-arrow[data-vedge="${arcId}"]`)
      : DomAdapter.getVisibleArrows(arcId);
    arrows.forEach(arrow => {
      const tip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
      if (tip) {
        arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, scale));
      }
    });
  }

  function clearHighlights() {
    // Clear shadow paths
    LayerManager.clearLayer(LayerManager.LAYERS.SHADOWS, DomAdapter);

    // Move highlighted elements back to base layers
    DomAdapter.querySelectorAll(Selectors.highlightedArcs()).forEach(el => {
      LayerManager.moveToBaseLayer(el, DomAdapter);
    });
    DomAdapter.querySelectorAll(Selectors.highlightedLabels()).forEach(el => {
      LayerManager.moveToBaseLayer(el, DomAdapter);
    });
    DomAdapter.querySelectorAll(Selectors.highlightedHitareas()).forEach(el => {
      LayerManager.moveToBaseLayer(el, DomAdapter);
    });

    // Reset regular arcs to original size
    DomAdapter.querySelectorAll('.arc-hitarea:not(.virtual-hitarea)').forEach(hitarea => {
      resetArcToOriginal(hitarea.dataset.arcId, false);
    });

    // Reset virtual arcs to original size (recalculated from aggregated locations)
    DomAdapter.querySelectorAll('.virtual-hitarea').forEach(hitarea => {
      resetArcToOriginal(hitarea.dataset.arcId, true, hitarea.dataset.sourceLocations);
    });

    // Remove CSS classes
    DomAdapter.querySelectorAll('.selected-crate, .selected-module, .dep-node, .dependent-node, .highlighted-arc, .highlighted-arrow, .highlighted-label, .dimmed')
      .forEach(el => el.classList.remove('selected-crate', 'selected-module', 'dep-node', 'dependent-node', 'highlighted-arc', 'highlighted-arrow', 'highlighted-label', 'dimmed'));
  }

  // Create shadow path for glow effect
  // relationType: 'dep' (incoming) or 'reverse' (outgoing)
  // arcId: used to look up original width
  // originalArcWidth: un-highlighted arc width (before calculateHighlightWidth)
  function createShadowPath(arc, relationType, arcId, originalArcWidth) {
    if (!arc) return;
    const shadow = arc.cloneNode(false);
    shadow.classList.add('shadow-path');
    shadow.classList.add(relationType === 'dep' ? 'glow-incoming' : 'glow-outgoing');
    shadow.removeAttribute('id');

    // Use provided original width (prevents shadow growth on repeated hover)
    // Estimate path length from path data (no DOM read)
    const pathLength = ArcLogic.estimatePathLength(arc.getAttribute('d'));
    const shadowData = HighlightLogic.calculateShadowData(originalArcWidth, pathLength);

    shadow.style.strokeWidth = shadowData.shadowWidth + 'px';
    shadow.setAttribute('opacity', '0.25');
    shadow.style.strokeLinecap = 'round';
    shadow.style.strokeDasharray = shadowData.visibleLength + ' ' + pathLength;
    shadow.style.strokeDashoffset = shadowData.dashOffset + 'px';

    const shadowLayer = DomAdapter.getElementById(LayerManager.LAYERS.SHADOWS);
    if (shadowLayer) shadowLayer.appendChild(shadow);
  }

  /**
   * Highlight a single arc element with correct sequencing.
   * CRITICAL: Calculates original width from source data to prevent growth bug.
   * @param {Element} arc - Arc DOM element
   * @param {string} arcId - Arc identifier (from-to)
   * @param {string} relationType - 'dep' (outgoing) or 'reverse' (incoming)
   * @returns {number} highlightWidth - For arrow scaling
   */
  function highlightArcElement(arc, arcId, relationType) {
    // 1. Calculate ORIGINAL width from source data (not from DOM to prevent growth bug)
    let arcWidth;
    if (arc.classList.contains('virtual-arc')) {
      // Virtual arc: find hitarea and calculate from sourceLocations
      const hitarea = DomAdapter.querySelector(`.virtual-hitarea[data-arc-id="${arcId}"]`);
      const sourceLocations = hitarea?.dataset.sourceLocations;
      const count = ArcLogic.countLocations(sourceLocations);
      arcWidth = ArcLogic.calculateStrokeWidth(count);
    } else {
      // Regular arc: use StaticData
      arcWidth = StaticData.getArcStrokeWidth(arcId);
    }

    const highlightWidth = HighlightLogic.calculateHighlightWidth(arcWidth);

    // 2. Apply highlighting
    arc.classList.add('highlighted-arc');
    arc.style.strokeWidth = highlightWidth + 'px';

    // 3. Create shadow using ORIGINAL width
    createShadowPath(arc, relationType, arcId, arcWidth);

    return highlightWidth;
  }

  // Dim all non-highlighted elements (except toolbar and hitareas)
  function dimNonHighlighted() {
    DomAdapter.querySelectorAll(
      'rect:not(.selected-crate):not(.selected-module):not(.dep-node):not(.dependent-node):not(.toolbar-btn):not(.toolbar-checkbox):not(.arc-count-bg), ' +
      'path:not(.highlighted-arc):not(.arc-hitarea):not(.virtual-hitarea), ' +
      'polygon:not(.highlighted-arrow), ' +
      'text.arc-count:not(.highlighted-label)'
    ).forEach(el => {
      if (!el.closest('.view-options')) el.classList.add('dimmed');
    });
  }

  function applyEdgeHighlight(from, to) {
    const arcId = from + '-' + to;
    const arc = DomAdapter.getVisibleArc(arcId);

    // Check if arc is filtered out (user checkbox) - skip completely
    if (arc?.classList.contains('hidden-by-filter')) return;

    // Node borders (always apply)
    DomAdapter.getNode(from)?.classList.add('dependent-node');
    DomAdapter.getNode(to)?.classList.add('dep-node');

    // Regular arc highlighting (skip if collapsed)
    const isCollapsed = arc?.style.display === 'none';
    if (!isCollapsed && arc) {
      const highlightWidth = highlightArcElement(arc, arcId, 'dep');
      scaleArrow(arcId, highlightWidth);

      // Regular arrows
      DomAdapter.getVisibleArrows(arcId)
        .forEach(el => el.classList.add('highlighted-arrow'));
    }

    // Virtual arcs (exist when regular arc is collapsed)
    DomAdapter.querySelectorAll(Selectors.virtualArc(from, to))
      .forEach(el => {
        const vHighlightWidth = highlightArcElement(el, arcId, 'dep');

        // Scale virtual arrows (use virtual arc width, not regular arc width)
        DomAdapter.querySelectorAll(`.virtual-arrow[data-vedge="${arcId}"]`).forEach(arrow => {
          arrow.classList.add('highlighted-arrow');
          const tip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
          if (tip) {
            arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, HighlightLogic.calculateVirtualArrowScale(vHighlightWidth)));
          }
        });
      });

    // Arc-count labels
    DomAdapter.querySelectorAll('.arc-count[data-vedge="' + arcId + '"]')
      .forEach(el => el.classList.add('highlighted-label'));

    // Move highlighted elements to highlight layers
    LayerManager.moveToHighlightLayer(arc, DomAdapter);
    LayerManager.moveToHighlightLayer(DomAdapter.querySelector(`.virtual-arc[data-arc-id="${arcId}"]`), DomAdapter);
    LayerManager.moveToHighlightLayer(DomAdapter.getLabelGroup(arcId), DomAdapter);
    // Arrows (only visible ones)
    DomAdapter.getVisibleArrows(arcId).forEach(el => LayerManager.moveToHighlightLayer(el, DomAdapter));
    DomAdapter.getVirtualArrows(arcId).forEach(el => LayerManager.moveToHighlightLayer(el, DomAdapter));

    // Move hitarea to highlight layer (higher z-order) so it receives events over dimmed hitareas
    const hitarea = DomAdapter.getHitarea(arcId);
    LayerManager.moveToLayer(hitarea, LayerManager.LAYERS.HIGHLIGHT_HITAREAS, DomAdapter);

    dimNonHighlighted();
  }

  function applyNodeHighlight(nodeId) {
    // Selected node: saturated original color
    const selectedNode = DomAdapter.getNode(nodeId);
    if (selectedNode) {
      if (selectedNode.classList.contains('crate')) {
        selectedNode.classList.add('selected-crate');
      } else if (selectedNode.classList.contains('module')) {
        selectedNode.classList.add('selected-module');
      }
    }

    // Regular arcs via hitareas
    DomAdapter.getConnectedHitareas(nodeId)
      .forEach(hitarea => {
        // Skip hitareas filtered out by user (e.g., CrateDeps checkbox)
        // Note: We check visibleArc for display:none (collapsed), not hitarea
        if (hitarea.classList.contains('hidden-by-filter')) return;

        const arcId = hitarea.dataset.arcId;
        const visibleArc = DomAdapter.getVisibleArc(arcId);

        // Skip if arc is hidden (endpoints are collapsed)
        if (visibleArc?.style.display === 'none') return;

        const from = hitarea.dataset.from;
        const to = hitarea.dataset.to;
        const relationType = HighlightLogic.determineRelationType(from, to, nodeId);

        // Only proceed if arc exists and is visible (getVisibleArc returns null for hidden arcs)
        if (!visibleArc) return;

        const highlightWidth = highlightArcElement(visibleArc, arcId, relationType);
        if (highlightWidth > 0) scaleArrow(from + '-' + to, highlightWidth);

        // Connected nodes (border only)
        const otherNodeId = relationType === 'dep' ? to : from;
        DomAdapter.getNode(otherNodeId)?.classList.add(relationType === 'dep' ? 'dep-node' : 'dependent-node');

        // Arrows: marker class (keeps direction color)
        DomAdapter.getVisibleArrows(from + '-' + to)
          .forEach(arr => arr.classList.add('highlighted-arrow'));

        // Move to highlight layers
        LayerManager.moveToHighlightLayer(visibleArc, DomAdapter);
        LayerManager.moveToHighlightLayer(DomAdapter.getLabelGroup(arcId), DomAdapter);
        DomAdapter.getVisibleArrows(arcId).forEach(el => LayerManager.moveToHighlightLayer(el, DomAdapter));
      });

    // Virtual arcs
    DomAdapter.querySelectorAll('.virtual-arc[data-from="' + nodeId + '"], .virtual-arc[data-to="' + nodeId + '"]')
      .forEach(arc => {
        const from = arc.dataset.from;
        const to = arc.dataset.to;
        const arcId = from + '-' + to;
        const relationType = HighlightLogic.determineRelationType(from, to, nodeId);

        const highlightWidth = highlightArcElement(arc, arcId, relationType);

        // Scale virtual arrows
        DomAdapter.querySelectorAll(`.virtual-arrow[data-vedge="${arcId}"]`).forEach(arrow => {
          const tip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
          if (tip) {
            arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, HighlightLogic.calculateVirtualArrowScale(highlightWidth)));
          }
        });

        // Connected nodes (border only)
        const otherNodeId = relationType === 'dep' ? to : from;
        DomAdapter.getNode(otherNodeId)?.classList.add(relationType === 'dep' ? 'dep-node' : 'dependent-node');

        // Arrows: marker class (keeps direction color)
        DomAdapter.getVirtualArrows(arcId)
          .forEach(arr => arr.classList.add('highlighted-arrow'));

        // Arc-count labels
        DomAdapter.querySelectorAll('.arc-count[data-vedge="' + arcId + '"]')
          .forEach(el => el.classList.add('highlighted-label'));

        // Move to highlight layers
        LayerManager.moveToHighlightLayer(arc, DomAdapter);
        LayerManager.moveToHighlightLayer(DomAdapter.getLabelGroup(arcId), DomAdapter);
        DomAdapter.getVirtualArrows(arcId).forEach(el => LayerManager.moveToHighlightLayer(el, DomAdapter));
      });

    // Move connected hitareas to highlight layer (higher z-order)
    DomAdapter.getConnectedHitareas(nodeId)
      .forEach(h => LayerManager.moveToLayer(h, LayerManager.LAYERS.HIGHLIGHT_HITAREAS, DomAdapter));

    dimNonHighlighted();
  }

  /**
   * Highlight a group of nodes (primary + descendants) and their connected arcs.
   * Used when a pinned node is expanded — all visible descendants get highlighted too.
   * @param {string} primaryNodeId - The pinned node
   * @param {Set<string>} highlightSet - Set of node IDs to highlight (primary + descendants)
   */
  function applyNodeGroupHighlight(primaryNodeId, highlightSet) {
    // Primary node: saturated original color
    const selectedNode = DomAdapter.getNode(primaryNodeId);
    if (selectedNode) {
      if (selectedNode.classList.contains('crate')) {
        selectedNode.classList.add('selected-crate');
      } else if (selectedNode.classList.contains('module')) {
        selectedNode.classList.add('selected-module');
      }
    }

    const processedArcs = new Set();

    // For each node in the highlight set, process connected arcs
    for (const nodeId of highlightSet) {
      // Regular arcs via hitareas
      DomAdapter.getConnectedHitareas(nodeId)
        .forEach(hitarea => {
          if (hitarea.classList.contains('hidden-by-filter')) return;

          const arcId = hitarea.dataset.arcId;
          if (processedArcs.has(arcId)) return;
          processedArcs.add(arcId);

          const visibleArc = DomAdapter.getVisibleArc(arcId);
          if (visibleArc?.style.display === 'none') return;
          if (!visibleArc) return;

          const from = hitarea.dataset.from;
          const to = hitarea.dataset.to;

          // Determine relation type relative to the SET
          const fromInSet = highlightSet.has(from);
          const toInSet = highlightSet.has(to);
          const relationType = fromInSet && !toInSet ? 'dep'
            : !fromInSet && toInSet ? 'reverse'
            : 'dep'; // both in set → internal, use dep convention

          const highlightWidth = highlightArcElement(visibleArc, arcId, relationType);
          if (highlightWidth > 0) scaleArrow(from + '-' + to, highlightWidth);

          // External nodes (not in set) get border
          if (!fromInSet) {
            DomAdapter.getNode(from)?.classList.add('dependent-node');
          }
          if (!toInSet) {
            DomAdapter.getNode(to)?.classList.add('dep-node');
          }

          DomAdapter.getVisibleArrows(from + '-' + to)
            .forEach(arr => arr.classList.add('highlighted-arrow'));

          LayerManager.moveToHighlightLayer(visibleArc, DomAdapter);
          LayerManager.moveToHighlightLayer(DomAdapter.getLabelGroup(arcId), DomAdapter);
          DomAdapter.getVisibleArrows(arcId).forEach(el => LayerManager.moveToHighlightLayer(el, DomAdapter));
        });

      // Virtual arcs
      DomAdapter.querySelectorAll('.virtual-arc[data-from="' + nodeId + '"], .virtual-arc[data-to="' + nodeId + '"]')
        .forEach(arc => {
          const from = arc.dataset.from;
          const to = arc.dataset.to;
          const arcId = from + '-' + to;
          if (processedArcs.has('v:' + arcId)) return;
          processedArcs.add('v:' + arcId);

          const fromInSet = highlightSet.has(from);
          const toInSet = highlightSet.has(to);
          const relationType = fromInSet && !toInSet ? 'dep'
            : !fromInSet && toInSet ? 'reverse'
            : 'dep';

          const highlightWidth = highlightArcElement(arc, arcId, relationType);

          DomAdapter.querySelectorAll(`.virtual-arrow[data-vedge="${arcId}"]`).forEach(arrow => {
            const tip = ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
            if (tip) {
              arrow.setAttribute('points', ArrowLogic.getArrowPoints(tip, HighlightLogic.calculateVirtualArrowScale(highlightWidth)));
            }
          });

          if (!fromInSet) {
            DomAdapter.getNode(from)?.classList.add('dependent-node');
          }
          if (!toInSet) {
            DomAdapter.getNode(to)?.classList.add('dep-node');
          }

          DomAdapter.getVirtualArrows(arcId)
            .forEach(arr => arr.classList.add('highlighted-arrow'));

          DomAdapter.querySelectorAll('.arc-count[data-vedge="' + arcId + '"]')
            .forEach(el => el.classList.add('highlighted-label'));

          LayerManager.moveToHighlightLayer(arc, DomAdapter);
          LayerManager.moveToHighlightLayer(DomAdapter.getLabelGroup(arcId), DomAdapter);
          DomAdapter.getVirtualArrows(arcId).forEach(el => LayerManager.moveToHighlightLayer(el, DomAdapter));
        });

      // Move connected hitareas to highlight layer
      DomAdapter.getConnectedHitareas(nodeId)
        .forEach(h => LayerManager.moveToLayer(h, LayerManager.LAYERS.HIGHLIGHT_HITAREAS, DomAdapter));
    }

    dimNonHighlighted();
  }

  function highlightEdge(from, to, pin) {
    const edgeId = from + '-' + to;
    if (pin) {
      // Toggle: if same edge is already pinned, deselect
      const wasPinned = AppState.togglePinned(appState, 'edge', edgeId);
      clearHighlights();
      if (!wasPinned) return; // Was unpinned, done
    } else {
      clearHighlights();
    }
    applyEdgeHighlight(from, to);
  }

  function highlightNode(nodeId, pin) {
    if (pin) {
      // Toggle: if same node is already pinned, deselect
      const wasPinned = AppState.togglePinned(appState, 'node', nodeId);
      clearHighlights();
      if (!wasPinned) return; // Was unpinned, done
    } else {
      clearHighlights();
    }
    const highlightSet = DerivedState.deriveHighlightSet(nodeId, appState.collapsed, StaticData);
    if (highlightSet.size === 1) {
      applyNodeHighlight(nodeId);
    } else {
      applyNodeGroupHighlight(nodeId, highlightSet);
    }
  }

  function handleMouseEnter(type, id) {
    if (AppState.getPinned(appState)) return; // Don't preview if something is pinned
    clearHighlights();
    if (type === 'node') applyNodeHighlight(id);
    else if (type === 'edge') {
      const [from, to] = id.split('-');
      applyEdgeHighlight(from, to);
    }
  }

  function handleMouseLeave() {
    if (AppState.getPinned(appState)) return; // Keep pinned highlight
    clearHighlights();
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
    return TreeLogic.getVisibleAncestor(nodeId, appState.collapsed, parentMap);
  }

  function countDescendants(nodeId) {
    return TreeLogic.countDescendants(nodeId, parentMap);
  }

  // Update tree lines for a node at new Y position
  function updateTreeLines(nodeId, newY, nodeHeight) {
    // Update lines where this node is the child
    DomAdapter.getTreeLines(nodeId, 'child').forEach(line => {
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
    DomAdapter.getTreeLines(nodeId, 'parent').forEach(line => {
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
    const sortedIds = StaticData.getAllNodeIds()
      .sort((a, b) => {
        const posA = StaticData.getOriginalPosition(a);
        const posB = StaticData.getOriginalPosition(b);
        return posA.y - posB.y;
      });

    sortedIds.forEach(nodeId => {
      const node = DomAdapter.getNode(nodeId);
      if (!node) return;
      if (node.classList.contains('collapsed')) return;
      if (node.classList.contains('hidden-by-filter')) return;

      // Get height from StaticData (no DOM read)
      const height = StaticData.getOriginalPosition(nodeId).height;

      // Update rect position
      node.setAttribute('y', currentY);

      // Update label position (next text sibling)
      const label = node.nextElementSibling;
      if (label && label.tagName === 'text' && label.classList.contains('label')) {
        label.setAttribute('y', currentY + height / 2 + 4);
      }

      // Update toggle icon position (if exists)
      const toggle = DomAdapter.getCollapseToggle(nodeId);
      if (toggle) {
        toggle.setAttribute('y', currentY + height / 2 + 4);
      }

      // Update tree lines
      updateTreeLines(nodeId, currentY, height);

      currentY += ROW_HEIGHT;
    });

    recalculateVirtualEdges();

    // Re-apply pinned highlight after edges were recreated
    const pinned = AppState.getPinned(appState);
    if (pinned) {
      clearHighlights();  // Remove stale shadow paths from deleted virtual arcs
      if (pinned.type === 'node') {
        const highlightSet = DerivedState.deriveHighlightSet(pinned.id, appState.collapsed, StaticData);
        if (highlightSet.size === 1) {
          applyNodeHighlight(pinned.id);
        } else {
          applyNodeGroupHighlight(pinned.id, highlightSet);
        }
      } else if (pinned.type === 'edge') {
        const [from, to] = pinned.id.split('-');
        applyEdgeHighlight(from, to);
      }
    }
  }

  // Helper: Calculate arc path from position objects (no DOM read)
  function calculateArcPathFromPositions(fromPos, toPos, yOffset, maxRight) {
    const fromX = fromPos.x + fromPos.width;
    const fromY = fromPos.y + fromPos.height / 2 + yOffset;
    const toX = toPos.x + toPos.width;
    const toY = toPos.y + toPos.height / 2 - yOffset;
    return ArcLogic.calculateArcPath(fromX, fromY, toX, toY, maxRight, ROW_HEIGHT);
  }

  // Helper: Extract edge data from DOM hitareas to pure objects
  // Uses DerivedState for visibility instead of per-edge getVisibleAncestor calls
  function extractEdgeData(hitareas, visibleNodes) {
    const edges = [];
    hitareas.forEach(hitarea => {
      const fromId = hitarea.dataset.from;
      const toId = hitarea.dataset.to;
      const fromNode = DomAdapter.getNode(fromId);
      const toNode = DomAdapter.getNode(toId);

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
        fromNode,
        toNode,
        fromHidden: fromIsHidden,
        toHidden: toIsHidden,
        sourceLocations: StaticData.getFormattedUsages(arcId),
        // Compute direction from hierarchy (no DOM read)
        direction: DerivedState._determineDirection(fromId, toId, parentMap)
      });
    });
    return edges;
  }

  // Remove virtual elements and reset original edge display
  function cleanupVirtualElements() {
    DomAdapter.querySelectorAll(Selectors.allVirtualElements()).forEach(el => el.remove());
    DomAdapter.querySelectorAll(Selectors.allBaseEdges()).forEach(edge => {
      edge.style.display = '';
    });
    DomAdapter.querySelectorAll(Selectors.allBaseArrows()).forEach(arrow => {
      arrow.style.display = '';
    });
  }

  // Hide original elements when from/to hidden, update visible arc paths
  function updateOriginalEdges(edgeData, currentPositions, maxRight) {
    edgeData.forEach(edge => {
      const { hitarea, arcId, fromId, toId, fromHidden, toHidden } = edge;

      if (window.DEBUG_ARCS) {
        console.log(`Arc ${arcId}: from=${fromId}(${fromHidden ? 'hidden' : 'visible'}), to=${toId}(${toHidden ? 'hidden' : 'visible'})`);
      }

      if (fromHidden || toHidden) {
        // Hide original elements
        hitarea.style.display = 'none';
        const visibleArc = DomAdapter.getVisibleArc(arcId);
        if (visibleArc) visibleArc.style.display = 'none';
        DomAdapter.getArrows(fromId + '-' + toId)
          .forEach(arr => arr.style.display = 'none');
      } else {
        // Update visible arc paths using computed positions (no DOM read)
        const fromPos = currentPositions.get(fromId);
        const toPos = currentPositions.get(toId);
        if (fromPos && toPos) {
          const arc = calculateArcPathFromPositions(fromPos, toPos, 3, maxRight);
          hitarea.setAttribute('d', arc.path);
          const visibleArc = DomAdapter.getVisibleArc(arcId);
          if (visibleArc) visibleArc.setAttribute('d', arc.path);

          const strokeWidth = visibleArc ? parseFloat(visibleArc.style.strokeWidth) || 0.5 : 0.5;
          const scale = strokeWidth / 1.5;
          // Update ALL arrow positions (even hidden ones) so they have correct position when shown
          DomAdapter.getArrows(fromId + '-' + toId).forEach(arrow => {
            arrow.setAttribute('points', ArcLogic.getArrowPoints({ x: arc.toX, y: arc.toY }, scale));
          });
        }
      }
    });
  }

  // Recalculate and show virtual edges for collapsed nodes
  function recalculateVirtualEdges() {
    cleanupVirtualElements();

    const visibleNodes = DerivedState.deriveNodeVisibility(appState.collapsed, StaticData);
    const currentPositions = DerivedState.computeCurrentPositions(
      appState.collapsed, StaticData, MARGIN, TOOLBAR_HEIGHT, ROW_HEIGHT
    );
    const maxRight = DerivedState.computeMaxRight(currentPositions);

    const hitareas = DomAdapter.getAllHitareas();
    const edgeData = extractEdgeData(hitareas, visibleNodes);

    updateOriginalEdges(edgeData, currentPositions, maxRight);

    const virtualEdges = VirtualEdgeLogic.aggregateHiddenEdges(edgeData, getVisibleAncestor);
    const mergedEdges = VirtualEdgeLogic.prepareVirtualEdgeData(
      virtualEdges, currentPositions, maxRight, ArcLogic, ROW_HEIGHT
    );

    const layers = {
      baseArcs: DomAdapter.getElementById(LayerManager.LAYERS.BASE_ARCS),
      baseLabels: DomAdapter.getElementById(LayerManager.LAYERS.BASE_LABELS),
      hitareas: DomAdapter.getElementById(LayerManager.LAYERS.HITAREAS)
    };
    renderVirtualElements(mergedEdges, layers);
  }

  // Create virtual arc elements (arcs, arrows, labels, hitareas)
  function renderVirtualElements(mergedEdges, layers) {
    // Pass 1: Arcs + Arrows (bottom layer)
    mergedEdges.forEach((data, key) => {
      const { fromId, toId, arc, strokeWidth, direction } = data;
      const arcId = fromId + '-' + toId;

      // Visible path (styled, no pointer events)
      const path = DomAdapter.createSvgElement('path');
      path.setAttribute('class', `virtual-arc ${direction}`);
      path.setAttribute('d', arc.path);
      path.setAttribute('data-arc-id', arcId);
      path.setAttribute('data-from', fromId);
      path.setAttribute('data-to', toId);
      path.style.strokeWidth = strokeWidth + 'px';
      layers.baseArcs.appendChild(path);

      // Arrow (scaled to match stroke width)
      const scale = strokeWidth / 1.5;
      const arrow = DomAdapter.createSvgElement('polygon');
      arrow.setAttribute('class', `virtual-arrow ${direction}`);
      arrow.setAttribute('data-vedge', arcId);
      arrow.setAttribute('data-from', fromId);
      arrow.setAttribute('data-to', toId);
      arrow.setAttribute('points', ArcLogic.getArrowPoints({ x: arc.toX, y: arc.toY }, scale));
      arrow.addEventListener('click', e => {
        e.stopPropagation();
        highlightVirtualEdge(fromId, toId, data.count);
      });
      layers.baseArcs.appendChild(arrow);
    });

    // Pass 2: Labels (middle layer, above arcs)
    mergedEdges.forEach((data, key) => {
      const { fromId, toId, arc, count } = data;

      if (count > 1) {
        const labelGroup = DomAdapter.createSvgElement('g');
        labelGroup.setAttribute('class', 'arc-count-group');
        labelGroup.setAttribute('data-vedge', fromId + '-' + toId);
        labelGroup.setAttribute('data-from', fromId);
        labelGroup.setAttribute('data-to', toId);
        const text = '(' + count + ')';
        const x = arc.ctrlX + 5;
        const y = arc.midY + 3;

        // Background rect (2-3px padding)
        const padding = 2;
        const textWidth = text.length * 6; // ~6px per char at 10px font
        const bg = DomAdapter.createSvgElement('rect');
        bg.setAttribute('class', 'arc-count-bg');
        bg.setAttribute('x', x - textWidth / 2 - padding);
        bg.setAttribute('y', y - 8 - padding);
        bg.setAttribute('width', textWidth + padding * 2);
        bg.setAttribute('height', 12 + padding * 2);
        bg.setAttribute('rx', '2');

        // Text label
        const countLabel = DomAdapter.createSvgElement('text');
        countLabel.setAttribute('class', 'arc-count');
        countLabel.setAttribute('data-vedge', fromId + '-' + toId);
        countLabel.setAttribute('x', x);
        countLabel.setAttribute('y', y);
        countLabel.textContent = text;

        labelGroup.appendChild(bg);
        labelGroup.appendChild(countLabel);
        labelGroup.style.cursor = 'pointer';
        labelGroup.addEventListener('click', e => {
          e.stopPropagation();
          highlightVirtualEdge(fromId, toId, count);
        });
        labelGroup.addEventListener('mouseenter', () => handleMouseEnter('edge', fromId + '-' + toId));
        labelGroup.addEventListener('mouseleave', handleMouseLeave);

        layers.baseLabels.appendChild(labelGroup);
      }
    });

    // Pass 3: Hitareas (hitareas layer, always on top)
    mergedEdges.forEach((data, key) => {
      const { fromId, toId, arc, hiddenEdgeData, count } = data;
      const arcId = fromId + '-' + toId;

      const hitarea = DomAdapter.createSvgElement('path');
      hitarea.setAttribute('class', 'virtual-hitarea arc-hitarea');
      hitarea.setAttribute('d', arc.path);
      hitarea.setAttribute('data-arc-id', arcId);
      hitarea.setAttribute('data-from', fromId);
      hitarea.setAttribute('data-to', toId);
      // Set aggregated source locations from hidden edges
      if (hiddenEdgeData.length > 0) {
        hitarea.dataset.sourceLocations = ArcLogic.sortAndGroupLocations(hiddenEdgeData);
      }
      hitarea.addEventListener('click', e => {
        e.stopPropagation();
        highlightVirtualEdge(fromId, toId, count);
      });
      hitarea.addEventListener('mouseenter', () => handleMouseEnter('edge', arcId));
      hitarea.addEventListener('mousemove', (e) => {
        const pinned = AppState.getPinned(appState);
        if (pinned) {
          const isHighlighted = pinned.type === 'edge'
            ? pinned.id === arcId
            : (fromId === pinned.id || toId === pinned.id);
          if (!isHighlighted) {
            hideFloatingLabel();
            return;
          }
        }
        const locs = hitarea.dataset.sourceLocations;
        if (locs) {
          const svg = DomAdapter.getSvgRoot();
          const rect = svg.getBoundingClientRect();
          const viewBox = svg.viewBox.baseVal;
          const svgPt = ArcLogic.getSvgCoords(e.clientX, e.clientY, rect, viewBox);
          showFloatingLabel(locs, svgPt.x + 10, svgPt.y - 20);
        }
      });
      hitarea.addEventListener('mouseleave', () => {
        handleMouseLeave();
        hideFloatingLabel();
      });
      layers.hitareas.appendChild(hitarea);
    });
  }

  // Highlight virtual (aggregated) edge
  function highlightVirtualEdge(fromId, toId, count) {
    const edgeId = fromId + '-' + toId;
    // Toggle: if same edge is already pinned, deselect
    const wasPinned = AppState.togglePinned(appState, 'edge', edgeId);
    clearHighlights();
    if (!wasPinned) return; // Was unpinned, done
    // from-Node: dependent (purple border)
    DomAdapter.getNode(fromId)?.classList.add('dependent-node');
    // to-Node: dep (green border)
    DomAdapter.getNode(toId)?.classList.add('dep-node');
    // Virtual arc: marker class (keeps direction color), dynamic stroke-width
    let highlightWidth = 0;
    DomAdapter.querySelectorAll(Selectors.virtualArc(fromId, toId))
      .forEach(el => {
        highlightWidth = highlightArcElement(el, edgeId, 'dep');
      });
    // Arrows: marker class (keeps direction color), scale to match arc
    DomAdapter.querySelectorAll('.virtual-arrow[data-vedge="' + edgeId + '"]')
      .forEach(el => {
        el.classList.add('highlighted-arrow');
        const tip = ArcLogic.parseTipFromPoints(el.getAttribute('points'));
        if (tip) {
          el.setAttribute('points', ArcLogic.getArrowPoints(tip, HighlightLogic.calculateVirtualArrowScale(highlightWidth)));
        }
      });
    // Arc-count labels
    DomAdapter.querySelectorAll('.arc-count[data-vedge="' + edgeId + '"]')
      .forEach(el => el.classList.add('highlighted-label'));
    // Move virtual arc and label group to highlight layers
    LayerManager.moveToHighlightLayer(DomAdapter.querySelector('.virtual-arc[data-arc-id="' + edgeId + '"]'), DomAdapter);
    LayerManager.moveToHighlightLayer(DomAdapter.getLabelGroup(edgeId), DomAdapter);
    DomAdapter.querySelectorAll('.virtual-arrow[data-vedge="' + edgeId + '"]').forEach(el => LayerManager.moveToHighlightLayer(el, DomAdapter));
    // Dim everything else
    dimNonHighlighted();
  }

  // Check if nodeId is an ancestor of targetId
  function isAncestorOf(nodeId, targetId) {
    let checkId = targetId;
    while (true) {
      const checkNode = DomAdapter.getNode(checkId);
      const parentId = checkNode?.dataset.parent;
      if (!parentId) return false;
      if (parentId === nodeId) return true;
      checkId = parentId;
    }
  }

  // --- Collapse helpers ---

  function updateDescendantVisibility(descId, collapsed) {
    const node = DomAdapter.getNode(descId);
    const label = node?.nextElementSibling;
    const toggle = DomAdapter.getCollapseToggle(descId);
    if (collapsed) {
      node?.classList.add('collapsed');
      label?.classList.add('collapsed');
      toggle?.classList.add('collapsed');
    } else {
      node?.classList.remove('collapsed');
      label?.classList.remove('collapsed');
      toggle?.classList.remove('collapsed');
    }
    DomAdapter.getTreeLines(descId, 'child').forEach(line => {
      if (collapsed) {
        line.classList.add('collapsed');
      } else if (!node?.classList.contains('collapsed')) {
        line.classList.remove('collapsed');
      }
    });
  }

  function hasCollapsedAncestor(descId, excludeNodeId) {
    let checkId = descId;
    while (true) {
      const checkNode = DomAdapter.getNode(checkId);
      const parentId = checkNode?.dataset.parent;
      if (!parentId) return false;
      if (AppState.isCollapsed(appState, parentId) && parentId !== excludeNodeId) {
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
      nodeRect.setAttribute('data-original-width', nodeRect.getAttribute('width'));
    }
    if (collapsed) {
      countLabel.textContent = ' (+' + countDescendants(nodeId) + ')';
      const labelText = countLabel.parentElement;
      if (labelText) {
        const textWidth = TextMetrics.estimateWidth(labelText.textContent, 12);
        const padding = 20;
        const neededWidth = textWidth + padding;
        const originalWidth = parseFloat(nodeRect.getAttribute('data-original-width'));
        nodeRect.setAttribute('width', Math.max(originalWidth, neededWidth));
      }
    } else {
      countLabel.textContent = '';
      const originalWidth = nodeRect.getAttribute('data-original-width');
      if (originalWidth) {
        nodeRect.setAttribute('width', originalWidth);
      }
    }
  }

  // Toggle collapse state
  function toggleCollapse(nodeId) {
    // Only clear visual highlights - relayout() will re-apply for pinned nodes
    clearHighlights();

    const collapsed = AppState.toggleCollapsed(appState, nodeId);

    getDescendants(nodeId).forEach(descId => {
      if (collapsed || !hasCollapsedAncestor(descId, nodeId)) {
        updateDescendantVisibility(descId, collapsed);
      }
    });

    updateParentNodeUI(nodeId, collapsed);
    relayout();
  }

  // Toggle collapse/expand all parent nodes
  function toggleCollapseAll() {
    // Only clear visual highlights - relayout() will re-apply for pinned nodes
    clearHighlights();

    const parentNodeIds = StaticData.getAllNodeIds().filter(id => StaticData.hasChildren(id));
    const allExpanded = parentNodeIds.every(id => !AppState.isCollapsed(appState, id));
    const collapsed = allExpanded;

    parentNodeIds.forEach(nodeId => {
      AppState.setCollapsed(appState, nodeId, collapsed);
      getDescendants(nodeId).forEach(descId => {
        updateDescendantVisibility(descId, collapsed);
      });
      updateParentNodeUI(nodeId, collapsed);
    });

    const label = DomAdapter.getElementById('collapse-toggle-label');
    if (label) label.textContent = collapsed ? 'Expand All' : 'Collapse All';

    relayout();
  }

  // Toggle visibility of crate-to-crate dependency arcs
  function toggleCrateDepVisibility() {
    const checkbox = DomAdapter.querySelector('#crate-dep-checkbox');
    if (!checkbox) return;

    const isChecked = checkbox.classList.toggle('checked');

    DomAdapter.querySelectorAll('.crate-dep-arc').forEach(arc => {
      if (isChecked) {
        arc.classList.remove('hidden-by-filter');
      } else {
        arc.classList.add('hidden-by-filter');
      }
    });

    // Also hide/show associated hitareas and arrows
    DomAdapter.querySelectorAll('.arc-hitarea').forEach(hitarea => {
      const arcId = hitarea.dataset.arcId;
      const visibleArc = DomAdapter.querySelector(`.crate-dep-arc[data-arc-id="${arcId}"]`);
      if (visibleArc) {
        if (isChecked) {
          hitarea.classList.remove('hidden-by-filter');
        } else {
          hitarea.classList.add('hidden-by-filter');
        }
        // Also handle arrows
        DomAdapter.getArrows(arcId).forEach(arrow => {
          if (isChecked) {
            arrow.classList.remove('hidden-by-filter');
          } else {
            arrow.classList.add('hidden-by-filter');
          }
        });
      }
    });
  }

  // Update toolbar position to stay at top when scrolling
  function updateToolbarPosition() {
    const toolbar = DomAdapter.querySelector('.view-options');
    const svg = DomAdapter.getSvgRoot();
    if (!toolbar || !svg) return;

    const rect = svg.getBoundingClientRect();
    const scrollTop = Math.max(0, -rect.top);
    toolbar.setAttribute('transform', `translate(0, ${scrollTop})`);
  }

  window.addEventListener('scroll', updateToolbarPosition);

  // === Event handlers ===
  // Iterate via StaticData instead of DOM query
  StaticData.getAllNodeIds().forEach(nodeId => {
    const node = DomAdapter.getNode(nodeId);
    if (!node) return;

    node.addEventListener('click', e => {
      e.stopPropagation();
      highlightNode(nodeId, true); // pin
    });

    node.addEventListener('mouseenter', () => handleMouseEnter('node', nodeId));
    node.addEventListener('mouseleave', handleMouseLeave);

    // Double-click to toggle collapse (only for parents)
    if (StaticData.hasChildren(nodeId)) {
      node.addEventListener('dblclick', e => {
        e.stopPropagation();
        toggleCollapse(nodeId);
        updateToolbarPosition();
      });
    }
  });

  DomAdapter.querySelectorAll('.collapse-toggle').forEach(toggle => {
    toggle.addEventListener('click', e => {
      e.stopPropagation();
      toggleCollapse(toggle.dataset.target);
      updateToolbarPosition();
    });
  });

  // Toolbar button event handlers
  DomAdapter.querySelector('#collapse-toggle-btn')?.addEventListener('click', e => {
    e.stopPropagation();
    toggleCollapseAll();
    updateToolbarPosition();
  });
  DomAdapter.querySelector('#collapse-toggle-label')?.addEventListener('click', e => {
    e.stopPropagation();
    toggleCollapseAll();
    updateToolbarPosition();
  });

  DomAdapter.querySelector('#crate-dep-checkbox')?.addEventListener('click', e => {
    e.stopPropagation();
    toggleCrateDepVisibility();
  });

  // Event handlers on hit-area paths (invisible, 12px wide) — regular arcs only
  DomAdapter.querySelectorAll('.arc-hitarea:not(.virtual-hitarea)').forEach(hitarea => {
    const edgeId = hitarea.dataset.from + '-' + hitarea.dataset.to;

    hitarea.addEventListener('click', e => {
      e.stopPropagation();
      highlightEdge(hitarea.dataset.from, hitarea.dataset.to, true); // pin
    });

    hitarea.addEventListener('mouseenter', () => handleMouseEnter('edge', edgeId));

    hitarea.addEventListener('mousemove', (e) => {
      // When pinned, only show tooltip on highlighted arcs
      const pinned = AppState.getPinned(appState);
      const arcId = hitarea.dataset.arcId;
      if (pinned) {
        const isHighlighted = pinned.type === 'edge'
          ? pinned.id === arcId
          : (hitarea.dataset.from === pinned.id || hitarea.dataset.to === pinned.id);
        if (!isHighlighted) {
          hideFloatingLabel();
          return;
        }
      }
      // Use StaticData for regular arc tooltips (no DOM read)
      const locs = StaticData.getFormattedUsages(arcId);
      if (locs) {
        const svg = DomAdapter.getSvgRoot();
        const rect = svg.getBoundingClientRect();
        const viewBox = svg.viewBox.baseVal;
        const svgPt = ArcLogic.getSvgCoords(e.clientX, e.clientY, rect, viewBox);
        showFloatingLabel(locs, svgPt.x + 10, svgPt.y - 20);
      }
    });

    hitarea.addEventListener('mouseleave', () => {
      handleMouseLeave();
      hideFloatingLabel();
    });
  });

  DomAdapter.getSvgRoot().addEventListener('click', () => {
    AppState.clearPinned(appState);
    clearHighlights();
  });

  // Apply initial arc weights based on source location counts
  applyInitialArcWeights();
})();
}
