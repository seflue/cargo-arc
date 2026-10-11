import { AppState } from './app_state.js';
import { ArcLogic } from './arc_logic.js';
import { HighlightLogic } from './highlight_logic.js';
import type { StaticData } from './static_data.js';
import { TreeLogic } from './tree_logic.js';

// derived_state.ts - Pure functions to derive display state from core state
// Computes highlights and visibility based on selection and collapse state
// No DOM dependencies - operates on Maps/Sets

export interface HighlightState {
  /** Highlighted nodes with their role and CSS class name. */
  nodeHighlights: Map<string, { role: string; cssClass: string }>;
  /** Highlighted arcs. Keys: "from-to" for regular arcs, "v:from-to" for virtual arcs. */
  arcHighlights: Map<
    string,
    {
      highlightWidth: number;
      arrowScale: number;
      relationType: string;
      isVirtual: boolean;
    }
  >;
  /** Shadow glow data per arc. Keys: same convention as arcHighlights. */
  shadowData: Map<
    string,
    {
      shadowWidth: number;
      visibleLength: number;
      dashOffset: number;
      glowClass: string;
    }
  >;
  /** Arc IDs (without "v:" prefix) whose hitareas should be promoted to highlight layer. */
  promotedHitareas: Set<string>;
  /** Whether the highlight is from a pinned (click) selection. */
  isPinned: boolean;
}

type Position = { x: number; y: number; width: number; height: number };
type ArcPathContext = { maxRight: number; rowHeight: number; yOffset: number };
type ArcDescriptor = {
  key: string;
  fromId: string;
  toId: string;
  fromInSet: boolean;
  toInSet: boolean;
  originalWidth: number;
  isVirtual: boolean;
  isNonProduction?: boolean;
  isCycle?: boolean;
};

