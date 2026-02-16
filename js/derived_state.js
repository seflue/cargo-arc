// @module DerivedState
// @deps TreeLogic, ArcLogic, AppState, HighlightLogic
// @config
// derived_state.js - Pure functions to derive display state from core state
// Computes highlights and visibility based on selection and collapse state
// No DOM dependencies - operates on Maps/Sets
// TreeLogic is loaded before this file (see render.rs load order)

const DerivedState = {
  /**
   * Derive which nodes are visible based on collapsed state.
   * A node is visible if getVisibleAncestor(nodeId) === nodeId
   * @param {Set<string>} collapsed - Set of collapsed node IDs
   * @param {Object} staticData - StaticData accessor
   * @returns {Set<string>} - Set of visible node IDs
   */
  deriveNodeVisibility(collapsed, staticData) {
    const parentMap = staticData.buildParentMap();
    const visibleNodes = new Set();

    for (const nodeId of staticData.getAllNodeIds()) {
      const visibleAncestor = TreeLogic.getVisibleAncestor(nodeId, collapsed, parentMap);
      if (visibleAncestor === nodeId) {
        visibleNodes.add(nodeId);
      }
    }

    return visibleNodes;
  },

  /**
   * Derive which arcs are visible (both endpoints visible).
   * @param {Set<string>} visibleNodes - Set of visible node IDs
   * @param {Object} staticData - StaticData accessor
   * @returns {Set<string>} - Set of visible arc IDs
   */
  deriveArcVisibility(visibleNodes, staticData) {
    const visibleArcs = new Set();

    for (const arcId of staticData.getAllArcIds()) {
      const arc = staticData.getArc(arcId);
      if (arc && visibleNodes.has(arc.from) && visibleNodes.has(arc.to)) {
        visibleArcs.add(arcId);
      }
    }

    return visibleArcs;
  },

  /**
   * Determine arc direction based on hierarchy.
   * @private
   * @param {string} fromId
   * @param {string} toId
   * @param {Map<string, string>} parentMap
   * @returns {string} 'upward' | 'downward'
   */
  _determineDirection(fromId, toId, parentMap) {
    // Simple heuristic: check if 'to' is an ancestor of 'from' (upward)
    // Otherwise downward
    let current = fromId;
    while (current) {
      if (current === toId) return 'upward';
      current = parentMap.get(current);
    }
    return 'downward';
  },

  /**
   * Derive the set of node IDs that should be highlighted when a node is pinned.
   * - Collapsed node → {nodeId} (children hidden, virtual arcs cover them)
   * - Expanded node → {nodeId, ...directly arc-connected visible descendants}
   * - Leaf node → {nodeId}
   * @param {string} nodeId - The pinned node ID
   * @param {Set<string>} collapsed - Set of collapsed node IDs
   * @param {Object} staticData - StaticData accessor
   * @returns {Set<string>} - Set of node IDs to highlight
   */
  deriveHighlightSet(nodeId, collapsed, staticData) {
    const result = new Set([nodeId]);

    // Collapsed or leaf → only the node itself
    if (collapsed.has(nodeId) || !staticData.hasChildren(nodeId)) {
      return result;
    }

    // Build outgoing-targets map once: sourceId → Set of targetIds
    const outgoing = new Map();
    for (const arcId of staticData.getAllArcIds()) {
      const arc = staticData.getArc(arcId);
      if (!arc) continue;
      if (!outgoing.has(arc.from)) outgoing.set(arc.from, new Set());
      outgoing.get(arc.from).add(arc.to);
    }

    // Transitive walk: expand through arc-connected, expanded descendants
    const parentMap = staticData.buildParentMap();
    const queue = [nodeId];
    while (queue.length > 0) {
      const current = queue.shift();
      const targets = outgoing.get(current);
      if (!targets) continue;

      const descendants = TreeLogic.getDescendants(current, parentMap);
      for (const descId of descendants) {
        if (result.has(descId)) continue;
        if (!targets.has(descId)) continue;
        const visibleAncestor = TreeLogic.getVisibleAncestor(descId, collapsed, parentMap);
        if (visibleAncestor !== descId) continue;

        result.add(descId);
        if (!collapsed.has(descId) && staticData.hasChildren(descId)) {
          queue.push(descId);
        }
      }
    }

    return result;
  },

  /**
   * Compute current positions for all visible nodes based on collapse state.
   * Replaces extractNodePositions() - no DOM reads needed.
   * @param {Set<string>} collapsed - Set of collapsed node IDs
   * @param {Object} staticData - StaticData accessor
   * @param {number} margin - Top/left margin
   * @param {number} toolbarHeight - Height of toolbar
   * @param {number} rowHeight - Height per row
   * @returns {Map<string, {x: number, y: number, width: number, height: number}>}
   */
  computeCurrentPositions(collapsed, staticData, margin, toolbarHeight, rowHeight, widthOverrides) {
    const visibleNodes = this.deriveNodeVisibility(collapsed, staticData);
    const positions = new Map();

    // Sort visible nodes by original Y position
    const sortedIds = [...visibleNodes].sort((a, b) => {
      const posA = staticData.getOriginalPosition(a);
      const posB = staticData.getOriginalPosition(b);
      return posA.y - posB.y;
    });

    // Compute new Y positions based on visible order
    let currentY = margin + toolbarHeight;
    for (const nodeId of sortedIds) {
      const orig = staticData.getOriginalPosition(nodeId);
      const width = (widthOverrides && widthOverrides.has(nodeId))
        ? widthOverrides.get(nodeId) : orig.width;
      positions.set(nodeId, {
        x: orig.x,
        y: currentY,
        width,
        height: orig.height
      });
      currentY += rowHeight;
    }

    return positions;
  },

  /**
   * @typedef {Object} HighlightState
   * @property {Map<string, {role: string, cssClass: string}>} nodeHighlights
   *   Highlighted nodes with their role and CSS class name.
   * @property {Map<string, {highlightWidth: number, arrowScale: number, relationType: string, isVirtual: boolean}>} arcHighlights
   *   Highlighted arcs. Keys: "from-to" for regular arcs, "v:from-to" for virtual arcs.
   * @property {Map<string, {shadowWidth: number, visibleLength: number, dashOffset: number, glowClass: string}>} shadowData
   *   Shadow glow data per arc. Keys: same convention as arcHighlights.
   * @property {Set<string>} promotedHitareas
   *   Arc IDs (without "v:" prefix) whose hitareas should be promoted to highlight layer.
   */

  /**
   * Derive complete highlight state from application state (pure, no DOM access).
   * @param {Object} appState - AppState object
   * @param {Object} staticData - StaticData accessor
   * @param {Map<string, Array>} virtualArcUsages - Runtime virtual arc usage map
   * @param {Set<string>} hiddenByFilter - Arc IDs hidden by user filters
   * @param {Map<string, {x: number, y: number, width: number, height: number}>} positions - Current node positions
   * @param {number} rowHeight - Row height for arc path calculation
   * @returns {HighlightState|null} null when no active selection
   */
  deriveHighlightState(appState, staticData, virtualArcUsages, hiddenByFilter, positions, rowHeight) {
    const selection = AppState.getActiveSelection(appState);
    if (selection.mode === 'none') return null;

    const result = {
      nodeHighlights: new Map(), arcHighlights: new Map(),
      shadowData: new Map(), promotedHitareas: new Set()
    };
    const ctx = { maxRight: this.computeMaxRight(positions), rowHeight, yOffset: 3 };

    if (selection.type === 'node') {
      this._deriveForNodeSelection(selection, appState, staticData, virtualArcUsages, hiddenByFilter, positions, ctx, result);
    } else if (selection.type === 'arc') {
      this._deriveForArcSelection(selection, staticData, virtualArcUsages, hiddenByFilter, positions, ctx, result);
    }

    return result;
  },

  /** @private Node-selection: highlight set + all connected arcs (regular + virtual). */
  _deriveForNodeSelection(selection, appState, staticData, virtualArcUsages, hiddenByFilter, positions, ctx, result) {
    const highlightSet = this.deriveHighlightSet(selection.id, appState.collapsed, staticData);

    const primaryNode = staticData.getNode(selection.id);
    if (primaryNode) {
      const cssClass = primaryNode.type === 'crate' ? 'selectedCrate' : 'selectedModule';
      result.nodeHighlights.set(selection.id, { role: 'current', cssClass });
    }

    // Group mode: mark children as group-member so they are not dimmed
    if (highlightSet.size > 1) {
      for (const nodeId of highlightSet) {
        if (nodeId !== selection.id) {
          result.nodeHighlights.set(nodeId, { role: 'group-member', cssClass: 'groupMember' });
        }
      }
    }

    const groupMode = highlightSet.size > 1;
    const descs = [
      ...this._collectRegularArcDescs(staticData, hiddenByFilter, highlightSet, groupMode, selection.id),
      ...this._collectVirtualArcDescs(virtualArcUsages, highlightSet, groupMode, selection.id)
    ];
    this._processArcDescriptors(descs, positions, ctx, result);

    // Direct-cycle detection: mark bidirectional cycle partners with cycle-member
    for (const arcId of staticData.getAllArcIds()) {
      const arc = staticData.getArc(arcId);
      if (!arc || !arc.cycleIds || arc.cycleIds.length === 0 || arc.from !== selection.id) continue;
      const partner = arc.to;
      const reverseArc = staticData.getArc(`${partner}-${selection.id}`);
      if (reverseArc && reverseArc.cycleIds && reverseArc.cycleIds.length > 0) {
        const existing = result.nodeHighlights.get(partner);
        if (!existing || existing.role !== 'current') {
          result.nodeHighlights.set(partner, { role: 'cycle-member', cssClass: 'cycleMember' });
        }
      }
    }
  },

  /** @private Arc-selection: specific arc + its virtual variant + cycle expansion. */
  _deriveForArcSelection(selection, staticData, virtualArcUsages, hiddenByFilter, positions, ctx, result) {
    const arcId = selection.id;
    const [fromId, toId] = arcId.split('-');

    result.nodeHighlights.set(fromId, { role: 'dependent', cssClass: 'dependentNode' });
    result.nodeHighlights.set(toId, { role: 'dependency', cssClass: 'depNode' });

    const descs = [];
    const arc = staticData.getArc(arcId);
    if (arc && !hiddenByFilter.has(arcId)) {
      descs.push({
        key: arcId, fromId, toId, fromInSet: true, toInSet: true,
        originalWidth: staticData.getArcStrokeWidth(arcId), isVirtual: false
      });
    }
    if (virtualArcUsages.has(arcId)) {
      const usages = virtualArcUsages.get(arcId);
      const count = usages.reduce((sum, g) => sum + g.locations.length, 0);
      descs.push({
        key: 'v:' + arcId, fromId, toId, fromInSet: true, toInSet: true,
        originalWidth: ArcLogic.calculateStrokeWidth(count), isVirtual: true
      });
    }

    // Cycle expansion: highlight all arcs and nodes in all associated cycles
    if (arc && arc.cycleIds && arc.cycleIds.length > 0 && typeof STATIC_DATA !== 'undefined' && STATIC_DATA.cycles) {
      for (const cid of arc.cycleIds) {
        const cycle = STATIC_DATA.cycles[cid];
        if (!cycle) continue;
        for (const nodeId of cycle.nodes) {
          if (!result.nodeHighlights.has(nodeId)) {
            result.nodeHighlights.set(nodeId, { role: 'cycle-member', cssClass: 'cycleMember' });
          }
        }
        for (const cycleArcId of cycle.arcs) {
          if (cycleArcId === arcId) continue;
          if (result.arcHighlights.has(cycleArcId)) continue;
          const cycleArc = staticData.getArc(cycleArcId);
          if (cycleArc && !hiddenByFilter.has(cycleArcId)) {
            descs.push({
              key: cycleArcId, fromId: cycleArc.from, toId: cycleArc.to,
              fromInSet: true, toInSet: true,
              originalWidth: staticData.getArcStrokeWidth(cycleArcId), isVirtual: false
            });
          }
        }
      }
    }

    this._processArcDescriptors(descs, positions, ctx, result);
  },

  /** @private Build descriptors for regular arcs connected to highlight set.
   *  @param {boolean} groupMode - When true, only keep arcs where selectionId is an endpoint.
   *    All other arcs (child-to-child, child-to-external) are suppressed so only the
   *    parent's own dependency arcs are shown.
   *  @param {string} selectionId - The primarily selected node (parent).
   */
  _collectRegularArcDescs(staticData, hiddenByFilter, highlightSet, groupMode, selectionId) {
    const descs = [];
    for (const arcId of staticData.getAllArcIds()) {
      if (hiddenByFilter.has(arcId)) continue;
      const arc = staticData.getArc(arcId);
      if (!arc) continue;
      const fromInSet = highlightSet.has(arc.from);
      const toInSet = highlightSet.has(arc.to);
      if (!fromInSet && !toInSet) continue;
      if (groupMode && arc.from !== selectionId && arc.to !== selectionId) continue;
      descs.push({
        key: arcId, fromId: arc.from, toId: arc.to, fromInSet, toInSet,
        originalWidth: staticData.getArcStrokeWidth(arcId), isVirtual: false,
        isNonProduction: arc.context?.kind !== "production"
      });
    }
    return descs;
  },

  /** @private Build descriptors for virtual arcs connected to highlight set.
   *  @param {boolean} groupMode - When true, only keep arcs where selectionId is an endpoint.
   *  @param {string} selectionId - The primarily selected node (parent).
   */
  _collectVirtualArcDescs(virtualArcUsages, highlightSet, groupMode, selectionId) {
    const descs = [];
    for (const [vArcId, usages] of virtualArcUsages) {
      const [fromId, toId] = vArcId.split('-');
      const fromInSet = highlightSet.has(fromId);
      const toInSet = highlightSet.has(toId);
      if (!fromInSet && !toInSet) continue;
      if (groupMode && fromId !== selectionId && toId !== selectionId) continue;
      const count = usages.reduce((sum, g) => sum + g.locations.length, 0);
      descs.push({
        key: 'v:' + vArcId, fromId, toId, fromInSet, toInSet,
        originalWidth: ArcLogic.calculateStrokeWidth(count), isVirtual: true
      });
    }
    return descs;
  },

  /** @private Process arc descriptors: compute highlights, shadows, endpoint nodes. */
  _processArcDescriptors(descriptors, positions, ctx, result) {
    for (const desc of descriptors) {
      const fromPos = positions.get(desc.fromId);
      const toPos = positions.get(desc.toId);
      // Virtual arcs: no position guard needed — highlight width/arrowScale are
      // position-independent, and _computeShadowEntry safely returns null when
      // positions are missing.
      if (!desc.isVirtual && (!fromPos || !toPos)) continue;

      const relationType = desc.fromInSet && !desc.toInSet ? 'dep'
        : !desc.fromInSet && desc.toInSet ? 'reverse' : 'dep';

      const { arcHighlight, shadow } = this._computeArcHighlightEntry(
        desc.originalWidth, desc.isVirtual, relationType, fromPos, toPos, ctx
      );

      result.arcHighlights.set(desc.key, arcHighlight);
      if (shadow) result.shadowData.set(desc.key, shadow);
      if (!desc.isVirtual) result.promotedHitareas.add(desc.key);
      if (!desc.isNonProduction) {
        this._markEndpointNodes(desc.fromId, desc.toId, desc.fromInSet, desc.toInSet, result.nodeHighlights);
      }
    }
  },

  /** @private Compute highlight entry + shadow for a single arc. */
  _computeArcHighlightEntry(originalWidth, isVirtual, relationType, fromPos, toPos, ctx) {
    const highlightWidth = HighlightLogic.calculateHighlightWidth(originalWidth);
    const arrowScale = isVirtual
      ? HighlightLogic.calculateVirtualArrowScale(highlightWidth)
      : ArcLogic.scaleFromStrokeWidth(highlightWidth);
    return {
      arcHighlight: { highlightWidth, arrowScale, relationType, isVirtual },
      shadow: this._computeShadowEntry(originalWidth, relationType, fromPos, toPos, ctx)
    };
  },

  /** @private Compute shadow glow data for an arc path. */
  _computeShadowEntry(originalWidth, relationType, fromPos, toPos, ctx) {
    if (!fromPos || !toPos) return null;
    const pathData = this._computeArcPath(fromPos, toPos, ctx.yOffset, ctx.maxRight, ctx.rowHeight);
    const pathLength = ArcLogic.estimatePathLength(pathData.path);
    const sd = HighlightLogic.calculateShadowData(originalWidth, pathLength);
    return {
      shadowWidth: sd.shadowWidth, visibleLength: sd.visibleLength,
      dashOffset: sd.dashOffset,
      glowClass: relationType === 'dep' ? 'glowIncoming' : 'glowOutgoing'
    };
  },

  /** @private Set CSS classes for arc endpoint nodes.
   *  depNode/dependentNode may override group-member (to give green/purple border).
   */
  _markEndpointNodes(fromId, toId, fromInSet, toInSet, nodeHighlights) {
    const fromRole = nodeHighlights.get(fromId)?.role;
    if (!fromRole || fromRole === 'group-member') {
      nodeHighlights.set(fromId, { role: 'dependent', cssClass: 'dependentNode' });
    }
    const toRole = nodeHighlights.get(toId)?.role;
    if (!toRole || toRole === 'group-member') {
      nodeHighlights.set(toId, { role: 'dependency', cssClass: 'depNode' });
    }
  },

  /**
   * Compute arc path between two node positions.
   * @private
   * @param {{x: number, y: number, width: number, height: number}} fromPos
   * @param {{x: number, y: number, width: number, height: number}} toPos
   * @param {number} yOffset - Y offset for arc endpoints
   * @param {number} maxRight - Rightmost X coordinate
   * @param {number} rowHeight - Row height for arc offset calculation
   * @returns {{path: string, toX: number, toY: number, ctrlX: number, midY: number}}
   */
  _computeArcPath(fromPos, toPos, yOffset, maxRight, rowHeight) {
    return ArcLogic.calculateArcPathFromPositions(fromPos, toPos, yOffset, maxRight, rowHeight);
  },

  /**
   * Compute the rightmost X coordinate from positions.
   * @param {Map<string, {x: number, width: number}>} positions
   * @returns {number}
   */
  computeMaxRight(positions) {
    let maxRight = 0;
    for (const pos of positions.values()) {
      maxRight = Math.max(maxRight, pos.x + pos.width);
    }
    return maxRight;
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { DerivedState };
}
