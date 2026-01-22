//! SVG Generation

use crate::layout::{EdgeKind, ItemKind, LayoutIR, NodeId};
use std::collections::HashSet;

/// Character width for monospace 12px font
const CHAR_WIDTH: f32 = 7.2;

/// Calculate text width based on character count
fn calculate_text_width(text: &str) -> f32 {
    text.len() as f32 * CHAR_WIDTH
}

/// Padding for boxes (10px left + 10px right)
const BOX_PADDING: f32 = 20.0;

/// Calculate uniform box width from longest label in LayoutIR
fn calculate_box_width(ir: &LayoutIR) -> f32 {
    ir.items
        .iter()
        .map(|item| calculate_text_width(&item.label))
        .fold(0.0_f32, |a, b| a.max(b))
        + BOX_PADDING
}

/// Arc base offset and scale per hop
const ARC_BASE: f32 = 20.0;
const ARC_SCALE: f32 = 15.0;
const ARROW_SIZE: f32 = 8.0;

/// Calculate maximum arc width from edges
fn calculate_max_arc_width(positioned: &[PositionedItem], ir: &LayoutIR, row_height: f32) -> f32 {
    ir.edges
        .iter()
        .filter_map(|edge| {
            let from = positioned.iter().find(|p| p.id == edge.from)?;
            let to = positioned.iter().find(|p| p.id == edge.to)?;
            let hops = ((to.y - from.y).abs() / row_height).round().max(1.0);
            Some(ARC_BASE + hops * ARC_SCALE + ARROW_SIZE)
        })
        .fold(0.0_f32, |a, b| a.max(b))
}

/// Configuration for SVG rendering
#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub row_height: f32,
    pub indent_size: f32,
    pub margin: f32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            row_height: 30.0,
            indent_size: 20.0,
            margin: 20.0,
        }
    }
}

/// Positioned item for rendering
struct PositionedItem {
    id: NodeId,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    label: String,
    kind: ItemKind,
}

/// Render LayoutIR to SVG string
pub fn render(ir: &LayoutIR, config: &RenderConfig) -> String {
    let box_width = calculate_box_width(ir);
    let positioned = calculate_positions(ir, config, box_width);
    let max_arc_width = calculate_max_arc_width(&positioned, ir, config.row_height);
    let (width, height) = calculate_canvas_size(&positioned, config, max_arc_width);

    // Collect all node IDs that are parents (have children)
    let parents: HashSet<NodeId> = ir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Module { parent, .. } => Some(*parent),
            ItemKind::Crate => None,
        })
        .collect();

    let mut svg = String::new();
    svg.push_str(&render_header(width, height));
    svg.push_str(&render_styles());
    svg.push_str(&render_tree_lines(&positioned, ir));
    svg.push_str(&render_nodes(&positioned, &parents));
    svg.push_str(&render_edges(&positioned, ir));
    svg.push_str(&render_script(config));
    svg.push_str("</svg>\n");
    svg
}

fn calculate_positions(
    ir: &LayoutIR,
    config: &RenderConfig,
    box_width: f32,
) -> Vec<PositionedItem> {
    ir.items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let nesting = match &item.kind {
                ItemKind::Crate => 0,
                ItemKind::Module { nesting, .. } => *nesting,
            };
            let height = match &item.kind {
                ItemKind::Crate => 24.0,
                ItemKind::Module { .. } => 20.0,
            };
            PositionedItem {
                id: item.id,
                x: config.margin + (nesting as f32 * config.indent_size),
                y: config.margin + (index as f32 * config.row_height),
                width: box_width,
                height,
                label: item.label.clone(),
                kind: item.kind.clone(),
            }
        })
        .collect()
}

fn calculate_canvas_size(
    positioned: &[PositionedItem],
    config: &RenderConfig,
    max_arc_width: f32,
) -> (f32, f32) {
    let height = if positioned.is_empty() {
        config.margin * 2.0
    } else {
        config.margin * 2.0 + positioned.len() as f32 * config.row_height
    };
    // Width: max(box_right_edge) + arc_space + margin
    let max_x = positioned
        .iter()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, |a, b| a.max(b));
    // Use calculated max_arc_width, with a minimum buffer for short/no edges
    let arc_space = max_arc_width.max(50.0);
    let width = max_x + arc_space + config.margin;
    (width, height)
}

fn render_header(width: f32, height: f32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
"#
    )
}