const DerivedState = {
  /**
   * Derive which nodes are visible based on collapsed and filter state.
   * A node is visible if getVisibleAncestor(nodeId) === nodeId and it is not
   * hidden by a filter.
   * @param collapsed - Set of collapsed node IDs
   * @param staticData - StaticData accessor
   * @param [hiddenByFilter] - Node IDs hidden by active filters
   * @returns - Set of visible node IDs
   */
  deriveNodeVisibility(
    collapsed: Set<string>,
    staticData: typeof StaticData,
    hiddenByFilter?: Set<string>,
  ): Set<string> {
    const parentMap = staticData.buildParentMap();
    const visibleNodes = new Set<string>();

    for (const nodeId of staticData.getAllNodeIds()) {
      if (hiddenByFilter?.has(nodeId)) continue;
      const visibleAncestor = TreeLogic.getVisibleAncestor(
        nodeId,
        collapsed,
        parentMap,
      );
      if (visibleAncestor === nodeId) {
        visibleNodes.add(nodeId);
      }
    }

    return visibleNodes;
  },

  /**
   * Derive which arcs are visible (both endpoints visible).
   * @param visibleNodes - Set of visible node IDs
   * @param staticData - StaticData accessor
   * @returns - Set of visible arc IDs
   */
  deriveArcVisibility(
    visibleNodes: Set<string>,
    staticData: typeof StaticData,
  ): Set<string> {
    const visibleArcs = new Set<string>();

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
   * @returns 'upward' | 'downward'
   */
  _determineDirection(
    fromId: string,
    toId: string,
    parentMap: Map<string, string>,
  ): string {
    // Simple heuristic: check if 'to' is an ancestor of 'from' (upward)
    // Otherwise downward
    let current: string | undefined = fromId;
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
   * @param nodeId - The pinned node ID
   * @param collapsed - Set of collapsed node IDs
   * @param staticData - StaticData accessor
   * @returns - Set of node IDs to highlight
   */
  deriveHighlightSet(
    nodeId: string,
    collapsed: Set<string>,
    staticData: typeof StaticData,
  ): Set<string> {
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
      if (current === undefined) continue;
      const targets = outgoing.get(current);
      if (!targets) continue;

      const descendants = TreeLogic.getDescendants(current, parentMap);
      for (const descId of descendants) {
        if (result.has(descId)) continue;
        if (!targets.has(descId)) continue;
        const visibleAncestor = TreeLogic.getVisibleAncestor(
          descId,
          collapsed,
          parentMap,
        );
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
   * @param collapsed - Set of collapsed node IDs
   * @param staticData - StaticData accessor
   * @param margin - Top/left margin
   * @param toolbarHeight - Height of toolbar
   * @param rowHeight - Height per row
   */
  computeCurrentPositions(
    collapsed: Set<string>,
    staticData: typeof StaticData,
    margin: number,
    toolbarHeight: number,
    rowHeight: number,
    widthOverrides?: Map<string, number>,
    hiddenByFilter?: Set<string>,
  ): Map<string, Position> {
    const visibleNodes = this.deriveNodeVisibility(
      collapsed,
      staticData,
      hiddenByFilter,
    );
    const positions = new Map<string, Position>();

    // Sort visible nodes by original Y position
    const sorted = [...visibleNodes]
      .flatMap((nodeId) => {
        const orig = staticData.getOriginalPosition(nodeId);
        return orig ? [{ nodeId, orig }] : [];
      })
      .sort((a, b) => a.orig.y - b.orig.y);

    // Compute new Y positions based on visible order
    let currentY = margin + toolbarHeight;
    for (const { nodeId, orig } of sorted) {
      const width = widthOverrides?.get(nodeId) ?? orig.width;
      positions.set(nodeId, {
        x: orig.x,
        y: currentY,
        width,
        height: orig.height,
      });
      currentY += rowHeight;
    }

    return positions;
  },

  /**
   * Derive complete highlight state from application state (pure, no DOM access).
   * @param appState - AppState object
   * @param staticData - StaticData accessor
   * @param virtualArcUsages - Runtime virtual arc usage map
   * @param hiddenByFilter - Arc IDs hidden by user filters
   * @param positions - Current node positions
   * @param rowHeight - Row height for arc path calculation
   * @returns null when no active selection
   */
  deriveHighlightState(
    appState: AppState,
    staticData: typeof StaticData,
    virtualArcUsages: Map<string, ArcData['usages']>,
    hiddenByFilter: Set<string>,
    positions: Map<string, Position>,
    rowHeight: number,
  ): HighlightState | null {
    const selection = AppState.getSelection(appState);
    const selectedScc = AppState.getSelectedScc(appState);
    const clusterActive =
      AppState.isClusterMode(appState) && selectedScc != null;
    if (selection.mode === 'none' && !clusterActive) return null;

    const result: HighlightState = {
      nodeHighlights: new Map(),
      arcHighlights: new Map(),
      shadowData: new Map(),
      promotedHitareas: new Set<string>(),
      // A selected SCC persists like a pin (see AppState.clickEdge), so the
      // has-pinned affordances apply before any inner edge is pinned.
      isPinned: selection.mode === 'click' || clusterActive,
    };
    const ctx: ArcPathContext = {
      maxRight: this.computeMaxRight(positions),
      rowHeight,
      yOffset: 3,
    };

    const { type, id } = selection;
    if (type === 'node' && id !== null) {
      this._deriveForNodeSelection(
        id,
        appState,
        staticData,
        virtualArcUsages,
        hiddenByFilter,
        positions,
        ctx,
        result,
      );
    } else if (type === 'arc' && id !== null) {
      this._deriveForArcSelection(
        id,
        selectedScc,
        clusterActive,
        staticData,
        virtualArcUsages,
        hiddenByFilter,
        positions,
        ctx,
        result,
      );
    } else if (clusterActive) {
      this._deriveForClusterWide(
        selectedScc,
        staticData,
        hiddenByFilter,
        positions,
        ctx,
        result,
      );
    }

    return result;
  },

  /** @private Node-selection: highlight set + all connected arcs (regular + virtual). */
  _deriveForNodeSelection(
    selectedId: string,
    appState: AppState,
    staticData: typeof StaticData,
    virtualArcUsages: Map<string, ArcData['usages']>,
    hiddenByFilter: Set<string>,
    positions: Map<string, Position>,
    ctx: ArcPathContext,
    result: HighlightState,
  ) {
    const highlightSet = this.deriveHighlightSet(
      selectedId,
      appState.collapsed,
      staticData,
    );

    const primaryNode = staticData.getNode(selectedId);
    if (primaryNode) {
      const cssClass =
        primaryNode.type === 'package'
          ? 'selectedPackage'
          : primaryNode.type === 'external-transitive'
            ? 'selectedExternalTransitive'
            : primaryNode.type === 'external-section' ||
                primaryNode.type === 'external'
              ? 'selectedExternal'
              : 'selectedModule';
      result.nodeHighlights.set(selectedId, { role: 'current', cssClass });
    }

    // Group mode: mark children as group-member so they are not dimmed
    if (highlightSet.size > 1) {
      for (const nodeId of highlightSet) {
        if (nodeId !== selectedId) {
          result.nodeHighlights.set(nodeId, {
            role: 'group-member',
            cssClass: 'groupMember',
          });
        }
      }
    }

    const groupMode = highlightSet.size > 1;
    const descs = [
      ...this._collectRegularArcDescs(
        staticData,
        hiddenByFilter,
        highlightSet,
        groupMode,
        selectedId,
      ),
      ...this._collectVirtualArcDescs(
        virtualArcUsages,
        highlightSet,
        groupMode,
        selectedId,
      ),
    ];
    this._processArcDescriptors(descs, positions, ctx, result);

    // Direct-cycle detection: mark bidirectional cycle partners with cycle-member
    for (const arcId of staticData.getAllArcIds()) {
      const arc = staticData.getArc(arcId);
      if (
        !arc ||
        !arc.cycleIds ||
        arc.cycleIds.length === 0 ||
        arc.from !== selectedId
      )
        continue;
      const partner = arc.to;
      const reverseArc = staticData.getArc(`${partner}-${selectedId}`);
      if (reverseArc?.cycleIds && reverseArc.cycleIds.length > 0) {
        const existing = result.nodeHighlights.get(partner);
        if (!existing || existing.role !== 'current') {
          result.nodeHighlights.set(partner, {
            role: 'cycle-member',
            cssClass: 'cycleMember',
          });
        }
      }
    }
  },

  /** @private Arc-selection: specific arc + its virtual variant. Two-level
   *  cycle-mode interaction: when an SCC is selected (selectedScc) and the
   *  resolved edge (click-pin or hover) belongs to it, only that edge is the
   *  focus and gets highlighted — the rest of the SCC dims. An edge outside
   *  the selected SCC (or no SCC selected at all) is ignored as a focus and
   *  falls back to the whole-SCC highlight / plain single-edge pick. */
  _deriveForArcSelection(
    arcId: string,
    selectedScc: number | null,
    clusterActive: boolean,
    staticData: typeof StaticData,
    virtualArcUsages: Map<string, ArcData['usages']>,
    hiddenByFilter: Set<string>,
    positions: Map<string, Position>,
    ctx: ArcPathContext,
    result: HighlightState,
  ) {
    const [fromId, toId] = arcId.split('-');

    const arc = staticData.getArc(arcId);
    const isFocusArc = clusterActive && arc?.sccId === selectedScc;

    if (clusterActive && !isFocusArc) {
      this._deriveForClusterWide(
        selectedScc,
        staticData,
        hiddenByFilter,
        positions,
        ctx,
        result,
      );
      return;
    }

    result.nodeHighlights.set(fromId, {
      role: 'dependent',
      cssClass: 'dependentNode',
    });
    result.nodeHighlights.set(toId, {
      role: 'dependency',
      cssClass: 'depNode',
    });

    const descs: ArcDescriptor[] = [];
    if (arc && !hiddenByFilter.has(arcId)) {
      descs.push({
        key: arcId,
        fromId,
        toId,
        fromInSet: true,
        toInSet: true,
        originalWidth: staticData.getArcStrokeWidth(arcId),
        isVirtual: false,
        isCycle: isFocusArc,
      });
    }
    const usages = virtualArcUsages.get(arcId);
    if (usages) {
      const count = usages.reduce((sum, g) => sum + g.locations.length, 0);
      descs.push({
        key: `v:${arcId}`,
        fromId,
        toId,
        fromInSet: true,
        toInSet: true,
        originalWidth: ArcLogic.calculateStrokeWidth(count),
        isVirtual: true,
      });
    }

    this._processArcDescriptors(descs, positions, ctx, result);
  },

  /** @private Whole-SCC highlight: every member node/edge of `selectedScc` is
   *  marked so the cluster reads as one unit. Used when no inner edge is
   *  focused, and as the fallback when the focused edge lies outside the
   *  selected SCC. */
  _deriveForClusterWide(
    selectedScc: number | null,
    staticData: typeof StaticData,
    hiddenByFilter: Set<string>,
    positions: Map<string, Position>,
    ctx: ArcPathContext,
    result: HighlightState,
  ) {
    for (const nodeId of staticData.getAllNodeIds()) {
      if (staticData.getNode(nodeId)?.sccId === selectedScc) {
        result.nodeHighlights.set(nodeId, {
          role: 'cycle-member',
          cssClass: 'cycleMember',
        });
      }
    }

    const descs: ArcDescriptor[] = [];
    for (const arcId of staticData.getAllArcIds()) {
      const arc = staticData.getArc(arcId);
      if (arc?.sccId === selectedScc && !hiddenByFilter.has(arcId)) {
        descs.push({
          key: arcId,
          fromId: arc.from,
          toId: arc.to,
          fromInSet: true,
          toInSet: true,
          originalWidth: staticData.getArcStrokeWidth(arcId),
          isVirtual: false,
          isCycle: true,
        });
      }
    }

    this._processArcDescriptors(descs, positions, ctx, result);
  },

  /** @private Build descriptors for regular arcs connected to highlight set.
   *  @param groupMode - When true, only keep arcs where selectionId is an endpoint.
   *    All other arcs (child-to-child, child-to-external) are suppressed so only the
   *    parent's own dependency arcs are shown.
   *  @param selectionId - The primarily selected node (parent).
   */
  _collectRegularArcDescs(
    staticData: typeof StaticData,
    hiddenByFilter: Set<string>,
    highlightSet: Set<string>,
    groupMode: boolean,
    selectionId: string,
  ): ArcDescriptor[] {
    const descs: ArcDescriptor[] = [];
    for (const arcId of staticData.getAllArcIds()) {
      if (hiddenByFilter.has(arcId)) continue;
      const arc = staticData.getArc(arcId);
      if (!arc) continue;
      const fromInSet = highlightSet.has(arc.from);
      const toInSet = highlightSet.has(arc.to);
      if (!fromInSet && !toInSet) continue;
      if (groupMode && arc.from !== selectionId && arc.to !== selectionId)
        continue;
      descs.push({
        key: arcId,
        fromId: arc.from,
        toId: arc.to,
        fromInSet,
        toInSet,
        originalWidth: staticData.getArcStrokeWidth(arcId),
        isVirtual: false,
        isNonProduction: arc.context?.kind !== 'production',
        isCycle: !!arc.cycleIds?.length,
      });
    }
    return descs;
  },

  /** @private Build descriptors for virtual arcs connected to highlight set.
   *  @param groupMode - When true, only keep arcs where selectionId is an endpoint.
   *  @param selectionId - The primarily selected node (parent).
   */
  _collectVirtualArcDescs(
    virtualArcUsages: Map<string, ArcData['usages']>,
    highlightSet: Set<string>,
    groupMode: boolean,
    selectionId: string,
  ): ArcDescriptor[] {
    const descs: ArcDescriptor[] = [];
    for (const [vArcId, usages] of virtualArcUsages) {
      const [fromId, toId] = vArcId.split('-');
      const fromInSet = highlightSet.has(fromId);
      const toInSet = highlightSet.has(toId);
      if (!fromInSet && !toInSet) continue;
      if (groupMode && fromId !== selectionId && toId !== selectionId) continue;
      const count = usages.reduce((sum, g) => sum + g.locations.length, 0);
      descs.push({
        key: `v:${vArcId}`,
        fromId,
        toId,
        fromInSet,
        toInSet,
        originalWidth: ArcLogic.calculateStrokeWidth(count),
        isVirtual: true,
      });
    }
    return descs;
  },

  /** @private Process arc descriptors: compute highlights, shadows, endpoint nodes.
   *  Production arcs are sorted first so their node roles (dependent/dependency)
   *  are established before non-production arcs — _markEndpointNodes only sets
   *  roles that are unset or group-member, so production wins on conflict.
   */
  _processArcDescriptors(
    descriptors: ArcDescriptor[],
    positions: Map<string, Position>,
    ctx: ArcPathContext,
    result: HighlightState,
  ) {
    descriptors.sort(
      (a, b) => (a.isNonProduction ? 1 : 0) - (b.isNonProduction ? 1 : 0),
    );
    for (const desc of descriptors) {
      const fromPos = positions.get(desc.fromId);
      const toPos = positions.get(desc.toId);
      // Virtual arcs: no position guard needed — highlight width/arrowScale are
      // position-independent, and _computeShadowEntry safely returns null when
      // positions are missing.
      if (!desc.isVirtual && (!fromPos || !toPos)) continue;

      const relationType = !desc.fromInSet && desc.toInSet ? 'reverse' : 'dep';

      const { arcHighlight, shadow } = this._computeArcHighlightEntry(
        desc.originalWidth,
        desc.isVirtual,
        relationType,
        fromPos,
        toPos,
        ctx,
        !!desc.isCycle,
      );

      result.arcHighlights.set(desc.key, arcHighlight);
      if (shadow) result.shadowData.set(desc.key, shadow);
      if (!desc.isVirtual) result.promotedHitareas.add(desc.key);
      this._markEndpointNodes(
        desc.fromId,
        desc.toId,
        desc.fromInSet,
        desc.toInSet,
        result.nodeHighlights,
      );
    }
  },

  /** @private Compute highlight entry + shadow for a single arc. */
  _computeArcHighlightEntry(
    originalWidth: number,
    isVirtual: boolean,
    relationType: string,
    fromPos: Position | undefined,
    toPos: Position | undefined,
    ctx: ArcPathContext,
    isCycle: boolean,
  ) {
    const highlightWidth =
      HighlightLogic.calculateHighlightWidth(originalWidth);
    const arrowScale = ArcLogic.scaleFromStrokeWidth(highlightWidth);
    return {
      arcHighlight: { highlightWidth, arrowScale, relationType, isVirtual },
      shadow: this._computeShadowEntry(
        originalWidth,
        relationType,
        fromPos,
        toPos,
        ctx,
        isCycle,
      ),
    };
  },

  /** @private Compute shadow glow data for an arc path. */
  _computeShadowEntry(
    originalWidth: number,
    relationType: string,
    fromPos: Position | undefined,
    toPos: Position | undefined,
    ctx: ArcPathContext,
    isCycle: boolean,
  ) {
    if (!fromPos || !toPos) return null;
    const pathData = ArcLogic.calculateArcPathFromPositions(
      fromPos,
      toPos,
      ctx.yOffset,
      ctx.maxRight,
      ctx.rowHeight,
    );
    const pathLength = ArcLogic.estimatePathLength(pathData.path);
    const sd = HighlightLogic.calculateShadowData(originalWidth, pathLength);
    const glowClass = isCycle
      ? 'glowCycle'
      : relationType === 'dep'
        ? 'glowIncoming'
        : 'glowOutgoing';
    return {
      shadowWidth: sd.shadowWidth,
      visibleLength: sd.visibleLength,
      dashOffset: sd.dashOffset,
      glowClass,
    };
  },

  /** @private Set CSS classes for arc endpoint nodes.
   *  depNode/dependentNode may override group-member (to give green/purple border).
   */
  _markEndpointNodes(
    fromId: string,
    toId: string,
    _fromInSet: boolean,
    _toInSet: boolean,
    nodeHighlights: HighlightState['nodeHighlights'],
  ) {
    const fromRole = nodeHighlights.get(fromId)?.role;
    if (!fromRole || fromRole === 'group-member') {
      nodeHighlights.set(fromId, {
        role: 'dependent',
        cssClass: 'dependentNode',
      });
    }
    const toRole = nodeHighlights.get(toId)?.role;
    if (!toRole || toRole === 'group-member') {
      nodeHighlights.set(toId, { role: 'dependency', cssClass: 'depNode' });
    }
  },

  /**
   * Compute the rightmost X coordinate from positions.
   */
  computeMaxRight(
    positions: Map<string, { x: number; width: number }>,
  ): number {
    let maxRight = 0;
    for (const pos of positions.values()) {
      maxRight = Math.max(maxRight, pos.x + pos.width);
    }
    return maxRight;
  },
};

export { DerivedState };
