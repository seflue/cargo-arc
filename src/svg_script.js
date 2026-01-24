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
  let pinnedHighlight = null; // {type: 'node'|'edge', id: string} or null

  function clearHighlights() {
    document.querySelectorAll('.highlighted, .highlighted-node, .highlighted-arrow, .dimmed')
      .forEach(el => el.classList.remove('highlighted', 'highlighted-node', 'highlighted-arrow', 'dimmed'));
  }

  // Helper: get visible arc element by arc-id
  function getVisibleArc(arcId) {
    return document.querySelector(
      `.dep-arc[data-arc-id="${arcId}"], .cycle-arc[data-arc-id="${arcId}"]`
    );
  }

  // Bring element to front (SVG z-order = DOM order)
  function bringToFront(element) {
    if (element) element.parentNode.appendChild(element);
  }

  // Dim all non-highlighted elements (except toolbar and hitareas)
  function dimNonHighlighted() {
    document.querySelectorAll(
      'rect:not(.highlighted-node):not(.toolbar-btn):not(.toolbar-checkbox):not(.arc-count-bg), ' +
      'path:not(.highlighted):not(.arc-hitarea):not(.virtual-hitarea), ' +
      'polygon:not(.highlighted-arrow), ' +
      'text.arc-count:not(.highlighted)'
    ).forEach(el => {
      if (!el.closest('.view-options')) el.classList.add('dimmed');
    });
  }

  function applyEdgeHighlight(from, to) {
    const arcId = from + '-' + to;
    document.getElementById('node-' + from)?.classList.add('highlighted-node');
    document.getElementById('node-' + to)?.classList.add('highlighted-node');
    // Highlight visible arc (not hitarea) via data-arc-id
    getVisibleArc(arcId)?.classList.add('highlighted');
    // Virtual arcs
    document.querySelectorAll('.virtual-arc[data-from="' + from + '"][data-to="' + to + '"]')
      .forEach(el => el.classList.add('highlighted'));
    document.querySelectorAll('[data-edge="' + arcId + '"]')
      .forEach(el => el.classList.add('highlighted-arrow'));
    document.querySelectorAll('[data-vedge="' + arcId + '"]:not(.arc-count)')
      .forEach(el => el.classList.add('highlighted-arrow'));
    // Arc-count labels get 'highlighted' class (not highlighted-arrow)
    document.querySelectorAll('.arc-count[data-vedge="' + arcId + '"]')
      .forEach(el => el.classList.add('highlighted'));

    // Bring highlighted arc to front (hit-area last = topmost)
    bringToFront(getVisibleArc(arcId));
    bringToFront(document.querySelector(`.virtual-arc[data-arc-id="${arcId}"]`));
    bringToFront(document.querySelector(`.arc-count-group[data-vedge="${arcId}"]`));
    bringToFront(document.querySelector(`.arc-hitarea[data-arc-id="${arcId}"]`));

    dimNonHighlighted();
  }

  function applyNodeHighlight(nodeId) {
    document.getElementById('node-' + nodeId)?.classList.add('highlighted-node');
    // Select hitareas (they have data-from/data-to), then highlight corresponding visible arcs
    document.querySelectorAll('.arc-hitarea[data-from="' + nodeId + '"], .arc-hitarea[data-to="' + nodeId + '"]')
      .forEach(hitarea => {
        const arcId = hitarea.dataset.arcId;
        const visibleArc = getVisibleArc(arcId);
        visibleArc?.classList.add('highlighted');
        const from = hitarea.dataset.from;
        const to = hitarea.dataset.to;
        document.getElementById('node-' + from)?.classList.add('highlighted-node');
        document.getElementById('node-' + to)?.classList.add('highlighted-node');
        document.querySelectorAll('[data-edge="' + from + '-' + to + '"]')
          .forEach(arr => arr.classList.add('highlighted-arrow'));
      });
    // Also handle virtual arcs
    document.querySelectorAll('.virtual-arc[data-from="' + nodeId + '"], .virtual-arc[data-to="' + nodeId + '"]')
      .forEach(arc => {
        arc.classList.add('highlighted');
        const from = arc.dataset.from;
        const to = arc.dataset.to;
        document.getElementById('node-' + from)?.classList.add('highlighted-node');
        document.getElementById('node-' + to)?.classList.add('highlighted-node');
        document.querySelectorAll('[data-vedge="' + from + '-' + to + '"]:not(.arc-count)')
          .forEach(arr => arr.classList.add('highlighted-arrow'));
        // Arc-count labels get 'highlighted' class
        document.querySelectorAll('.arc-count[data-vedge="' + from + '-' + to + '"]')
          .forEach(el => el.classList.add('highlighted'));
      });

    // Bring all highlighted arcs to front (hit-areas last = topmost)
    document.querySelectorAll('.arc-hitarea[data-from="' + nodeId + '"], .arc-hitarea[data-to="' + nodeId + '"]')
      .forEach(hitarea => {
        bringToFront(getVisibleArc(hitarea.dataset.arcId));
        bringToFront(hitarea);
      });

    dimNonHighlighted();
  }

  function highlightEdge(from, to, pin) {
    const edgeId = from + '-' + to;
    // Toggle-check: if same edge is already pinned, deselect
    if (pin && pinnedHighlight && pinnedHighlight.type === 'edge' && pinnedHighlight.id === edgeId) {
      pinnedHighlight = null;
      clearHighlights();
      return;
    }
    clearHighlights();
    applyEdgeHighlight(from, to);
    if (pin) pinnedHighlight = {type: 'edge', id: edgeId};
  }

  function highlightNode(nodeId, pin) {
    // Toggle-check: if same node is already pinned, deselect
    if (pin && pinnedHighlight && pinnedHighlight.type === 'node' && pinnedHighlight.id === nodeId) {
      pinnedHighlight = null;
      clearHighlights();
      return;
    }
    clearHighlights();
    applyNodeHighlight(nodeId);
    if (pin) pinnedHighlight = {type: 'node', id: nodeId};
  }

  function handleMouseEnter(type, id) {
    if (pinnedHighlight) return; // Don't preview if something is pinned
    clearHighlights();
    if (type === 'node') applyNodeHighlight(id);
    else if (type === 'edge') {
      const [from, to] = id.split('-');
      applyEdgeHighlight(from, to);
    }
  }

  function handleMouseLeave() {
    if (pinnedHighlight) return; // Keep pinned highlight
    clearHighlights();
  }

  // === Collapse functionality ===
  const collapseState = new Map(); // nodeId -> boolean (collapsed)
  const originalPositions = new Map(); // nodeId -> {x, y}

  // Store original positions on load
  document.querySelectorAll('.crate, .module').forEach(node => {
    const id = node.id.replace('node-', '');
    originalPositions.set(id, {
      x: parseFloat(node.getAttribute('x')),
      y: parseFloat(node.getAttribute('y'))
    });
  });

  // Get all descendants recursively (transitive)
  function getDescendants(nodeId) {
    const descendants = [];
    document.querySelectorAll('[data-parent="' + nodeId + '"]').forEach(child => {
      if (child.tagName === 'rect') {
        const childId = child.id.replace('node-', '');
        descendants.push(childId);
        descendants.push(...getDescendants(childId));
      }
    });
    return descendants;
  }

  // Find the nearest visible ancestor (node without .collapsed class)
  function getVisibleAncestor(nodeId) {
    const node = document.getElementById('node-' + nodeId);
    if (!node) return null;
    // If this node is hidden, find its visible parent
    if (node.classList.contains('collapsed')) {
      const parentId = node.dataset.parent;
      if (!parentId) return null;  // No visible ancestor
      return getVisibleAncestor(parentId);
    }
    return nodeId;  // This node is visible
  }

  // Count all descendants
  function countDescendants(nodeId) {
    return getDescendants(nodeId).length;
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
        return originalPositions.get(aId).y - originalPositions.get(bId).y;
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
    if (pinnedHighlight) {
      if (pinnedHighlight.type === 'node') {
        applyNodeHighlight(pinnedHighlight.id);
      } else if (pinnedHighlight.type === 'edge') {
        const [from, to] = pinnedHighlight.id.split('-');
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

  // Recalculate and show virtual edges for collapsed nodes
  function recalculateVirtualEdges() {
    // Remove existing virtual elements (hitareas, visible arcs, counts)
    document.querySelectorAll('.virtual-arc, .virtual-hitarea, .virtual-arrow, .arc-count, .arc-count-group, .arc-count-bg').forEach(el => el.remove());

    // FIRST: Reset ALL edges (hitareas + visible) and arrows to visible
    document.querySelectorAll('.arc-hitarea, .dep-arc, .cycle-arc').forEach(edge => {
      edge.style.display = '';
    });
    document.querySelectorAll('.dep-arrow, .cycle-arrow').forEach(arrow => {
      arrow.style.display = '';
    });

    // Find all original hitareas and update paths / determine which need to be hidden
    const hitareas = document.querySelectorAll('.arc-hitarea');
    const virtualEdges = new Map(); // "visibleFrom-visibleTo" -> { count, hiddenEdgeData }

    hitareas.forEach(hitarea => {
      const arcId = hitarea.dataset.arcId;
      const fromId = hitarea.dataset.from;
      const toId = hitarea.dataset.to;
      const fromNode = document.getElementById('node-' + fromId);
      const toNode = document.getElementById('node-' + toId);

      // Check if nodes are hidden (have .collapsed class)
      const fromHidden = fromNode && fromNode.classList.contains('collapsed');
      const toHidden = toNode && toNode.classList.contains('collapsed');

      if (fromHidden || toHidden) {
        // Hide original hitarea, visible edge, and arrows
        hitarea.style.display = 'none';
        const visibleArc = getVisibleArc(arcId);
        if (visibleArc) visibleArc.style.display = 'none';
        document.querySelectorAll('[data-edge="' + fromId + '-' + toId + '"]')
          .forEach(arr => arr.style.display = 'none');

        // Calculate visible endpoints for virtual edge
        const visibleFrom = fromHidden ? getVisibleAncestor(fromId) : fromId;
        const visibleTo = toHidden ? getVisibleAncestor(toId) : toId;

        if (visibleFrom && visibleTo && visibleFrom !== visibleTo) {
          const key = visibleFrom + '-' + visibleTo;
          const existing = virtualEdges.get(key) || { count: 0, hiddenEdgeData: [] };
          existing.count++;
          // Collect source locations from hidden hitarea
          const locs = hitarea.dataset.sourceLocations;
          if (locs) existing.hiddenEdgeData.push(locs);
          virtualEdges.set(key, existing);
        }
      } else if (fromNode && toNode) {
        // Both endpoints visible - update arc paths to current node positions
        const arc = calculateArcPathFromNodes(fromNode, toNode, 3);
        hitarea.setAttribute('d', arc.path);
        const visibleArc = getVisibleArc(arcId);
        if (visibleArc) visibleArc.setAttribute('d', arc.path);

        // Update arrow position
        const arrows = document.querySelectorAll('[data-edge="' + fromId + '-' + toId + '"]');
        arrows.forEach(arrow => {
          arrow.setAttribute('points', `${arc.toX + 8},${arc.toY - 4} ${arc.toX},${arc.toY} ${arc.toX + 8},${arc.toY + 4}`);
        });
      }
    });

    // Create virtual edges with proper Z-order: arcs → labels → hitareas
    const depsGroup = document.getElementById('dependencies');

    // Find rightmost node edge (once, for all arcs)
    let maxRight = 0;
    document.querySelectorAll('.crate, .module').forEach(n => {
      if (!n.classList.contains('collapsed')) {
        const right = parseFloat(n.getAttribute('x')) + parseFloat(n.getAttribute('width'));
        if (right > maxRight) maxRight = right;
      }
    });

    // Prepare arc data for all edges
    const mergedEdges = new Map();
    virtualEdges.forEach((data, key) => {
      const [fromId, toId] = key.split('-');
      const fromNode = document.getElementById('node-' + fromId);
      const toNode = document.getElementById('node-' + toId);

      if (fromNode && toNode) {
        const fromX = parseFloat(fromNode.getAttribute('x')) + parseFloat(fromNode.getAttribute('width'));
        const fromY = parseFloat(fromNode.getAttribute('y')) + parseFloat(fromNode.getAttribute('height')) / 2 + 3;
        const toX = parseFloat(toNode.getAttribute('x')) + parseFloat(toNode.getAttribute('width'));
        const toY = parseFloat(toNode.getAttribute('y')) + parseFloat(toNode.getAttribute('height')) / 2 - 3;

        const arc = ArcLogic.calculateArcPath(fromX, fromY, toX, toY, maxRight, ROW_HEIGHT);
        mergedEdges.set(key, { ...data, fromId, toId, arc });
      }
    });

    // Pass 1: Arcs + Arrows (bottom layer)
    mergedEdges.forEach((data, key) => {
      const { fromId, toId, arc } = data;
      const arcId = fromId + '-' + toId;

      // Visible path (styled, no pointer events)
      const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      path.setAttribute('class', 'virtual-arc');
      path.setAttribute('d', arc.path);
      path.setAttribute('data-arc-id', arcId);
      path.setAttribute('data-from', fromId);
      path.setAttribute('data-to', toId);
      depsGroup.appendChild(path);

      // Arrow
      const arrow = document.createElementNS('http://www.w3.org/2000/svg', 'polygon');
      arrow.setAttribute('class', 'virtual-arrow');
      arrow.setAttribute('data-vedge', arcId);
      arrow.setAttribute('data-from', fromId);
      arrow.setAttribute('data-to', toId);
      arrow.setAttribute('points', `${arc.toX + 8},${arc.toY - 4} ${arc.toX},${arc.toY} ${arc.toX + 8},${arc.toY + 4}`);
      arrow.addEventListener('click', e => {
        e.stopPropagation();
        highlightVirtualEdge(fromId, toId, data.count);
      });
      depsGroup.appendChild(arrow);
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

        depsGroup.appendChild(labelGroup);
      }
    });

    // Pass 3: Hitareas (top layer, for pointer events)
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
        if (pinnedHighlight) {
          const visibleArc = document.querySelector(`.virtual-arc[data-arc-id="${arcId}"]`);
          if (!visibleArc?.classList.contains('highlighted')) {
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
      depsGroup.appendChild(hitarea);
    });
  }

  // Highlight virtual (aggregated) edge
  function highlightVirtualEdge(fromId, toId, count) {
    const edgeId = fromId + '-' + toId;
    // Toggle-check: if same edge is already pinned, deselect
    if (pinnedHighlight && pinnedHighlight.type === 'edge' && pinnedHighlight.id === edgeId) {
      pinnedHighlight = null;
      clearHighlights();
      return;
    }
    clearHighlights();
    pinnedHighlight = {type: 'edge', id: edgeId};
    document.getElementById('node-' + fromId)?.classList.add('highlighted-node');
    document.getElementById('node-' + toId)?.classList.add('highlighted-node');
    // Highlight the virtual arc
    document.querySelectorAll('.virtual-arc[data-from="' + fromId + '"][data-to="' + toId + '"]')
      .forEach(el => el.classList.add('highlighted'));
    document.querySelectorAll('.virtual-arrow[data-vedge="' + fromId + '-' + toId + '"]')
      .forEach(el => el.classList.add('highlighted-arrow'));
    // Highlight arc-count labels
    document.querySelectorAll('.arc-count[data-vedge="' + fromId + '-' + toId + '"]')
      .forEach(el => el.classList.add('highlighted'));
    // Bring virtual arc, label group and hitarea to front
    bringToFront(document.querySelector('.virtual-arc[data-arc-id="' + edgeId + '"]'));
    bringToFront(document.querySelector('.arc-count-group[data-vedge="' + edgeId + '"]'));
    bringToFront(document.querySelector('.virtual-hitarea[data-arc-id="' + edgeId + '"]'));
    // Dim everything else
    dimNonHighlighted();
  }

  // Toggle collapse state
  function toggleCollapse(nodeId) {
    const collapsed = !collapseState.get(nodeId);
    collapseState.set(nodeId, collapsed);

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
          if (collapseState.get(parentId) && parentId !== nodeId) {
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
      .every(node => !collapseState.get(node.id.replace('node-', '')));

    document.querySelectorAll('[data-has-children="true"]').forEach(node => {
      const nodeId = node.id.replace('node-', '');
      if (allExpanded) {
        // Collapse all
        collapseState.set(nodeId, true);
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
        collapseState.set(nodeId, false);
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
      if (pinnedHighlight) {
        const arcId = hitarea.dataset.arcId;
        const visibleArc = getVisibleArc(arcId);
        if (!visibleArc?.classList.contains('highlighted')) {
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
    pinnedHighlight = null;
    clearHighlights();
  });
})();
}