fn render_styles() -> String {
    r#"  <style>
    .crate { fill: #e3f2fd; stroke: #1976d2; stroke-width: 2; }
    .module { fill: #fff3e0; stroke: #f57c00; stroke-width: 1; }
    .label { font-family: monospace; font-size: 12px; }
    .tree-line { stroke: #666; stroke-width: 1; }
    .dep-arc { fill: none; stroke: #9c27b0; stroke-width: 1.5; }
    .dep-arrow { fill: #9c27b0; }
    .cycle-arc { fill: none; stroke: #f44336; stroke-width: 1.5; }
    .cycle-arrow { fill: #f44336; }
    /* Interactive highlighting */
    .highlighted { stroke: #ff9800 !important; stroke-width: 3 !important; }
    .highlighted-node { fill: #fff176 !important; stroke: #ff9800 !important; stroke-width: 3 !important; }
    .highlighted-arrow { fill: #ff9800 !important; }
    .dimmed { opacity: 0.3; }
    .crate, .module, .dep-arc, .cycle-arc { cursor: pointer; }
    /* Collapse functionality */
    .collapse-toggle { font-family: monospace; font-size: 14px; cursor: pointer; fill: #666; }
    .collapse-toggle:hover { fill: #1976d2; }
    .collapsed { display: none; }
    .virtual-arc { fill: none; stroke: #9c27b0; stroke-width: 2; stroke-dasharray: 4,2; }
    .arc-count { font-family: monospace; font-size: 10px; fill: #9c27b0; text-anchor: middle; }
    .child-count { font-size: 10px; fill: #888; }
    /* Floating label for source locations */
    .floating-label { pointer-events: none; }
    .floating-label rect { fill: #1a1a2e; fill-opacity: 0.95; rx: 4; }
    .floating-label text { fill: #e0e0e0; font-family: monospace; font-size: 11px; }
  </style>
"#
    .to_string()
}

fn render_script(config: &RenderConfig) -> String {
    format!(
        r#"  <script><![CDATA[
(function() {{
  const ROW_HEIGHT = {row_height};
  const MARGIN = {margin};

  // === Floating label for source locations ===
  let floatingLabel = null;

  function showFloatingLabel(text, x, y) {{
    hideFloatingLabel();
    const svg = document.querySelector('svg');
    floatingLabel = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    floatingLabel.setAttribute('class', 'floating-label');

    const padding = 6;
    const lineHeight = 14;
    const lines = text.split(', ');

    const textEl = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    textEl.setAttribute('x', x + padding);
    textEl.setAttribute('y', y + lineHeight);

    // Create tspan for each line
    lines.forEach((line, i) => {{
      const tspan = document.createElementNS('http://www.w3.org/2000/svg', 'tspan');
      tspan.setAttribute('x', x + padding);
      tspan.setAttribute('dy', i === 0 ? 0 : lineHeight);
      tspan.textContent = line;
      textEl.appendChild(tspan);
    }});

    // Measure width
    svg.appendChild(textEl);
    const bbox = textEl.getBBox();
    svg.removeChild(textEl);

    const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
    rect.setAttribute('x', x);
    rect.setAttribute('y', y);
    rect.setAttribute('width', bbox.width + padding * 2);
    rect.setAttribute('height', lines.length * lineHeight + padding);

    floatingLabel.appendChild(rect);
    floatingLabel.appendChild(textEl);
    svg.appendChild(floatingLabel);
  }}

  function hideFloatingLabel() {{
    if (floatingLabel) {{ floatingLabel.remove(); floatingLabel = null; }}
  }}

  // === Highlight functionality ===
  let pinnedHighlight = null; // {{type: 'node'|'edge', id: string}} or null

  function clearHighlights() {{
    document.querySelectorAll('.highlighted, .highlighted-node, .highlighted-arrow, .dimmed')
      .forEach(el => el.classList.remove('highlighted', 'highlighted-node', 'highlighted-arrow', 'dimmed'));
  }}

  function applyEdgeHighlight(from, to) {{
    document.getElementById('node-' + from)?.classList.add('highlighted-node');
    document.getElementById('node-' + to)?.classList.add('highlighted-node');
    document.getElementById('edge-' + from + '-' + to)?.classList.add('highlighted');
    document.querySelectorAll('[data-edge="' + from + '-' + to + '"]')
      .forEach(el => el.classList.add('highlighted-arrow'));
    document.querySelectorAll('rect:not(.highlighted-node), path:not(.highlighted), polygon:not(.highlighted-arrow)')
      .forEach(el => el.classList.add('dimmed'));
  }}

  function applyNodeHighlight(nodeId) {{
    document.getElementById('node-' + nodeId)?.classList.add('highlighted-node');
    document.querySelectorAll('[data-from="' + nodeId + '"], [data-to="' + nodeId + '"]')
      .forEach(edge => {{
        edge.classList.add('highlighted');
        const from = edge.dataset.from;
        const to = edge.dataset.to;
        document.getElementById('node-' + from)?.classList.add('highlighted-node');
        document.getElementById('node-' + to)?.classList.add('highlighted-node');
        document.querySelectorAll('[data-edge="' + from + '-' + to + '"]')
          .forEach(arr => arr.classList.add('highlighted-arrow'));
      }});
    document.querySelectorAll('rect:not(.highlighted-node), path:not(.highlighted), polygon:not(.highlighted-arrow)')
      .forEach(el => el.classList.add('dimmed'));
  }}

  function highlightEdge(from, to, pin) {{
    const edgeId = from + '-' + to;
    // Toggle-check: if same edge is already pinned, deselect
    if (pin && pinnedHighlight && pinnedHighlight.type === 'edge' && pinnedHighlight.id === edgeId) {{
      pinnedHighlight = null;
      clearHighlights();
      return;
    }}
    clearHighlights();
    applyEdgeHighlight(from, to);
    if (pin) pinnedHighlight = {{type: 'edge', id: edgeId}};
  }}

  function highlightNode(nodeId, pin) {{
    // Toggle-check: if same node is already pinned, deselect
    if (pin && pinnedHighlight && pinnedHighlight.type === 'node' && pinnedHighlight.id === nodeId) {{
      pinnedHighlight = null;
      clearHighlights();
      return;
    }}
    clearHighlights();
    applyNodeHighlight(nodeId);
    if (pin) pinnedHighlight = {{type: 'node', id: nodeId}};
  }}

  function handleMouseEnter(type, id) {{
    if (pinnedHighlight) return; // Don't preview if something is pinned
    clearHighlights();
    if (type === 'node') applyNodeHighlight(id);
    else if (type === 'edge') {{
      const [from, to] = id.split('-');
      applyEdgeHighlight(from, to);
    }}
  }}

  function handleMouseLeave() {{
    if (pinnedHighlight) return; // Keep pinned highlight
    clearHighlights();
  }}

  // === Collapse functionality ===
  const collapseState = new Map(); // nodeId -> boolean (collapsed)
  const originalPositions = new Map(); // nodeId -> {{x, y}}

  // Store original positions on load
  document.querySelectorAll('.crate, .module').forEach(node => {{
    const id = node.id.replace('node-', '');
    originalPositions.set(id, {{
      x: parseFloat(node.getAttribute('x')),
      y: parseFloat(node.getAttribute('y'))
    }});
  }});

  // Get all descendants recursively (transitive)
  function getDescendants(nodeId) {{
    const descendants = [];
    document.querySelectorAll('[data-parent="' + nodeId + '"]').forEach(child => {{
      if (child.tagName === 'rect') {{
        const childId = child.id.replace('node-', '');
        descendants.push(childId);
        descendants.push(...getDescendants(childId));
      }}
    }});
    return descendants;
  }}

  // Find the nearest visible ancestor (node without .collapsed class)
  function getVisibleAncestor(nodeId) {{
    const node = document.getElementById('node-' + nodeId);
    if (!node) return null;
    // If this node is hidden, find its visible parent
    if (node.classList.contains('collapsed')) {{
      const parentId = node.dataset.parent;
      if (!parentId) return null;  // No visible ancestor
      return getVisibleAncestor(parentId);
    }}
    return nodeId;  // This node is visible
  }}

  // Count all descendants
  function countDescendants(nodeId) {{
    return getDescendants(nodeId).length;
  }}

  // Update tree lines for a node at new Y position
  function updateTreeLines(nodeId, newY, nodeHeight) {{
    // Update lines where this node is the child
    document.querySelectorAll('line[data-child="' + nodeId + '"]').forEach(line => {{
      const midY = newY + nodeHeight / 2;
      if (line.getAttribute('x1') === line.getAttribute('x2')) {{
        // Vertical line - update y2
        line.setAttribute('y2', midY);
      }} else {{
        // Horizontal line - update both y1 and y2
        line.setAttribute('y1', midY);
        line.setAttribute('y2', midY);
      }}
    }});

    // Update lines where this node is the parent (vertical line y1)
    document.querySelectorAll('line[data-parent="' + nodeId + '"]').forEach(line => {{
      if (line.getAttribute('x1') === line.getAttribute('x2')) {{
        // Vertical line - update y1 (parent bottom)
        line.setAttribute('y1', newY + nodeHeight);
      }}
    }});
  }}

  // Relayout visible nodes
  function relayout() {{
    let currentY = MARGIN;

    // Get all nodes sorted by original Y position
    const items = [...document.querySelectorAll('.crate, .module')]
      .sort((a, b) => {{
        const aId = a.id.replace('node-', '');
        const bId = b.id.replace('node-', '');
        return originalPositions.get(aId).y - originalPositions.get(bId).y;
      }});

    items.forEach(node => {{
      if (node.classList.contains('collapsed')) return;

      const nodeId = node.id.replace('node-', '');
      const height = parseFloat(node.getAttribute('height'));

      // Update rect position
      node.setAttribute('y', currentY);

      // Update label position (next text sibling)
      const label = node.nextElementSibling;
      if (label && label.tagName === 'text' && label.classList.contains('label')) {{
        label.setAttribute('y', currentY + height / 2 + 4);
      }}

      // Update toggle icon position (if exists)
      const toggle = document.querySelector('.collapse-toggle[data-target="' + nodeId + '"]');
      if (toggle) {{
        toggle.setAttribute('y', currentY + height / 2 + 4);
      }}

      // Update tree lines
      updateTreeLines(nodeId, currentY, height);

      currentY += ROW_HEIGHT;
    }});

    recalculateVirtualEdges();
  }}

  // Helper: Calculate arc path between two nodes
  function calculateArcPath(fromNode, toNode, yOffset) {{
    const fromX = parseFloat(fromNode.getAttribute('x')) + parseFloat(fromNode.getAttribute('width'));
    const fromY = parseFloat(fromNode.getAttribute('y')) + parseFloat(fromNode.getAttribute('height')) / 2 + yOffset;
    const toX = parseFloat(toNode.getAttribute('x')) + parseFloat(toNode.getAttribute('width'));
    const toY = parseFloat(toNode.getAttribute('y')) + parseFloat(toNode.getAttribute('height')) / 2 - yOffset;

    // Find rightmost visible node for arc positioning
    let maxRight = 0;
    document.querySelectorAll('.crate, .module').forEach(n => {{
      if (!n.classList.contains('collapsed')) {{
        const right = parseFloat(n.getAttribute('x')) + parseFloat(n.getAttribute('width'));
        if (right > maxRight) maxRight = right;
      }}
    }});

    const hops = Math.max(1, Math.round(Math.abs(toY - fromY) / ROW_HEIGHT));
    const arcOffset = 20 + (hops * 15);
    const ctrlX = maxRight + arcOffset;
    const midY = (fromY + toY) / 2;

    return {{
      path: `M ${{fromX}},${{fromY}} Q ${{ctrlX}},${{fromY}} ${{ctrlX}},${{midY}} Q ${{ctrlX}},${{toY}} ${{toX}},${{toY}}`,
      toX, toY, ctrlX, midY
    }};
  }}

  // Recalculate and show virtual edges for collapsed nodes
  function recalculateVirtualEdges() {{
    // Remove existing virtual edges
    document.querySelectorAll('.virtual-arc, .arc-count').forEach(el => el.remove());

    // FIRST: Reset ALL edges and arrows to visible
    document.querySelectorAll('.dep-arc, .cycle-arc').forEach(edge => {{
      edge.style.display = '';
    }});
    document.querySelectorAll('.dep-arrow, .cycle-arrow').forEach(arrow => {{
      arrow.style.display = '';
    }});

    // Find all original edges and update their paths / determine which need to be hidden
    const edges = document.querySelectorAll('.dep-arc, .cycle-arc');
    const virtualEdges = new Map(); // "visibleFrom-visibleTo" -> {{ count, hiddenEdgeData }}

    edges.forEach(edge => {{
      const fromId = edge.dataset.from;
      const toId = edge.dataset.to;
      const fromNode = document.getElementById('node-' + fromId);
      const toNode = document.getElementById('node-' + toId);

      // Check if nodes are hidden (have .collapsed class)
      const fromHidden = fromNode && fromNode.classList.contains('collapsed');
      const toHidden = toNode && toNode.classList.contains('collapsed');

      if (fromHidden || toHidden) {{
        // Hide original edge and its arrows
        edge.style.display = 'none';
        document.querySelectorAll('[data-edge="' + fromId + '-' + toId + '"]')
          .forEach(arr => arr.style.display = 'none');

        // Calculate visible endpoints for virtual edge
        const visibleFrom = fromHidden ? getVisibleAncestor(fromId) : fromId;
        const visibleTo = toHidden ? getVisibleAncestor(toId) : toId;

        if (visibleFrom && visibleTo && visibleFrom !== visibleTo) {{
          const key = visibleFrom + '-' + visibleTo;
          const existing = virtualEdges.get(key) || {{ count: 0, hiddenEdgeData: [] }};
          existing.count++;
          // Collect source locations from hidden edge
          const locs = edge.dataset.sourceLocations;
          if (locs) existing.hiddenEdgeData.push(locs);
          virtualEdges.set(key, existing);
        }}
      }} else if (fromNode && toNode) {{
        // Both endpoints visible - update arc path to current node positions
        const arc = calculateArcPath(fromNode, toNode, 3);
        edge.setAttribute('d', arc.path);

        // Update arrow position
        const arrows = document.querySelectorAll('[data-edge="' + fromId + '-' + toId + '"]');
        arrows.forEach(arrow => {{
          arrow.setAttribute('points', `${{arc.toX + 8}},${{arc.toY - 4}} ${{arc.toX}},${{arc.toY}} ${{arc.toX + 8}},${{arc.toY + 4}}`);
        }});
      }}
    }});

    // Create virtual edges
    const depsGroup = document.getElementById('dependencies');
    virtualEdges.forEach((data, key) => {{
      const [fromId, toId] = key.split('-');
      const fromNode = document.getElementById('node-' + fromId);
      const toNode = document.getElementById('node-' + toId);

      if (fromNode && toNode) {{
        const fromX = parseFloat(fromNode.getAttribute('x')) + parseFloat(fromNode.getAttribute('width'));
        const fromY = parseFloat(fromNode.getAttribute('y')) + parseFloat(fromNode.getAttribute('height')) / 2 + 3;
        const toX = parseFloat(toNode.getAttribute('x')) + parseFloat(toNode.getAttribute('width'));
        const toY = parseFloat(toNode.getAttribute('y')) + parseFloat(toNode.getAttribute('height')) / 2 - 3;

        // Find rightmost node edge for arc positioning
        let maxRight = 0;
        document.querySelectorAll('.crate, .module').forEach(n => {{
          if (!n.classList.contains('collapsed')) {{
            const right = parseFloat(n.getAttribute('x')) + parseFloat(n.getAttribute('width'));
            if (right > maxRight) maxRight = right;
          }}
        }});

        const hops = Math.max(1, Math.round(Math.abs(toY - fromY) / ROW_HEIGHT));
        const arcOffset = 20 + (hops * 15);
        const ctrlX = maxRight + arcOffset;
        const midY = (fromY + toY) / 2;

        const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
        path.setAttribute('class', 'virtual-arc');
        path.setAttribute('d', `M ${{fromX}},${{fromY}} Q ${{ctrlX}},${{fromY}} ${{ctrlX}},${{midY}} Q ${{ctrlX}},${{toY}} ${{toX}},${{toY}}`);
        path.setAttribute('data-from', fromId);
        path.setAttribute('data-to', toId);
        // Set aggregated source locations from hidden edges
        if (data.hiddenEdgeData.length > 0) {{
          path.dataset.sourceLocations = data.hiddenEdgeData.join(', ');
        }}
        path.style.cursor = 'pointer';
        // Click handler for highlighting
        path.addEventListener('click', e => {{
          e.stopPropagation();
          highlightVirtualEdge(fromId, toId, data.count);
        }});
        // Hover handlers for floating label
        path.addEventListener('mouseenter', (e) => {{
          handleMouseEnter('edge', fromId + '-' + toId);
          const locs = path.dataset.sourceLocations;
          if (locs) {{
            const svg = document.querySelector('svg');
            const pt = svg.createSVGPoint();
            pt.x = e.clientX; pt.y = e.clientY;
            const svgPt = pt.matrixTransform(svg.getScreenCTM().inverse());
            showFloatingLabel(locs, svgPt.x + 10, svgPt.y - 20);
          }}
        }});
        path.addEventListener('mouseleave', () => {{
          handleMouseLeave();
          hideFloatingLabel();
        }});
        depsGroup.appendChild(path);

        // Arrow
        const arrow = document.createElementNS('http://www.w3.org/2000/svg', 'polygon');
        arrow.setAttribute('class', 'virtual-arc virtual-arrow');
        arrow.setAttribute('data-vedge', fromId + '-' + toId);
        arrow.setAttribute('points', `${{toX + 8}},${{toY - 4}} ${{toX}},${{toY}} ${{toX + 8}},${{toY + 4}}`);
        arrow.style.fill = '#9c27b0';
        arrow.style.cursor = 'pointer';
        arrow.addEventListener('click', e => {{
          e.stopPropagation();
          highlightVirtualEdge(fromId, toId, data.count);
        }});
        depsGroup.appendChild(arrow);

        // Count label if multiple edges merged
        if (data.count > 1) {{
          const countLabel = document.createElementNS('http://www.w3.org/2000/svg', 'text');
          countLabel.setAttribute('class', 'arc-count');
          countLabel.setAttribute('data-vedge', fromId + '-' + toId);
          countLabel.setAttribute('x', ctrlX + 5);
          countLabel.setAttribute('y', midY + 3);
          countLabel.textContent = '(' + data.count + ')';
          countLabel.style.cursor = 'pointer';
          countLabel.addEventListener('click', e => {{
            e.stopPropagation();
            highlightVirtualEdge(fromId, toId, data.count);
          }});
          depsGroup.appendChild(countLabel);
        }}
      }}
    }});
  }}

  // Highlight virtual (aggregated) edge
  function highlightVirtualEdge(fromId, toId, count) {{
    const edgeId = fromId + '-' + toId;
    // Toggle-check: if same edge is already pinned, deselect
    if (pinnedHighlight && pinnedHighlight.type === 'edge' && pinnedHighlight.id === edgeId) {{
      pinnedHighlight = null;
      clearHighlights();
      return;
    }}
    clearHighlights();
    pinnedHighlight = {{type: 'edge', id: edgeId}};
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
  }}

  // Toggle collapse state
  function toggleCollapse(nodeId) {{
    const collapsed = !collapseState.get(nodeId);
    collapseState.set(nodeId, collapsed);

    const descendants = getDescendants(nodeId);

    // Toggle visibility of descendants
    descendants.forEach(descId => {{
      const node = document.getElementById('node-' + descId);
      const label = node?.nextElementSibling;
      const toggle = document.querySelector('.collapse-toggle[data-target="' + descId + '"]');

      if (collapsed) {{
        node?.classList.add('collapsed');
        label?.classList.add('collapsed');
        toggle?.classList.add('collapsed');
      }} else {{
        // Only show if no ancestor is collapsed
        let ancestorCollapsed = false;
        let checkId = descId;
        while (true) {{
          const checkNode = document.getElementById('node-' + checkId);
          const parentId = checkNode?.dataset.parent;
          if (!parentId) break;
          if (collapseState.get(parentId) && parentId !== nodeId) {{
            ancestorCollapsed = true;
            break;
          }}
          checkId = parentId;
        }}
        if (!ancestorCollapsed) {{
          node?.classList.remove('collapsed');
          label?.classList.remove('collapsed');
          toggle?.classList.remove('collapsed');
        }}
      }}

      // Hide/show tree lines for descendants
      document.querySelectorAll('line[data-child="' + descId + '"]').forEach(line => {{
        if (collapsed) {{
          line.classList.add('collapsed');
        }} else if (!document.getElementById('node-' + descId)?.classList.contains('collapsed')) {{
          line.classList.remove('collapsed');
        }}
      }});
    }});

    // Update toggle icon
    const toggleIcon = document.querySelector('.collapse-toggle[data-target="' + nodeId + '"]');
    if (toggleIcon) {{
      toggleIcon.textContent = collapsed ? '+' : '−';
    }}

    // Update child count label
    const countLabel = document.getElementById('count-' + nodeId);
    if (countLabel) {{
      if (collapsed) {{
        const count = countDescendants(nodeId);
        countLabel.textContent = ' (+' + count + ')';
      }} else {{
        countLabel.textContent = '';
      }}
    }}

    relayout();
  }}

  // === Event handlers ===
  document.querySelectorAll('.crate, .module').forEach(node => {{
    const nodeId = node.id.replace('node-', '');

    node.addEventListener('click', e => {{
      e.stopPropagation();
      highlightNode(nodeId, true); // pin
    }});

    node.addEventListener('mouseenter', () => handleMouseEnter('node', nodeId));
    node.addEventListener('mouseleave', handleMouseLeave);

    // Double-click to toggle collapse (only for parents)
    if (node.dataset.hasChildren === 'true') {{
      node.addEventListener('dblclick', e => {{
        e.stopPropagation();
        toggleCollapse(nodeId);
      }});
    }}
  }});

  document.querySelectorAll('.collapse-toggle').forEach(toggle => {{
    toggle.addEventListener('click', e => {{
      e.stopPropagation();
      toggleCollapse(toggle.dataset.target);
    }});
  }});

  document.querySelectorAll('.dep-arc, .cycle-arc').forEach(arc => {{
    const edgeId = arc.dataset.from + '-' + arc.dataset.to;

    arc.addEventListener('click', e => {{
      e.stopPropagation();
      highlightEdge(arc.dataset.from, arc.dataset.to, true); // pin
    }});

    arc.addEventListener('mouseenter', (e) => {{
      handleMouseEnter('edge', edgeId);
      const locs = arc.dataset.sourceLocations;
      if (locs) {{
        const svg = document.querySelector('svg');
        const pt = svg.createSVGPoint();
        pt.x = e.clientX; pt.y = e.clientY;
        const svgPt = pt.matrixTransform(svg.getScreenCTM().inverse());
        showFloatingLabel(locs, svgPt.x + 10, svgPt.y - 20);
      }}
    }});

    arc.addEventListener('mouseleave', () => {{
      handleMouseLeave();
      hideFloatingLabel();
    }});
  }});

  document.querySelector('svg').addEventListener('click', () => {{
    pinnedHighlight = null;
    clearHighlights();
  }});
}})();
]]></script>
"#,
        row_height = config.row_height,
        margin = config.margin
    )
}

fn render_tree_lines(
    positioned: &[PositionedItem],
    ir: &LayoutIR,
) -> String {
    let mut lines = String::new();
    lines.push_str("  <g id=\"tree-lines\">\n");

    // Find children for each parent
    for item in &ir.items {
        if let ItemKind::Module { parent, .. } = &item.kind {
            let parent_pos = positioned.iter().find(|p| p.id == *parent);
            let child_pos = positioned.iter().find(|p| p.id == item.id);

            if let (Some(parent_pos), Some(child_pos)) = (parent_pos, child_pos) {
                let line_x = parent_pos.x + 10.0;
                let parent_bottom = parent_pos.y + parent_pos.height;
                let child_mid_y = child_pos.y + child_pos.height / 2.0;

                let data_attrs = format!(r#" data-parent="{}" data-child="{}""#, parent, item.id);

                lines.push_str(&format!(
                    "    <line class=\"tree-line\" x1=\"{line_x}\" y1=\"{parent_bottom}\" x2=\"{line_x}\" y2=\"{child_mid_y}\"{data_attrs}/>\n"
                ));

                let child_left = child_pos.x;
                lines.push_str(&format!(
                    "    <line class=\"tree-line\" x1=\"{line_x}\" y1=\"{child_mid_y}\" x2=\"{child_left}\" y2=\"{child_mid_y}\"{data_attrs}/>\n"
                ));
            }
        }
    }

    lines.push_str("  </g>\n");
    lines
}

fn render_nodes(positioned: &[PositionedItem], parents: &HashSet<NodeId>) -> String {
    let mut nodes = String::new();
    nodes.push_str("  <g id=\"nodes\">\n");

    for item in positioned {
        let class = match &item.kind {
            ItemKind::Crate => "crate",
            ItemKind::Module { .. } => "module",
        };
        let rx = match &item.kind {
            ItemKind::Crate => 3.0,
            ItemKind::Module { .. } => 2.0,
        };

        // data-parent attribute for modules
        let parent_attr = match &item.kind {
            ItemKind::Module { parent, .. } => format!(r#" data-parent="{}""#, parent),
            ItemKind::Crate => String::new(),
        };

        // data-has-children attribute for parents
        let has_children_attr = if parents.contains(&item.id) {
            r#" data-has-children="true""#
        } else {
            ""
        };

        let label = escape_xml(&item.label);
        let text_x = item.x + 10.0;
        let text_y = item.y + item.height / 2.0 + 4.0;

        nodes.push_str(&format!(
            "    <rect class=\"{class}\" id=\"node-{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{rx}\"{parent_attr}{has_children_attr}/>\n",
            item.id, item.x, item.y, item.width, item.height
        ));

        // Label with optional child-count tspan for parents
        if parents.contains(&item.id) {
            nodes.push_str(&format!(
                "    <text class=\"label\" x=\"{text_x}\" y=\"{text_y}\">{label}<tspan id=\"count-{}\" class=\"child-count\"></tspan></text>\n",
                item.id
            ));
        } else {
            nodes.push_str(&format!(
                "    <text class=\"label\" x=\"{text_x}\" y=\"{text_y}\">{label}</text>\n"
            ));
        }

        // Toggle icon (+/-) for parents
        if parents.contains(&item.id) {
            let toggle_x = item.x + item.width - 14.0;
            let toggle_y = item.y + item.height / 2.0 + 4.0;
            nodes.push_str(&format!(
                "    <text class=\"collapse-toggle\" data-target=\"{}\" x=\"{}\" y=\"{}\">−</text>\n",
                item.id, toggle_x, toggle_y
            ));
        }
    }

    nodes.push_str("  </g>\n");
    nodes
}

fn render_edges(positioned: &[PositionedItem], ir: &LayoutIR) -> String {
    let mut edges = String::new();
    edges.push_str("  <g id=\"dependencies\">\n");

    // Find the rightmost edge of all nodes for base arc position
    let base_x = positioned
        .iter()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, |a, b| a.max(b));

    for edge in &ir.edges {
        let from_pos = positioned.iter().find(|p| p.id == edge.from);
        let to_pos = positioned.iter().find(|p| p.id == edge.to);

        if let (Some(from), Some(to)) = (from_pos, to_pos) {
            let from_x = from.x + from.width;
            let to_x = to.x + to.width;

            // Offset arc endpoints: outgoing slightly below center, incoming slightly above
            // This prevents arcs from overlapping at nodes with both incoming and outgoing connections
            let y_offset = 3.0;
            let from_y = from.y + from.height / 2.0 + y_offset; // outgoing: below center
            let to_y = to.y + to.height / 2.0 - y_offset; // incoming: above center

            // Calculate "hops" - how many rows the arc spans
            let row_height = 30.0; // Same as RenderConfig default
            let hops = ((to_y - from_y).abs() / row_height).round().max(1.0);

            // Control point X scales with number of hops
            // Base offset + additional offset per hop
            let arc_offset = 20.0 + (hops * 15.0);
            let ctrl_x = base_x + arc_offset;
            let mid_y = (from_y + to_y) / 2.0;

            // S-shaped Bezier with two Q commands
            let path = format!(
                "M {from_x},{from_y} Q {ctrl_x},{from_y} {ctrl_x},{mid_y} Q {ctrl_x},{to_y} {to_x},{to_y}"
            );

            let (arc_class, arrow_class, extra_style) = match edge.kind {
                EdgeKind::Normal => ("dep-arc", "dep-arrow", ""),
                EdgeKind::DirectCycle => ("cycle-arc", "cycle-arrow", ""),
                EdgeKind::TransitiveCycle => {
                    ("cycle-arc", "cycle-arrow", " stroke-dasharray=\"4,2\"")
                }
            };

            // Build data-source-locations attribute
            let locations_str = edge
                .source_locations
                .iter()
                .map(|loc| format!("{}:{}", loc.file.display(), loc.line))
                .collect::<Vec<_>>()
                .join(", ");
            let data_loc_attr = if !locations_str.is_empty() {
                format!(r#" data-source-locations="{}""#, escape_xml(&locations_str))
            } else {
                String::new()
            };

            let edge_id = format!("{}-{}", edge.from, edge.to);
            edges.push_str(&format!(
                "    <path class=\"{arc_class}\" id=\"edge-{edge_id}\" data-from=\"{}\" data-to=\"{}\" d=\"{path}\"{extra_style}{data_loc_attr}/>\n",
                edge.from, edge.to
            ));

            // Arrow head pointing to target
            let arrow = render_arrow(to_x, to_y, arrow_class, &edge_id);
            edges.push_str(&arrow);

            // For DirectCycle, add reverse arrow
            if edge.kind == EdgeKind::DirectCycle {
                let reverse_arrow = render_arrow(from_x, from_y, arrow_class, &edge_id);
                edges.push_str(&reverse_arrow);
            }
        }
    }

    edges.push_str("  </g>\n");
    edges
}

fn render_arrow(x: f32, y: f32, class: &str, edge_id: &str) -> String {
    // Arrow pointing left (toward the node at x)
    // Tip at x, base at x+8
    let p1 = format!("{},{}", x + 8.0, y - 4.0); // top-right
    let p2 = format!("{},{}", x, y); // tip (left, pointing at node)
    let p3 = format!("{},{}", x + 8.0, y + 4.0); // bottom-right
    format!("    <polygon class=\"{class}\" data-edge=\"{edge_id}\" points=\"{p1} {p2} {p3}\"/>\n")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_empty() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn test_render_single_crate() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "my_crate".into());
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(r#"class="crate""#));
        assert!(svg.contains("my_crate"));
    }

    #[test]
    fn test_render_with_edges() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.add_edge(a, b, EdgeKind::Normal, vec![]);
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(" Q ")); // Bezier
        assert!(svg.contains("<polygon")); // Arrow
    }

    #[test]
    fn test_render_cycle_edges() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );

        // Test DirectCycle
        ir.add_edge(a, b, EdgeKind::DirectCycle, vec![]);
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("cycle-arc"));
        // DirectCycle should have two arrows (bidirectional)
        // Count polygon elements with cycle-arrow class (not style definition)
        assert_eq!(svg.matches(r#"class="cycle-arrow""#).count(), 2);

        // Test TransitiveCycle
        let mut ir2 = LayoutIR::new();
        let c2 = ir2.add_item(ItemKind::Crate, "c".into());
        let a2 = ir2.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c2,
            },
            "a".into(),
        );
        let b2 = ir2.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c2,
            },
            "b".into(),
        );
        ir2.add_edge(a2, b2, EdgeKind::TransitiveCycle, vec![]);
        let svg2 = render(&ir2, &RenderConfig::default());
        assert!(svg2.contains("cycle-arc"));
        assert!(svg2.contains("stroke-dasharray"));
    }

    #[test]
    fn test_xml_escaping() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "foo<bar>&baz".into());
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("foo&lt;bar&gt;&amp;baz"));
        assert!(!svg.contains("foo<bar>&baz"));
    }

    #[test]
    fn test_tree_lines() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("tree-line"));
    }

    #[test]
    fn test_calculate_text_width() {
        // Monospace 12px → ~7.2px pro Zeichen
        let epsilon = 0.01;
        assert!((calculate_text_width("cli") - 21.6).abs() < epsilon); // 3 * 7.2
        assert!((calculate_text_width("authentication_handler") - 158.4).abs() < epsilon); // 22 * 7.2
    }

    #[test]
    fn test_box_width_from_longest_label() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "cli".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: 0,
            },
            "authentication_handler".into(),
        );

        let width = calculate_box_width(&ir);
        // längster Name (22 chars) * 7.2 + padding (20px)
        assert!(width >= 158.4 + 20.0);
    }

    #[test]
    fn test_canvas_width_includes_max_arc() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        // 10 Module für große Arc-Distanz
        for i in 0..10 {
            ir.add_item(
                ItemKind::Module {
                    nesting: 1,
                    parent: c,
                },
                format!("m{i}"),
            );
        }
        // Edge von erstem zu letztem Modul (9 Hops)
        ir.add_edge(1, 10, EdgeKind::Normal, vec![]);

        let svg = render(&ir, &RenderConfig::default());
        // Parse viewBox width
        let viewbox_width: f32 = svg
            .lines()
            .find(|l| l.contains("viewBox"))
            .and_then(|l| {
                let start = l.find("viewBox=\"0 0 ")? + 13;
                let rest = &l[start..];
                let end = rest.find(' ')?;
                rest[..end].parse().ok()
            })
            .unwrap();

        // max_arc for 9 hops = 20 + 9*15 + 8 = 163px
        // Canvas must include box_width + max_arc + margin
        let box_width = calculate_box_width(&ir);
        let expected_min = box_width + 163.0;
        assert!(
            viewbox_width >= expected_min,
            "viewBox width {} should be >= {} (box_width {} + arc 163)",
            viewbox_width,
            expected_min,
            box_width
        );
    }

    #[test]
    fn test_all_boxes_same_width() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "short".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: 0,
            },
            "very_long_module_name".into(),
        );

        let svg = render(&ir, &RenderConfig::default());
        // Extract all rect widths
        let widths: Vec<f32> = svg
            .lines()
            .filter(|l| l.contains("<rect"))
            .filter_map(|l| {
                let start = l.find("width=\"")? + 7;
                let rest = &l[start..];
                let end = rest.find('"')?;
                rest[..end].parse().ok()
            })
            .collect();

        assert_eq!(widths.len(), 2, "Expected 2 rects");
        assert_eq!(
            widths[0], widths[1],
            "All boxes should have same width: {:?}",
            widths
        );
    }

    #[test]
    fn test_nodes_have_ids() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "m".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(r#"id="node-0""#), "Crate should have id");
        assert!(svg.contains(r#"id="node-1""#), "Module should have id");
    }

    #[test]
    fn test_edges_have_data_attributes() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.add_edge(a, b, EdgeKind::Normal, vec![]);
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(r#"id="edge-1-2""#), "Edge should have id");
        assert!(
            svg.contains(r#"data-from="1""#),
            "Edge should have data-from"
        );
        assert!(svg.contains(r#"data-to="2""#), "Edge should have data-to");
    }

    #[test]
    fn test_svg_has_script() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("<script>"), "SVG should contain script tag");
        assert!(
            svg.contains("highlightNode"),
            "Script should contain highlightNode function"
        );
        assert!(
            svg.contains("highlightEdge"),
            "Script should contain highlightEdge function"
        );
    }

    #[test]
    fn test_render_has_parent_data_attribute() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent_crate".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child_module".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(r#"data-parent="0""#),
            "Module should have data-parent attribute pointing to crate"
        );
    }

    #[test]
    fn test_render_has_children_attribute() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent_crate".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child_module".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(r#"data-has-children="true""#),
            "Crate with children should have data-has-children attribute"
        );
    }

    #[test]
    fn test_render_tree_lines_have_data_attributes() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        // Tree lines should have data-parent and data-child attributes
        assert!(
            svg.contains(r#"class="tree-line""#) && svg.contains(r#"data-parent="0""#),
            "Tree lines should have data-parent attribute"
        );
        assert!(
            svg.contains(r#"data-child="1""#),
            "Tree lines should have data-child attribute"
        );
    }

    #[test]
    fn test_render_collapse_toggle_present() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(r#"class="collapse-toggle""#),
            "Parent nodes should have collapse toggle"
        );
        assert!(
            svg.contains(r#"data-target="0""#),
            "Collapse toggle should target parent node"
        );
    }

    #[test]
    fn test_render_collapse_css_classes() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(".collapse-toggle"),
            "CSS should contain .collapse-toggle style"
        );
        assert!(
            svg.contains(".collapsed"),
            "CSS should contain .collapsed style"
        );
        assert!(
            svg.contains(".virtual-arc"),
            "CSS should contain .virtual-arc style"
        );
        assert!(
            svg.contains(".arc-count"),
            "CSS should contain .arc-count style"
        );
        assert!(
            svg.contains(".child-count"),
            "CSS should contain .child-count style"
        );
    }

    #[test]
    fn test_render_child_count_tspan() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(r#"id="count-0""#),
            "Parent should have child-count tspan with id"
        );
        assert!(
            svg.contains(r#"class="child-count""#),
            "Tspan should have child-count class"
        );
    }

    #[test]
    fn test_render_script_has_collapse_functions() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains("toggleCollapse"),
            "Script should contain toggleCollapse function"
        );
        assert!(
            svg.contains("getDescendants"),
            "Script should contain getDescendants function"
        );
        assert!(
            svg.contains("relayout"),
            "Script should contain relayout function"
        );
        assert!(
            svg.contains("collapseState"),
            "Script should contain collapseState map"
        );
    }

    #[test]
    fn test_render_script_has_hover_functions() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains("pinnedHighlight"),
            "Script should contain pinnedHighlight state"
        );
        assert!(
            svg.contains("handleMouseEnter"),
            "Script should contain handleMouseEnter function"
        );
        assert!(
            svg.contains("handleMouseLeave"),
            "Script should contain handleMouseLeave function"
        );
        assert!(
            svg.contains("mouseenter"),
            "Script should register mouseenter events"
        );
        assert!(
            svg.contains("mouseleave"),
            "Script should register mouseleave events"
        );
    }

    #[test]
    fn test_render_script_has_toggle_deselect() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        // highlightNode toggle-check
        assert!(
            svg.contains("pinnedHighlight.id === nodeId"),
            "highlightNode should have toggle-check for same node"
        );
        // highlightEdge toggle-check
        assert!(
            svg.contains("pinnedHighlight.id === edgeId"),
            "highlightEdge should have toggle-check for same edge"
        );
    }

    #[test]
    fn test_render_edge_has_data_source_locations() {
        use crate::graph::SourceLocation;
        use std::path::PathBuf;

        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.add_edge(
            a,
            b,
            EdgeKind::Normal,
            vec![SourceLocation {
                file: PathBuf::from("src/a.rs"),
                line: 5,
            }],
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(r#"data-source-locations="#));
        assert!(svg.contains("src/a.rs:5"));
    }

    #[test]
    fn test_render_has_floating_label_css() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(".floating-label"));
    }

    #[test]
    fn test_render_script_has_floating_label_functions() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("showFloatingLabel"));
        assert!(svg.contains("hideFloatingLabel"));
        assert!(svg.contains("floatingLabel"));
    }

    #[test]
    fn test_render_script_arc_hover_shows_locations() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("dataset.sourceLocations"));
        assert!(svg.contains("showFloatingLabel"));
    }

    #[test]
    fn test_render_script_virtual_arc_aggregates_locations() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        // Virtual arcs should aggregate source locations from hidden edges
        assert!(
            svg.contains("aggregatedLocations") || svg.contains("hiddenEdgeData"),
            "Script should collect locations from hidden edges for virtual arcs"
        );
    }
}
