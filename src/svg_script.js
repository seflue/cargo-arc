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
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') module.exports = { ArcLogic };

// IIFE for SVG embedding (DOM-code) - only runs in browser with placeholders replaced
if (typeof document !== 'undefined') {
(function() {
  const ROW_HEIGHT = __ROW_HEIGHT__;
  const MARGIN = __MARGIN__;

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

  function applyEdgeHighlight(from, to) {
    document.getElementById('node-' + from)?.classList.add('highlighted-node');
    document.getElementById('node-' + to)?.classList.add('highlighted-node');
    // Highlight both regular edges (by id) and virtual arcs (by data attributes)
    document.getElementById('edge-' + from + '-' + to)?.classList.add('highlighted');
    document.querySelectorAll('.virtual-arc[data-from="' + from + '"][data-to="' + to + '"]')
      .forEach(el => el.classList.add('highlighted'));
    document.querySelectorAll('[data-edge="' + from + '-' + to + '"]')
      .forEach(el => el.classList.add('highlighted-arrow'));
    document.querySelectorAll('[data-vedge="' + from + '-' + to + '"]')
      .forEach(el => el.classList.add('highlighted-arrow'));
    document.querySelectorAll('rect:not(.highlighted-node), path:not(.highlighted), polygon:not(.highlighted-arrow)')
      .forEach(el => el.classList.add('dimmed'));
  }

  function applyNodeHighlight(nodeId) {
    document.getElementById('node-' + nodeId)?.classList.add('highlighted-node');
    document.querySelectorAll('[data-from="' + nodeId + '"], [data-to="' + nodeId + '"]')
      .forEach(edge => {
        edge.classList.add('highlighted');
        const from = edge.dataset.from;
        const to = edge.dataset.to;
        document.getElementById('node-' + from)?.classList.add('highlighted-node');
        document.getElementById('node-' + to)?.classList.add('highlighted-node');
        document.querySelectorAll('[data-edge="' + from + '-' + to + '"]')
          .forEach(arr => arr.classList.add('highlighted-arrow'));
      });
    document.querySelectorAll('rect:not(.highlighted-node), path:not(.highlighted), polygon:not(.highlighted-arrow)')
      .forEach(el => el.classList.add('dimmed'));
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
    let currentY = MARGIN;

    // Get all nodes sorted by original Y position
    const items = [...document.querySelectorAll('.crate, .module')]
      .sort((a, b) => {
        const aId = a.id.replace('node-', '');
        const bId = b.id.replace('node-', '');
        return originalPositions.get(aId).y - originalPositions.get(bId).y;
      });

    items.forEach(node => {
      if (node.classList.contains('collapsed')) return;

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
    // Remove existing virtual edges
    document.querySelectorAll('.virtual-arc, .arc-count').forEach(el => el.remove());

    // FIRST: Reset ALL edges and arrows to visible
    document.querySelectorAll('.dep-arc, .cycle-arc').forEach(edge => {
      edge.style.display = '';
    });
    document.querySelectorAll('.dep-arrow, .cycle-arrow').forEach(arrow => {
      arrow.style.display = '';
    });

    // Find all original edges and update their paths / determine which need to be hidden
    const edges = document.querySelectorAll('.dep-arc, .cycle-arc');
    const virtualEdges = new Map(); // "visibleFrom-visibleTo" -> { count, hiddenEdgeData }

    edges.forEach(edge => {
      const fromId = edge.dataset.from;
      const toId = edge.dataset.to;
      const fromNode = document.getElementById('node-' + fromId);
      const toNode = document.getElementById('node-' + toId);

      // Check if nodes are hidden (have .collapsed class)
      const fromHidden = fromNode && fromNode.classList.contains('collapsed');
      const toHidden = toNode && toNode.classList.contains('collapsed');

      if (fromHidden || toHidden) {
        // Hide original edge and its arrows
        edge.style.display = 'none';
        document.querySelectorAll('[data-edge="' + fromId + '-' + toId + '"]')
          .forEach(arr => arr.style.display = 'none');

        // Calculate visible endpoints for virtual edge
        const visibleFrom = fromHidden ? getVisibleAncestor(fromId) : fromId;
        const visibleTo = toHidden ? getVisibleAncestor(toId) : toId;

        if (visibleFrom && visibleTo && visibleFrom !== visibleTo) {
          const key = visibleFrom + '-' + visibleTo;
          const existing = virtualEdges.get(key) || { count: 0, hiddenEdgeData: [] };
          existing.count++;
          // Collect source locations from hidden edge
          const locs = edge.dataset.sourceLocations;
          if (locs) existing.hiddenEdgeData.push(locs);
          virtualEdges.set(key, existing);
        }
      } else if (fromNode && toNode) {
        // Both endpoints visible - update arc path to current node positions
        const arc = calculateArcPathFromNodes(fromNode, toNode, 3);
        edge.setAttribute('d', arc.path);

        // Update arrow position
        const arrows = document.querySelectorAll('[data-edge="' + fromId + '-' + toId + '"]');
        arrows.forEach(arrow => {
          arrow.setAttribute('points', `${arc.toX + 8},${arc.toY - 4} ${arc.toX},${arc.toY} ${arc.toX + 8},${arc.toY + 4}`);
        });
      }
    });

    // Create virtual edges
    const depsGroup = document.getElementById('dependencies');
    virtualEdges.forEach((data, key) => {
      const [fromId, toId] = key.split('-');
      const fromNode = document.getElementById('node-' + fromId);
      const toNode = document.getElementById('node-' + toId);

      if (fromNode && toNode) {
        const fromX = parseFloat(fromNode.getAttribute('x')) + parseFloat(fromNode.getAttribute('width'));
        const fromY = parseFloat(fromNode.getAttribute('y')) + parseFloat(fromNode.getAttribute('height')) / 2 + 3;
        const toX = parseFloat(toNode.getAttribute('x')) + parseFloat(toNode.getAttribute('width'));
        const toY = parseFloat(toNode.getAttribute('y')) + parseFloat(toNode.getAttribute('height')) / 2 - 3;

        // Find rightmost node edge for arc positioning
        let maxRight = 0;
        document.querySelectorAll('.crate, .module').forEach(n => {
          if (!n.classList.contains('collapsed')) {
            const right = parseFloat(n.getAttribute('x')) + parseFloat(n.getAttribute('width'));
            if (right > maxRight) maxRight = right;
          }
        });

        const arc = ArcLogic.calculateArcPath(fromX, fromY, toX, toY, maxRight, ROW_HEIGHT);

        const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
        path.setAttribute('class', 'virtual-arc');
        path.setAttribute('d', arc.path);
        path.setAttribute('data-from', fromId);
        path.setAttribute('data-to', toId);
        // Set aggregated source locations from hidden edges
        if (data.hiddenEdgeData.length > 0) {
          path.dataset.sourceLocations = data.hiddenEdgeData.join('|');
        }
        path.style.cursor = 'pointer';
        // Click handler for highlighting
        path.addEventListener('click', e => {
          e.stopPropagation();
          highlightVirtualEdge(fromId, toId, data.count);
        });
        // Hover handlers for floating label
        path.addEventListener('mouseenter', () => handleMouseEnter('edge', fromId + '-' + toId));
        path.addEventListener('mousemove', (e) => {
          const locs = path.dataset.sourceLocations;
          if (locs) {
            const svg = document.querySelector('svg');
            const pt = svg.createSVGPoint();
            pt.x = e.clientX; pt.y = e.clientY;
            const svgPt = pt.matrixTransform(svg.getScreenCTM().inverse());
            showFloatingLabel(locs, svgPt.x + 10, svgPt.y - 20);
          }
        });
        path.addEventListener('mouseleave', () => {
          handleMouseLeave();
          hideFloatingLabel();
        });
        depsGroup.appendChild(path);

        // Arrow
        const arrow = document.createElementNS('http://www.w3.org/2000/svg', 'polygon');
        arrow.setAttribute('class', 'virtual-arc virtual-arrow');
        arrow.setAttribute('data-vedge', fromId + '-' + toId);
        arrow.setAttribute('points', `${arc.toX + 8},${arc.toY - 4} ${arc.toX},${arc.toY} ${arc.toX + 8},${arc.toY + 4}`);
        arrow.style.fill = '#9c27b0';
        arrow.style.cursor = 'pointer';
        arrow.addEventListener('click', e => {
          e.stopPropagation();
          highlightVirtualEdge(fromId, toId, data.count);
        });
        depsGroup.appendChild(arrow);

        // Count label if multiple edges merged
        if (data.count > 1) {
          const countLabel = document.createElementNS('http://www.w3.org/2000/svg', 'text');
          countLabel.setAttribute('class', 'arc-count');
          countLabel.setAttribute('data-vedge', fromId + '-' + toId);
          countLabel.setAttribute('x', arc.ctrlX + 5);
          countLabel.setAttribute('y', arc.midY + 3);
          countLabel.textContent = '(' + data.count + ')';
          countLabel.style.cursor = 'pointer';
          countLabel.addEventListener('click', e => {
            e.stopPropagation();
            highlightVirtualEdge(fromId, toId, data.count);
          });
          depsGroup.appendChild(countLabel);
        }
      }
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
    // Dim everything else
    document.querySelectorAll('rect:not(.highlighted-node), path:not(.highlighted), polygon:not(.highlighted-arrow)')
      .forEach(el => el.classList.add('dimmed'));
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
      });
    }
  });

  document.querySelectorAll('.collapse-toggle').forEach(toggle => {
    toggle.addEventListener('click', e => {
      e.stopPropagation();
      toggleCollapse(toggle.dataset.target);
    });
  });

  document.querySelectorAll('.dep-arc, .cycle-arc').forEach(arc => {
    const edgeId = arc.dataset.from + '-' + arc.dataset.to;

    arc.addEventListener('click', e => {
      e.stopPropagation();
      highlightEdge(arc.dataset.from, arc.dataset.to, true); // pin
    });

    arc.addEventListener('mouseenter', () => handleMouseEnter('edge', edgeId));

    arc.addEventListener('mousemove', (e) => {
      const locs = arc.dataset.sourceLocations;
      if (locs) {
        const svg = document.querySelector('svg');
        const pt = svg.createSVGPoint();
        pt.x = e.clientX; pt.y = e.clientY;
        const svgPt = pt.matrixTransform(svg.getScreenCTM().inverse());
        showFloatingLabel(locs, svgPt.x + 10, svgPt.y - 20);
      }
    });

    arc.addEventListener('mouseleave', () => {
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
