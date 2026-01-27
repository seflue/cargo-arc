// svg_script.js - Extracted from render.rs
// Placeholders replaced at runtime: __ROW_HEIGHT__, __MARGIN__

// === Testable pure logic ===
const ArcLogic = {
  /**
   * Calculate arc offset based on number of hops between nodes
   * @param {number} hops - Number of row hops between source and target
   * @returns {number} - Offset for arc control point
   */
  getArcOffset(hops) {
    return 20 + (hops * 15);
  },

  /**
   * Transform mouse event coordinates to SVG coordinates (scroll-aware)
   * Uses getBoundingClientRect() instead of getScreenCTM() to handle scrollable containers.
   * See: WebKit Bug #44083, D3.js Issue #1164
   * @param {number} clientX - Mouse clientX from event
   * @param {number} clientY - Mouse clientY from event
   * @param {{left: number, top: number, width: number, height: number}} svgRect - SVG bounding rect
   * @param {{x: number, y: number, width: number, height: number}|null} viewBox - SVG viewBox or null
   * @returns {{x: number, y: number}} - Coordinates in SVG coordinate space
   */
  getSvgCoords(clientX, clientY, svgRect, viewBox) {
    let x = clientX - svgRect.left;
    let y = clientY - svgRect.top;

    if (viewBox && viewBox.width > 0) {
      x = x * (viewBox.width / svgRect.width) + viewBox.x;
      y = y * (viewBox.height / svgRect.height) + viewBox.y;
    }

    return { x, y };
  },

  /**
   * Calculate SVG path for an arc between two points
   * @param {number} fromX - Start X coordinate
   * @param {number} fromY - Start Y coordinate
   * @param {number} toX - End X coordinate
   * @param {number} toY - End Y coordinate
   * @param {number} maxRight - Rightmost X coordinate of visible nodes
   * @param {number} rowHeight - Height of each row
   * @returns {{path: string, toX: number, toY: number, ctrlX: number, midY: number}}
   */
  calculateArcPath(fromX, fromY, toX, toY, maxRight, rowHeight) {
    const hops = Math.max(1, Math.round(Math.abs(toY - fromY) / rowHeight));
    const arcOffset = this.getArcOffset(hops);
    const ctrlX = maxRight + arcOffset;
    const midY = (fromY + toY) / 2;

    return {
      path: `M ${fromX},${fromY} Q ${ctrlX},${fromY} ${ctrlX},${midY} Q ${ctrlX},${toY} ${toX},${toY}`,
      toX,
      toY,
      ctrlX,
      midY
    };
  },

  /**
   * Sort and group tooltip location strings by symbol name.
   * Re-sorts aggregated tooltip data for virtual arcs to ensure consistent display.
   *
   * Input format: Array of pipe-separated location strings, each containing
   * entries like "Symbol  ← file:line" or bare "file:line"
   *
   * @param {string[]} locStrings - Array of tooltip location strings
   * @returns {string} - Sorted and grouped locations joined by '|'
   */
  /**
   * Count the number of source locations in a pipe-separated string.
   * Format: "Symbol  ← file:line|     ← file:line|Symbol2  ← file:line"
   * Each pipe-separated segment represents one location.
   * @param {string} locationsString - Pipe-separated location string
   * @returns {number} - Number of locations
   */
  countLocations(locationsString) {
    if (!locationsString) return 0;
    return locationsString.split('|').length;
  },

  /**
   * Calculate stroke width based on location count using logarithmic scaling.
   * @param {number} locationCount - Number of source locations
   * @returns {number} - Stroke width in pixels (0.5 to 2.5)
   */
  calculateStrokeWidth(locationCount) {
    const MIN = 0.5, MAX = 2.5, CAP = 50;
    if (locationCount <= 0) return MIN;
    const count = Math.min(locationCount, CAP);
    return MIN + (MAX - MIN) * Math.log(count) / Math.log(CAP);
  },

  sortAndGroupLocations(locStrings) {
    const symbolRegex = /^(\S+)\s+←\s+(.+)$/;
    const bySymbol = {};  // symbol -> [locations]
    const bareLocations = [];

    // Parse all location entries
    for (const str of locStrings) {
      for (const entry of str.split('|')) {
        const trimmed = entry.trim();
        if (!trimmed) continue;

        const match = trimmed.match(symbolRegex);
        if (match) {
          const symbol = match[1];
          const location = match[2];
          if (!bySymbol[symbol]) bySymbol[symbol] = [];
          bySymbol[symbol].push(location);
        } else {
          // Bare location (no symbol prefix)
          bareLocations.push(trimmed);
        }
      }
    }

    // Sort symbols alphabetically
    const sortedSymbols = Object.keys(bySymbol).sort();

    // Sort locations within each symbol
    for (const symbol of sortedSymbols) {
      bySymbol[symbol].sort();
    }
    bareLocations.sort();

    // Find max symbol length for column alignment
    const maxLen = sortedSymbols.reduce((max, s) => Math.max(max, s.length), 0);

    // Build output
    const lines = [];

    // Bare locations first
    for (const loc of bareLocations) {
      lines.push(loc);
    }

    // Symbol-grouped locations with alignment
    for (const symbol of sortedSymbols) {
      const locs = bySymbol[symbol];
      for (let i = 0; i < locs.length; i++) {
        if (i === 0) {
          const padding = ' '.repeat(maxLen - symbol.length + 2);
          lines.push(`${symbol}${padding}← ${locs[i]}`);
        } else {
          const spaces = ' '.repeat(maxLen + 2);
          lines.push(`${spaces}← ${locs[i]}`);
        }
      }
    }

    return lines.join('|');
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') module.exports = { ArcLogic };

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
    const scale = ArrowLogic.scaleFromStrokeWidth(strokeWidth);
    const arrows = document.querySelectorAll(`[data-edge="${edgeId}"]`);
    arrows.forEach(arrow => {
      const points = arrow.getAttribute('points');
      const tip = ArrowLogic.parseTipFromPoints(points);
      if (tip) {
        arrow.setAttribute('points', ArrowLogic.getArrowPoints(tip, scale));
      }
    });
  }

  function applyInitialArcWeights() {
    document.querySelectorAll('.arc-hitarea').forEach(hitarea => {
      const locs = hitarea.dataset.sourceLocations;
      const count = ArcLogic.countLocations(locs);
      const width = ArcLogic.calculateStrokeWidth(count);
      const arcId = hitarea.dataset.arcId;
      const visibleArc = document.querySelector(`.dep-arc[data-arc-id="${arcId}"], .cycle-arc[data-arc-id="${arcId}"]`);
      if (visibleArc) visibleArc.style.strokeWidth = width + 'px';
      scaleArrow(arcId, width);

      // Store original values for reset (fixes arrow-head growth bug)
      const scale = ArrowLogic.scaleFromStrokeWidth(width);
      const arrows = document.querySelectorAll(`[data-edge="${arcId}"]`);
      arrows.forEach(arrow => {
        const tip = ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
        if (tip) {
          HighlightState.storeOriginal(highlightState, arcId, {
            strokeWidth: width,
            scale: scale,
            tipX: tip.x,
            tipY: tip.y
          });
        }
      });
    });
  }

  // === Floating label for source locations ===
  let floatingLabel = null;

  function showFloatingLabel(text, x, y) {
    hideFloatingLabel();
    const svg = document.querySelector('svg');
    floatingLabel = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    floatingLabel.setAttribute('class', 'floating-label');

    const padding = 6;
    const lineHeight = 14;
    const lines = text.split('|');

    const textEl = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    textEl.setAttribute('x', x + padding);
    textEl.setAttribute('y', y + lineHeight);

    // Create tspan for each line
    lines.forEach((line, i) => {
      const tspan = document.createElementNS('http://www.w3.org/2000/svg', 'tspan');
      tspan.setAttribute('x', x + padding);
      tspan.setAttribute('dy', i === 0 ? 0 : lineHeight);
      tspan.textContent = line;
      textEl.appendChild(tspan);
    });

    // Measure width
    svg.appendChild(textEl);
    const bbox = textEl.getBBox();
    svg.removeChild(textEl);

    const labelWidth = bbox.width + padding * 2;
    const labelHeight = lines.length * lineHeight + padding;

    const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
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
  // Use HighlightState module for state management
  const highlightState = HighlightState.create();

  function clearHighlights() {
    // Clear shadow paths
    const shadowLayer = document.getElementById('highlight-shadows');
    if (shadowLayer) shadowLayer.innerHTML = '';

    // Move highlighted elements back to base layers
    document.querySelectorAll('#highlight-arcs-layer > *').forEach(el => {
      moveToLayer(el, 'base-arcs-layer');
    });
    document.querySelectorAll('#highlight-labels-layer > *').forEach(el => {
      moveToLayer(el, 'base-labels-layer');
    });
    document.querySelectorAll('#highlight-hitareas-layer > *').forEach(el => {
      moveToLayer(el, 'hitareas-layer');
    });

    // Reset regular arcs and arrows to original size (using stored values)
    document.querySelectorAll('.arc-hitarea:not(.virtual-hitarea)').forEach(hitarea => {
      const arcId = hitarea.dataset.arcId;
      const original = HighlightState.getOriginal(highlightState, arcId);

      if (original) {
        // Use stored original values (fixes arrow-head growth bug)
        const visibleArc = document.querySelector(`.dep-arc[data-arc-id="${arcId}"], .cycle-arc[data-arc-id="${arcId}"]`);
        if (visibleArc) visibleArc.style.strokeWidth = original.strokeWidth + 'px';
        // Reset arrow using stored tip position and scale
        const arrows = document.querySelectorAll(`[data-edge="${arcId}"]`);
        arrows.forEach(arrow => {
          arrow.setAttribute('points', ArrowLogic.getArrowPoints(
            { x: original.tipX, y: original.tipY },
            original.scale
          ));
        });
      } else {
        // Fallback: calculate from source locations (for arcs without stored values)
        const locs = hitarea.dataset.sourceLocations;
        const count = ArcLogic.countLocations(locs);
        const strokeWidth = ArcLogic.calculateStrokeWidth(count);
        const visibleArc = document.querySelector(`.dep-arc[data-arc-id="${arcId}"], .cycle-arc[data-arc-id="${arcId}"]`);
        if (visibleArc) visibleArc.style.strokeWidth = strokeWidth + 'px';
        scaleArrow(arcId, strokeWidth);
      }
    });

    // Reset virtual arcs and arrows to original size (based on aggregated arc weights)
    // Note: Virtual arcs don't use stored originals because they are destroyed and
    // recreated on each recalculateVirtualEdges() call. Recalculating from
    // sourceLocations is correct here - no accumulation bug possible.
    document.querySelectorAll('.virtual-hitarea').forEach(hitarea => {
      const arcId = hitarea.dataset.arcId;
      const locs = hitarea.dataset.sourceLocations;
      const count = ArcLogic.countLocations(locs);
      const strokeWidth = ArcLogic.calculateStrokeWidth(count);
      const scale = strokeWidth / 1.5;

      // Reset virtual arc stroke-width
      document.querySelectorAll(`.virtual-arc[data-arc-id="${arcId}"]`).forEach(arc => {
        arc.style.strokeWidth = strokeWidth + 'px';
      });

      // Reset virtual arrow size
      document.querySelectorAll(`.virtual-arrow[data-vedge="${arcId}"]`).forEach(arrow => {
        const tip = ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
        if (tip) {
          arrow.setAttribute('points', ArrowLogic.getArrowPoints(tip, scale));
        }
      });
    });

    // Remove CSS classes (including new highlight marker classes)
    document.querySelectorAll('.selected-crate, .selected-module, .dep-node, .dependent-node, .highlighted-arc, .highlighted-arrow, .highlighted-label, .dimmed')
      .forEach(el => el.classList.remove('selected-crate', 'selected-module', 'dep-node', 'dependent-node', 'highlighted-arc', 'highlighted-arrow', 'highlighted-label', 'dimmed'));
  }

  // Helper: get visible arc element by arc-id
  function getVisibleArc(arcId) {
    return document.querySelector(
      `.dep-arc[data-arc-id="${arcId}"], .cycle-arc[data-arc-id="${arcId}"]`
    );
  }

  // Move element to a specific layer
  function moveToLayer(element, layerId) {
    if (element) {
      document.getElementById(layerId)?.appendChild(element);
    }
  }

  // Move element to appropriate highlight layer based on type
  function moveToHighlightLayer(element) {
    if (!element) return;
    if (element.classList.contains('dep-arc') ||
        element.classList.contains('cycle-arc') ||
        element.classList.contains('virtual-arc') ||
        element.tagName === 'polygon') {
      moveToLayer(element, 'highlight-arcs-layer');
    } else if (element.classList.contains('arc-count-group')) {
      moveToLayer(element, 'highlight-labels-layer');
    }
  }

  // Move element back to appropriate base layer
  function moveToBaseLayer(element) {
    if (!element) return;
    if (element.classList.contains('dep-arc') ||
        element.classList.contains('cycle-arc') ||
        element.classList.contains('virtual-arc') ||
        element.tagName === 'polygon') {
      moveToLayer(element, 'base-arcs-layer');
    } else if (element.classList.contains('arc-count-group')) {
      moveToLayer(element, 'base-labels-layer');
    }
  }

  // Create shadow path for glow effect
  // relationType: 'dep' (incoming) or 'reverse' (outgoing)
  function createShadowPath(arc, relationType) {
    if (!arc) return;
    const shadow = arc.cloneNode(false);
    shadow.classList.add('shadow-path');
    shadow.classList.add(relationType === 'dep' ? 'glow-incoming' : 'glow-outgoing');
    shadow.removeAttribute('id');

    // Dynamic width: 4× arc width for glow effect
    const arcWidth = parseFloat(arc.style.strokeWidth) || 0.5;
    shadow.style.strokeWidth = (arcWidth * 4) + 'px';
    shadow.setAttribute('opacity', '0.25');
    shadow.style.strokeLinecap = 'round';

    // Shorten shadow to compensate for wider stroke
    // Overhang per end ≈ (shadowWidth - arcWidth) / 2
    const shadowWidth = arcWidth * 4;
    const overhang = (shadowWidth - arcWidth) / 2;
    const pathLength = arc.getTotalLength?.() || 100;
    const visibleLength = Math.max(0, pathLength - overhang * 2);
    shadow.style.strokeDasharray = visibleLength + ' ' + pathLength;
    shadow.style.strokeDashoffset = -overhang + 'px';

    const shadowLayer = document.getElementById('highlight-shadows');
    if (shadowLayer) shadowLayer.appendChild(shadow);
  }

  // Dim all non-highlighted elements (except toolbar and hitareas)
  function dimNonHighlighted() {
    document.querySelectorAll(
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
    const arc = getVisibleArc(arcId);

    // Skip if arc is hidden (collapsed or filtered out)
    if (arc?.style.display === 'none' || arc?.classList.contains('hidden-by-filter')) return;

    // Create shadow path with relation type (edge click = viewing dependency)
    createShadowPath(arc, 'dep');

    // from-Node: dependent (purple border) - source of the edge
    document.getElementById('node-' + from)?.classList.add('dependent-node');
    // to-Node: dep (green border) - target of the edge
    document.getElementById('node-' + to)?.classList.add('dep-node');

    // Arc: marker class only (keeps direction color), dynamic stroke-width
    arc?.classList.add('highlighted-arc');
    const arcWidth = parseFloat(arc?.style.strokeWidth) || 0.5;
    const highlightWidth = arcWidth * 1.3;
    if (arc) arc.style.strokeWidth = highlightWidth + 'px';
    scaleArrow(arcId, highlightWidth);

    // Virtual arcs
    document.querySelectorAll('.virtual-arc[data-from="' + from + '"][data-to="' + to + '"]')
      .forEach(el => {
        el.classList.add('highlighted-arc');
        createShadowPath(el, 'dep');
        const vWidth = parseFloat(el.style.strokeWidth) || 0.5;
        el.style.strokeWidth = (vWidth * 1.3) + 'px';
      });

    // Arrows: marker class (keeps direction color)
    document.querySelectorAll('[data-edge="' + arcId + '"]')
      .forEach(el => el.classList.add('highlighted-arrow'));
    document.querySelectorAll('[data-vedge="' + arcId + '"]:not(.arc-count)')
      .forEach(el => {
        el.classList.add('highlighted-arrow');
        // Scale virtual arrows
        if (el.classList.contains('virtual-arrow')) {
          const tip = ArrowLogic.parseTipFromPoints(el.getAttribute('points'));
          if (tip) {
            el.setAttribute('points', ArrowLogic.getArrowPoints(tip, highlightWidth / 1.5));
          }
        }
      });

    // Arc-count labels
    document.querySelectorAll('.arc-count[data-vedge="' + arcId + '"]')
      .forEach(el => el.classList.add('highlighted-label'));

    // Move highlighted elements to highlight layers
    moveToHighlightLayer(arc);
    moveToHighlightLayer(document.querySelector(`.virtual-arc[data-arc-id="${arcId}"]`));
    moveToHighlightLayer(document.querySelector(`.arc-count-group[data-vedge="${arcId}"]`));
    // Arrows
    document.querySelectorAll(`[data-edge="${arcId}"]`).forEach(moveToHighlightLayer);
    document.querySelectorAll(`[data-vedge="${arcId}"]:not(.arc-count)`).forEach(moveToHighlightLayer);

    // Move hitarea to highlight layer (higher z-order) so it receives events over dimmed hitareas
    const highlightHitareasLayer = document.getElementById('highlight-hitareas-layer');
    const hitarea = document.querySelector(`.arc-hitarea[data-arc-id="${arcId}"]`);
    if (hitarea && highlightHitareasLayer) highlightHitareasLayer.appendChild(hitarea);

    dimNonHighlighted();
  }

  function applyNodeHighlight(nodeId) {
    // Selected node: saturated original color
    const selectedNode = document.getElementById('node-' + nodeId);
    if (selectedNode) {
      if (selectedNode.classList.contains('crate')) {
        selectedNode.classList.add('selected-crate');
      } else if (selectedNode.classList.contains('module')) {
        selectedNode.classList.add('selected-module');
      }
    }

    // Helper to determine if edge is outgoing (dep) or incoming (dependent)
    const isOutgoing = (from, to) => from === nodeId;

    // Regular arcs via hitareas
    document.querySelectorAll('.arc-hitarea[data-from="' + nodeId + '"], .arc-hitarea[data-to="' + nodeId + '"]')
      .forEach(hitarea => {
        // Skip hitareas filtered out by user (e.g., CrateDeps checkbox)
        // Note: We check visibleArc for display:none (collapsed), not hitarea
        if (hitarea.classList.contains('hidden-by-filter')) return;

        const arcId = hitarea.dataset.arcId;
        const visibleArc = getVisibleArc(arcId);

        // Skip if arc is hidden (endpoints are collapsed)
        if (visibleArc?.style.display === 'none') return;

        const from = hitarea.dataset.from;
        const to = hitarea.dataset.to;
        const outgoing = isOutgoing(from, to);

        // Create shadow path with relation type (glow color)
        createShadowPath(visibleArc, outgoing ? 'dep' : 'reverse');

        // Arc: marker class only (keeps direction color), dynamic stroke-width
        visibleArc?.classList.add('highlighted-arc');
        const arcWidth = parseFloat(visibleArc?.style.strokeWidth) || 0.5;
        const highlightWidth = arcWidth * 1.3;
        if (visibleArc) visibleArc.style.strokeWidth = highlightWidth + 'px';
        scaleArrow(from + '-' + to, highlightWidth);

        // Connected nodes (border only)
        const otherNodeId = outgoing ? to : from;
        const otherNode = document.getElementById('node-' + otherNodeId);
        otherNode?.classList.add(outgoing ? 'dep-node' : 'dependent-node');

        // Arrows: marker class (keeps direction color)
        document.querySelectorAll('[data-edge="' + from + '-' + to + '"]')
          .forEach(arr => arr.classList.add('highlighted-arrow'));

        // Move to highlight layers
        moveToHighlightLayer(visibleArc);
        moveToHighlightLayer(document.querySelector(`.arc-count-group[data-vedge="${arcId}"]`));
        document.querySelectorAll(`[data-edge="${arcId}"]`).forEach(moveToHighlightLayer);
      });

    // Virtual arcs
    document.querySelectorAll('.virtual-arc[data-from="' + nodeId + '"], .virtual-arc[data-to="' + nodeId + '"]')
      .forEach(arc => {
        const from = arc.dataset.from;
        const to = arc.dataset.to;
        const outgoing = isOutgoing(from, to);

        // Create shadow path with relation type (glow color)
        createShadowPath(arc, outgoing ? 'dep' : 'reverse');

        // Arc: marker class only (keeps direction color), dynamic stroke-width
        arc.classList.add('highlighted-arc');
        const arcWidth = parseFloat(arc.style.strokeWidth) || 0.5;
        const highlightWidth = arcWidth * 1.3;
        arc.style.strokeWidth = highlightWidth + 'px';

        // Scale virtual arrows
        document.querySelectorAll(`.virtual-arrow[data-vedge="${from}-${to}"]`).forEach(arrow => {
          const tip = ArrowLogic.parseTipFromPoints(arrow.getAttribute('points'));
          if (tip) {
            arrow.setAttribute('points', ArrowLogic.getArrowPoints(tip, highlightWidth / 1.5));
          }
        });

        // Connected nodes (border only)
        const otherNodeId = outgoing ? to : from;
        const otherNode = document.getElementById('node-' + otherNodeId);
        otherNode?.classList.add(outgoing ? 'dep-node' : 'dependent-node');

        // Arrows: marker class (keeps direction color)
        document.querySelectorAll('[data-vedge="' + from + '-' + to + '"]:not(.arc-count)')
          .forEach(arr => arr.classList.add('highlighted-arrow'));

        // Arc-count labels
        document.querySelectorAll('.arc-count[data-vedge="' + from + '-' + to + '"]')
          .forEach(el => el.classList.add('highlighted-label'));

        // Move to highlight layers
        moveToHighlightLayer(arc);
        moveToHighlightLayer(document.querySelector(`.arc-count-group[data-vedge="${from}-${to}"]`));
        document.querySelectorAll(`[data-vedge="${from}-${to}"]:not(.arc-count)`).forEach(moveToHighlightLayer);
      });

    // Move connected hitareas to highlight layer (higher z-order)
    const highlightHitareasLayer = document.getElementById('highlight-hitareas-layer');
    if (highlightHitareasLayer) {
      document.querySelectorAll(`.arc-hitarea[data-from="${nodeId}"], .arc-hitarea[data-to="${nodeId}"]`)
        .forEach(h => highlightHitareasLayer.appendChild(h));
    }

    dimNonHighlighted();
  }

  function highlightEdge(from, to, pin) {
    const edgeId = from + '-' + to;
    if (pin) {
      // Toggle: if same edge is already pinned, deselect
      const wasPinned = HighlightState.togglePinned(highlightState, 'edge', edgeId);
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
      const wasPinned = HighlightState.togglePinned(highlightState, 'node', nodeId);
      clearHighlights();
      if (!wasPinned) return; // Was unpinned, done
    } else {
      clearHighlights();
    }
    applyNodeHighlight(nodeId);
  }

  function handleMouseEnter(type, id) {
    if (HighlightState.getPinned(highlightState)) return; // Don't preview if something is pinned
    clearHighlights();
    if (type === 'node') applyNodeHighlight(id);
    else if (type === 'edge') {
      const [from, to] = id.split('-');
      applyEdgeHighlight(from, to);
    }
  }

  function handleMouseLeave() {
    if (HighlightState.getPinned(highlightState)) return; // Keep pinned highlight
    clearHighlights();
  }

  // === Collapse functionality ===
  // Build parentMap once at init (pure data structure for TreeLogic)
  const parentMap = TreeLogic.buildParentMap(document);

  // Initialize collapse state with CollapseState module
  const collapseState = CollapseState.create();

  // Store original positions on load
  document.querySelectorAll('.crate, .module').forEach(node => {
    const id = node.id.replace('node-', '');
    CollapseState.storePosition(
      collapseState,
      id,
      parseFloat(node.getAttribute('x')),
      parseFloat(node.getAttribute('y'))
    );
  });

  // Wrapper functions for TreeLogic (use parentMap from closure)
  function getDescendants(nodeId) {
    return TreeLogic.getDescendants(nodeId, parentMap);
  }

  function getVisibleAncestor(nodeId) {
    return TreeLogic.getVisibleAncestor(nodeId, collapseState.collapsed, parentMap);
  }

  function countDescendants(nodeId) {
    return TreeLogic.countDescendants(nodeId, parentMap);
  }

  // Update tree lines for a node at new Y position
  function updateTreeLines(nodeId, newY, nodeHeight) {
    // Update lines where this node is the child
    document.querySelectorAll('line[data-child="' + nodeId + '"]').forEach(line => {
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
    document.querySelectorAll('line[data-parent="' + nodeId + '"]').forEach(line => {
      if (line.getAttribute('x1') === line.getAttribute('x2')) {
        // Vertical line - update y1 (parent bottom)
        line.setAttribute('y1', newY + nodeHeight);
      }
    });
  }

  // Relayout visible nodes
  function relayout() {
    let currentY = MARGIN + TOOLBAR_HEIGHT;

    // Get all nodes sorted by original Y position
    const items = [...document.querySelectorAll('.crate, .module')]
      .sort((a, b) => {
        const aId = a.id.replace('node-', '');
        const bId = b.id.replace('node-', '');
        const posA = CollapseState.getPosition(collapseState, aId);
        const posB = CollapseState.getPosition(collapseState, bId);
        return posA.y - posB.y;
      });

    items.forEach(node => {
      if (node.classList.contains('collapsed')) return;
      if (node.classList.contains('hidden-by-filter')) return;

      const nodeId = node.id.replace('node-', '');
      const height = parseFloat(node.getAttribute('height'));

      // Update rect position
      node.setAttribute('y', currentY);

      // Update label position (next text sibling)
      const label = node.nextElementSibling;
      if (label && label.tagName === 'text' && label.classList.contains('label')) {
        label.setAttribute('y', currentY + height / 2 + 4);
      }

      // Update toggle icon position (if exists)
      const toggle = document.querySelector('.collapse-toggle[data-target="' + nodeId + '"]');
      if (toggle) {
        toggle.setAttribute('y', currentY + height / 2 + 4);
      }

      // Update tree lines
      updateTreeLines(nodeId, currentY, height);

      currentY += ROW_HEIGHT;
    });

    recalculateVirtualEdges();

    // Re-apply pinned highlight after edges were recreated
    const pinned = HighlightState.getPinned(highlightState);
    if (pinned) {
      clearHighlights();  // Remove stale shadow paths from deleted virtual arcs
      if (pinned.type === 'node') {
        applyNodeHighlight(pinned.id);
      } else if (pinned.type === 'edge') {
        const [from, to] = pinned.id.split('-');
        applyEdgeHighlight(from, to);
      }
    }
  }

  // Helper: Calculate arc path between two DOM nodes (uses ArcLogic)
  function calculateArcPathFromNodes(fromNode, toNode, yOffset) {
    const fromX = parseFloat(fromNode.getAttribute('x')) + parseFloat(fromNode.getAttribute('width'));
    const fromY = parseFloat(fromNode.getAttribute('y')) + parseFloat(fromNode.getAttribute('height')) / 2 + yOffset;
    const toX = parseFloat(toNode.getAttribute('x')) + parseFloat(toNode.getAttribute('width'));
    const toY = parseFloat(toNode.getAttribute('y')) + parseFloat(toNode.getAttribute('height')) / 2 - yOffset;

    // Find rightmost visible node for arc positioning
    let maxRight = 0;
    document.querySelectorAll('.crate, .module').forEach(n => {
      if (!n.classList.contains('collapsed')) {
        const right = parseFloat(n.getAttribute('x')) + parseFloat(n.getAttribute('width'));
        if (right > maxRight) maxRight = right;
      }
    });

    return ArcLogic.calculateArcPath(fromX, fromY, toX, toY, maxRight, ROW_HEIGHT);
  }

  // Helper: Extract edge data from DOM hitareas to pure objects
  function extractEdgeData(hitareas) {
    const edges = [];
    hitareas.forEach(hitarea => {
      const fromId = hitarea.dataset.from;
      const toId = hitarea.dataset.to;
      const fromNode = document.getElementById('node-' + fromId);
      const toNode = document.getElementById('node-' + toId);

      // A node is hidden if its visible ancestor is NOT itself
      // (i.e., an ancestor is collapsed, hiding this node)
      const fromIsHidden = getVisibleAncestor(fromId) !== fromId;
      const toIsHidden = getVisibleAncestor(toId) !== toId;

      edges.push({
        hitarea,
        arcId: hitarea.dataset.arcId,
        fromId,
        toId,
        fromNode,
        toNode,
        fromHidden: fromIsHidden,
        toHidden: toIsHidden,
        sourceLocations: hitarea.dataset.sourceLocations,
        direction: hitarea.dataset.direction
      });
    });
    return edges;
  }

  // Helper: Extract node positions from DOM to Map
  function extractNodePositions(nodeIds) {
    const positions = new Map();
    for (const nodeId of nodeIds) {
      const node = document.getElementById('node-' + nodeId);
      if (node) {
        positions.set(nodeId, {
          x: parseFloat(node.getAttribute('x')),
          y: parseFloat(node.getAttribute('y')),
          width: parseFloat(node.getAttribute('width')),
          height: parseFloat(node.getAttribute('height'))
        });
      }
    }
    return positions;
  }

  // Recalculate and show virtual edges for collapsed nodes
  function recalculateVirtualEdges() {
    // === DOM Cleanup ===
    document.querySelectorAll('.virtual-arc, .virtual-hitarea, .virtual-arrow, .arc-count, .arc-count-group, .arc-count-bg').forEach(el => el.remove());
    document.querySelectorAll('.arc-hitarea, .dep-arc, .cycle-arc').forEach(edge => {
      edge.style.display = '';
    });
    document.querySelectorAll('.dep-arrow, .cycle-arrow').forEach(arrow => {
      arrow.style.display = '';
    });

    // === Extract edge data from DOM ===
    const hitareas = document.querySelectorAll('.arc-hitarea');
    const edgeData = extractEdgeData(hitareas);

    // === Process edges: hide original elements, update visible paths ===
    edgeData.forEach(edge => {
      const { hitarea, arcId, fromId, toId, fromNode, toNode, fromHidden, toHidden } = edge;

      if (window.DEBUG_ARCS) {
        console.log(`Arc ${arcId}: from=${fromId}(${fromHidden ? 'hidden' : 'visible'}), to=${toId}(${toHidden ? 'hidden' : 'visible'})`);
      }

      if (fromHidden || toHidden) {
        // Hide original elements
        hitarea.style.display = 'none';
        const visibleArc = getVisibleArc(arcId);
        if (visibleArc) visibleArc.style.display = 'none';
        document.querySelectorAll('[data-edge="' + fromId + '-' + toId + '"]')
          .forEach(arr => arr.style.display = 'none');
      } else if (fromNode && toNode) {
        // Update visible arc paths
        const arc = calculateArcPathFromNodes(fromNode, toNode, 3);
        hitarea.setAttribute('d', arc.path);
        const visibleArc = getVisibleArc(arcId);
        if (visibleArc) visibleArc.setAttribute('d', arc.path);

        const strokeWidth = visibleArc ? parseFloat(visibleArc.style.strokeWidth) || 0.5 : 0.5;
        const scale = strokeWidth / 1.5;
        document.querySelectorAll('[data-edge="' + fromId + '-' + toId + '"]').forEach(arrow => {
          arrow.setAttribute('points', ArrowLogic.getArrowPoints({ x: arc.toX, y: arc.toY }, scale));
        });
      }
    });

    // === Pure logic: aggregate hidden edges ===
    const virtualEdges = VirtualEdgeLogic.aggregateHiddenEdges(edgeData, getVisibleAncestor);

    // === Extract node positions for virtual edge endpoints ===
    const nodeIds = new Set();
    virtualEdges.forEach((_, key) => {
      const [fromId, toId] = key.split('-');
      nodeIds.add(fromId);
      nodeIds.add(toId);
    });
    const nodePositions = extractNodePositions(nodeIds);

    // === Find maxRight for arc positioning ===
    let maxRight = 0;
    document.querySelectorAll('.crate, .module').forEach(n => {
      if (!n.classList.contains('collapsed')) {
        const right = parseFloat(n.getAttribute('x')) + parseFloat(n.getAttribute('width'));
        if (right > maxRight) maxRight = right;
      }
    });

    // === Pure logic: prepare render data ===
    const mergedEdges = VirtualEdgeLogic.prepareVirtualEdgeData(
      virtualEdges, nodePositions, maxRight, ArcLogic, ROW_HEIGHT
    );

    // === DOM: Create virtual edge elements ===
    const baseArcsLayer = document.getElementById('base-arcs-layer');
    const baseLabelsLayer = document.getElementById('base-labels-layer');
    const hitareasLayer = document.getElementById('hitareas-layer');

    // Pass 1: Arcs + Arrows (bottom layer)
    mergedEdges.forEach((data, key) => {
      const { fromId, toId, arc, strokeWidth, direction } = data;
      const arcId = fromId + '-' + toId;

      // Visible path (styled, no pointer events)
      const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      path.setAttribute('class', `virtual-arc ${direction}`);
      path.setAttribute('d', arc.path);
      path.setAttribute('data-arc-id', arcId);
      path.setAttribute('data-from', fromId);
      path.setAttribute('data-to', toId);
      path.style.strokeWidth = strokeWidth + 'px';
      baseArcsLayer.appendChild(path);

      // Arrow (scaled to match stroke width)
      const scale = strokeWidth / 1.5;
      const arrow = document.createElementNS('http://www.w3.org/2000/svg', 'polygon');
      arrow.setAttribute('class', `virtual-arrow ${direction}`);
      arrow.setAttribute('data-vedge', arcId);
      arrow.setAttribute('data-from', fromId);
      arrow.setAttribute('data-to', toId);
      arrow.setAttribute('points', ArrowLogic.getArrowPoints({ x: arc.toX, y: arc.toY }, scale));
      arrow.addEventListener('click', e => {
        e.stopPropagation();
        highlightVirtualEdge(fromId, toId, data.count);
      });
      baseArcsLayer.appendChild(arrow);
    });

    // Pass 2: Labels (middle layer, above arcs)
    mergedEdges.forEach((data, key) => {
      const { fromId, toId, arc, count } = data;

      if (count > 1) {
        const labelGroup = document.createElementNS('http://www.w3.org/2000/svg', 'g');
        labelGroup.setAttribute('class', 'arc-count-group');
        labelGroup.setAttribute('data-vedge', fromId + '-' + toId);

        const text = '(' + count + ')';
        const x = arc.ctrlX + 5;
        const y = arc.midY + 3;

        // Background rect (2-3px padding)
        const padding = 2;
        const textWidth = text.length * 6; // ~6px per char at 10px font
        const bg = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
        bg.setAttribute('class', 'arc-count-bg');
        bg.setAttribute('x', x - textWidth / 2 - padding);
        bg.setAttribute('y', y - 8 - padding);
        bg.setAttribute('width', textWidth + padding * 2);
        bg.setAttribute('height', 12 + padding * 2);
        bg.setAttribute('rx', '2');

        // Text label
        const countLabel = document.createElementNS('http://www.w3.org/2000/svg', 'text');
        countLabel.setAttribute('class', 'arc-count');
        countLabel.setAttribute('data-vedge', fromId + '-' + toId);
        countLabel.setAttribute('x', x);
        countLabel.setAttribute('y', y);
        countLabel.textContent = text;

        labelGroup.appendChild(bg);
        labelGroup.appendChild(countLabel);

        // Event handlers on group
        labelGroup.style.cursor = 'pointer';
        labelGroup.addEventListener('click', e => {
          e.stopPropagation();
          highlightVirtualEdge(fromId, toId, count);
        });
        labelGroup.addEventListener('mouseenter', () => handleMouseEnter('edge', fromId + '-' + toId));
        labelGroup.addEventListener('mouseleave', handleMouseLeave);

        baseLabelsLayer.appendChild(labelGroup);
      }
    });

    // Pass 3: Hitareas (hitareas layer, always on top)
    mergedEdges.forEach((data, key) => {
      const { fromId, toId, arc, hiddenEdgeData, count } = data;
      const arcId = fromId + '-' + toId;

      const hitarea = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      hitarea.setAttribute('class', 'virtual-hitarea arc-hitarea');
      hitarea.setAttribute('d', arc.path);
      hitarea.setAttribute('data-arc-id', arcId);
      hitarea.setAttribute('data-from', fromId);
      hitarea.setAttribute('data-to', toId);
      // Set aggregated source locations from hidden edges
      if (hiddenEdgeData.length > 0) {
        hitarea.dataset.sourceLocations = ArcLogic.sortAndGroupLocations(hiddenEdgeData);
      }
      // Click handler for highlighting
      hitarea.addEventListener('click', e => {
        e.stopPropagation();
        highlightVirtualEdge(fromId, toId, count);
      });
      // Hover handlers for floating label
      hitarea.addEventListener('mouseenter', () => handleMouseEnter('edge', arcId));
      hitarea.addEventListener('mousemove', (e) => {
        // When pinned, only show tooltip on highlighted arcs
        const pinned = HighlightState.getPinned(highlightState);
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
          const svg = document.querySelector('svg');
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
      hitareasLayer.appendChild(hitarea);
    });
  }

  // Highlight virtual (aggregated) edge
  function highlightVirtualEdge(fromId, toId, count) {
    const edgeId = fromId + '-' + toId;
    // Toggle: if same edge is already pinned, deselect
    const wasPinned = HighlightState.togglePinned(highlightState, 'edge', edgeId);
    clearHighlights();
    if (!wasPinned) return; // Was unpinned, done
    // from-Node: dependent (purple border)
    document.getElementById('node-' + fromId)?.classList.add('dependent-node');
    // to-Node: dep (green border)
    document.getElementById('node-' + toId)?.classList.add('dep-node');
    // Virtual arc: marker class (keeps direction color), dynamic stroke-width
    document.querySelectorAll('.virtual-arc[data-from="' + fromId + '"][data-to="' + toId + '"]')
      .forEach(el => {
        createShadowPath(el, 'dep');
        el.classList.add('highlighted-arc');
        const arcWidth = parseFloat(el.style.strokeWidth) || 0.5;
        const highlightWidth = arcWidth * 1.3;
        el.style.strokeWidth = highlightWidth + 'px';
      });
    // Arrows: marker class (keeps direction color), scale to match arc
    document.querySelectorAll('.virtual-arrow[data-vedge="' + fromId + '-' + toId + '"]')
      .forEach(el => {
        el.classList.add('highlighted-arrow');
        const arc = document.querySelector('.virtual-arc[data-from="' + fromId + '"][data-to="' + toId + '"]');
        const arcWidth = parseFloat(arc?.style.strokeWidth) || 0.5;
        const tip = ArrowLogic.parseTipFromPoints(el.getAttribute('points'));
        if (tip) {
          el.setAttribute('points', ArrowLogic.getArrowPoints(tip, arcWidth / 1.5));
        }
      });
    // Arc-count labels
    document.querySelectorAll('.arc-count[data-vedge="' + fromId + '-' + toId + '"]')
      .forEach(el => el.classList.add('highlighted-label'));
    // Move virtual arc and label group to highlight layers
    moveToHighlightLayer(document.querySelector('.virtual-arc[data-arc-id="' + edgeId + '"]'));
    moveToHighlightLayer(document.querySelector('.arc-count-group[data-vedge="' + edgeId + '"]'));
    document.querySelectorAll('.virtual-arrow[data-vedge="' + edgeId + '"]').forEach(moveToHighlightLayer);
    // Dim everything else
    dimNonHighlighted();
  }

  // Check if nodeId is an ancestor of targetId
  function isAncestorOf(nodeId, targetId) {
    let checkId = targetId;
    while (true) {
      const checkNode = document.getElementById('node-' + checkId);
      const parentId = checkNode?.dataset.parent;
      if (!parentId) return false;
      if (parentId === nodeId) return true;
      checkId = parentId;
    }
  }

  // Toggle collapse state
  function toggleCollapse(nodeId) {
    // Always clear selection on collapse/expand - shadows would need recalculation
    // and the visual context changes significantly
    if (HighlightState.getPinned(highlightState)) {
      HighlightState.clearPinned(highlightState);
      clearHighlights();
    }

    const collapsed = CollapseState.toggle(collapseState, nodeId);

    const descendants = getDescendants(nodeId);

    // Toggle visibility of descendants
    descendants.forEach(descId => {
      const node = document.getElementById('node-' + descId);
      const label = node?.nextElementSibling;
      const toggle = document.querySelector('.collapse-toggle[data-target="' + descId + '"]');

      if (collapsed) {
        node?.classList.add('collapsed');
        label?.classList.add('collapsed');
        toggle?.classList.add('collapsed');
      } else {
        // Only show if no ancestor is collapsed
        let ancestorCollapsed = false;
        let checkId = descId;
        while (true) {
          const checkNode = document.getElementById('node-' + checkId);
          const parentId = checkNode?.dataset.parent;
          if (!parentId) break;
          if (CollapseState.isCollapsed(collapseState, parentId) && parentId !== nodeId) {
            ancestorCollapsed = true;
            break;
          }
          checkId = parentId;
        }
        if (!ancestorCollapsed) {
          node?.classList.remove('collapsed');
          label?.classList.remove('collapsed');
          toggle?.classList.remove('collapsed');
        }
      }

      // Hide/show tree lines for descendants
      document.querySelectorAll('line[data-child="' + descId + '"]').forEach(line => {
        if (collapsed) {
          line.classList.add('collapsed');
        } else if (!document.getElementById('node-' + descId)?.classList.contains('collapsed')) {
          line.classList.remove('collapsed');
        }
      });
    });

    // Update toggle icon
    const toggleIcon = document.querySelector('.collapse-toggle[data-target="' + nodeId + '"]');
    if (toggleIcon) {
      toggleIcon.textContent = collapsed ? '+' : '−';
    }

    // Update child count label
    const countLabel = document.getElementById('count-' + nodeId);
    if (countLabel) {
      if (collapsed) {
        const count = countDescendants(nodeId);
        countLabel.textContent = ' (+' + count + ')';
      } else {
        countLabel.textContent = '';
      }
    }

    relayout();
  }

  // Toggle collapse/expand all parent nodes
  function toggleCollapseAll() {
    const allExpanded = [...document.querySelectorAll('[data-has-children="true"]')]
      .every(node => !CollapseState.isCollapsed(collapseState, node.id.replace('node-', '')));

    document.querySelectorAll('[data-has-children="true"]').forEach(node => {
      const nodeId = node.id.replace('node-', '');
      if (allExpanded) {
        // Collapse all
        CollapseState.setCollapsed(collapseState, nodeId, true);
        getDescendants(nodeId).forEach(descId => {
          const descNode = document.getElementById('node-' + descId);
          const label = descNode?.nextElementSibling;
          const toggle = document.querySelector('.collapse-toggle[data-target="' + descId + '"]');
          descNode?.classList.add('collapsed');
          label?.classList.add('collapsed');
          toggle?.classList.add('collapsed');
          document.querySelectorAll('line[data-child="' + descId + '"]').forEach(line => {
            line.classList.add('collapsed');
          });
        });
        // Update toggle icon
        const toggleIcon = document.querySelector('.collapse-toggle[data-target="' + nodeId + '"]');
        if (toggleIcon) toggleIcon.textContent = '+';
        // Update child count
        const countLabel = document.getElementById('count-' + nodeId);
        if (countLabel) countLabel.textContent = ' (+' + countDescendants(nodeId) + ')';
      } else {
        // Expand all
        CollapseState.setCollapsed(collapseState, nodeId, false);
        getDescendants(nodeId).forEach(descId => {
          const descNode = document.getElementById('node-' + descId);
          const label = descNode?.nextElementSibling;
          const toggle = document.querySelector('.collapse-toggle[data-target="' + descId + '"]');
          descNode?.classList.remove('collapsed');
          label?.classList.remove('collapsed');
          toggle?.classList.remove('collapsed');
          document.querySelectorAll('line[data-child="' + descId + '"]').forEach(line => {
            line.classList.remove('collapsed');
          });
        });
        // Update toggle icon
        const toggleIcon = document.querySelector('.collapse-toggle[data-target="' + nodeId + '"]');
        if (toggleIcon) toggleIcon.textContent = '−';
        // Clear child count
        const countLabel = document.getElementById('count-' + nodeId);
        if (countLabel) countLabel.textContent = '';
      }
    });

    // Update button label
    const label = document.getElementById('collapse-toggle-label');
    if (label) label.textContent = allExpanded ? 'Expand All' : 'Collapse All';

    relayout();
  }

  // Toggle visibility of crate-to-crate dependency arcs
  function toggleCrateDepVisibility() {
    const checkbox = document.querySelector('#crate-dep-checkbox');
    if (!checkbox) return;

    const isChecked = checkbox.classList.toggle('checked');

    document.querySelectorAll('.crate-dep-arc').forEach(arc => {
      if (isChecked) {
        arc.classList.remove('hidden-by-filter');
      } else {
        arc.classList.add('hidden-by-filter');
      }
    });

    // Also hide/show associated hitareas and arrows
    document.querySelectorAll('.arc-hitarea').forEach(hitarea => {
      const arcId = hitarea.dataset.arcId;
      const visibleArc = document.querySelector(`.crate-dep-arc[data-arc-id="${arcId}"]`);
      if (visibleArc) {
        if (isChecked) {
          hitarea.classList.remove('hidden-by-filter');
        } else {
          hitarea.classList.add('hidden-by-filter');
        }
        // Also handle arrows
        document.querySelectorAll(`[data-edge="${arcId}"]`).forEach(arrow => {
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
    const toolbar = document.querySelector('.view-options');
    const svg = document.querySelector('svg');
    if (!toolbar || !svg) return;

    const rect = svg.getBoundingClientRect();
    const scrollTop = Math.max(0, -rect.top);
    toolbar.setAttribute('transform', `translate(0, ${scrollTop})`);
  }

  window.addEventListener('scroll', updateToolbarPosition);

  // === Event handlers ===
  document.querySelectorAll('.crate, .module').forEach(node => {
    const nodeId = node.id.replace('node-', '');

    node.addEventListener('click', e => {
      e.stopPropagation();
      highlightNode(nodeId, true); // pin
    });

    node.addEventListener('mouseenter', () => handleMouseEnter('node', nodeId));
    node.addEventListener('mouseleave', handleMouseLeave);

    // Double-click to toggle collapse (only for parents)
    if (node.dataset.hasChildren === 'true') {
      node.addEventListener('dblclick', e => {
        e.stopPropagation();
        toggleCollapse(nodeId);
        updateToolbarPosition();
      });
    }
  });

  document.querySelectorAll('.collapse-toggle').forEach(toggle => {
    toggle.addEventListener('click', e => {
      e.stopPropagation();
      toggleCollapse(toggle.dataset.target);
      updateToolbarPosition();
    });
  });

  // Toolbar button event handlers
  document.querySelector('#collapse-toggle-btn')?.addEventListener('click', e => {
    e.stopPropagation();
    toggleCollapseAll();
    updateToolbarPosition();
  });
  document.querySelector('#collapse-toggle-label')?.addEventListener('click', e => {
    e.stopPropagation();
    toggleCollapseAll();
    updateToolbarPosition();
  });

  document.querySelector('#crate-dep-checkbox')?.addEventListener('click', e => {
    e.stopPropagation();
    toggleCrateDepVisibility();
  });

  // Event handlers on hit-area paths (invisible, 12px wide)
  document.querySelectorAll('.arc-hitarea').forEach(hitarea => {
    const edgeId = hitarea.dataset.from + '-' + hitarea.dataset.to;

    hitarea.addEventListener('click', e => {
      e.stopPropagation();
      highlightEdge(hitarea.dataset.from, hitarea.dataset.to, true); // pin
    });

    hitarea.addEventListener('mouseenter', () => handleMouseEnter('edge', edgeId));

    hitarea.addEventListener('mousemove', (e) => {
      // When pinned, only show tooltip on highlighted arcs
      const pinned = HighlightState.getPinned(highlightState);
      if (pinned) {
        const arcId = hitarea.dataset.arcId;
        const isHighlighted = pinned.type === 'edge'
          ? pinned.id === arcId
          : (hitarea.dataset.from === pinned.id || hitarea.dataset.to === pinned.id);
        if (!isHighlighted) {
          hideFloatingLabel();
          return;
        }
      }
      const locs = hitarea.dataset.sourceLocations;
      if (locs) {
        const svg = document.querySelector('svg');
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

  document.querySelector('svg').addEventListener('click', () => {
    HighlightState.clearPinned(highlightState);
    clearHighlights();
  });

  // Apply initial arc weights based on source location counts
  applyInitialArcWeights();
})();
}
