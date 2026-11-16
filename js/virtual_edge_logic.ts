import type { ArcLogic, RowExtent } from './arc_logic.js';

// virtual_edge_logic.ts - Pure logic for virtual edge aggregation
// No DOM dependencies - uses Maps as data structures

type Direction = 'upward' | 'downward';

type VirtualEdge = {
  count: number;
  hiddenEdgeData: ArcData['usages'][];
  direction: Direction;
  originalArcs: string[];
  fromId: string;
  toId: string;
};

const VirtualEdgeLogic = {
  /**
   * The id of the virtual arc that stands for the hidden edges of one
   * direction between two visible nodes. The upward edges get an arc of their
   * own, drawn on the left.
   */
  virtualArcId(fromId: string, toId: string, direction: Direction): string {
    return direction === 'upward'
      ? `${fromId}-${toId}-up`
      : `${fromId}-${toId}`;
  },

  /** Whether `arcId` is the id of a virtual arc for upward edges. */
  isUpwardArcId(arcId: string): boolean {
    return arcId.endsWith('-up');
  },

  /**
   * Aggregate hidden edges by their visible endpoints and their direction.
   * @param getVisibleAncestorFn - Function to get visible ancestor for a node
   */
  aggregateHiddenEdges(
    edges: Array<{
      arcId: string;
      fromId: string;
      toId: string;
      fromHidden: boolean;
      toHidden: boolean;
      sourceLocations?: ArcData['usages'];
      direction?: string;
    }>,
    getVisibleAncestorFn: (arg0: string) => string,
  ): Map<string, VirtualEdge> {
    const virtualEdges = new Map<string, VirtualEdge>();

    for (const edge of edges) {
      const { arcId, fromId, toId, fromHidden, toHidden, sourceLocations } =
        edge;

      // Skip if both endpoints are visible
      if (!fromHidden && !toHidden) continue;

      // Calculate visible endpoints
      const visibleFrom = fromHidden ? getVisibleAncestorFn(fromId) : fromId;
      const visibleTo = toHidden ? getVisibleAncestorFn(toId) : toId;

      // Skip self-loops (both map to same visible ancestor)
      if (!visibleFrom || !visibleTo || visibleFrom === visibleTo) continue;

      const direction: Direction =
        edge.direction === 'upward' ? 'upward' : 'downward';
      const key = this.virtualArcId(visibleFrom, visibleTo, direction);
      let existing = virtualEdges.get(key);
      if (!existing) {
        existing = {
          count: 0,
          hiddenEdgeData: [],
          direction,
          originalArcs: [],
          fromId: visibleFrom,
          toId: visibleTo,
        };
        virtualEdges.set(key, existing);
      }

      existing.count++;
      if (sourceLocations) existing.hiddenEdgeData.push(sourceLocations);
      existing.originalArcs.push(arcId);
    }

    return virtualEdges;
  },

  /**
   * Prepare virtual edge data for rendering.
   * @param extent - Outer edges of the visible rows
   * @param arcLogic - ArcLogic module with calculateArcPath, countLocations, calculateStrokeWidth
   * @param rowHeight - Row height for arc calculation
   */
  prepareVirtualEdgeData(
    virtualEdges: Map<string, VirtualEdge>,
    nodePositions: Map<
      string,
      { x: number; y: number; width: number; height: number }
    >,
    extent: RowExtent,
    arcLogic: typeof ArcLogic,
    rowHeight: number,
  ): Map<
    string,
    {
      fromId: string;
      toId: string;
      count: number;
      hiddenEdgeData: ArcData['usages'][];
      arc: ReturnType<typeof ArcLogic.calculateArcPathFromPositions>;
      strokeWidth: number;
      direction: Direction;
      originalArcs: string[];
    }
  > {
    const mergedEdges = new Map();

    virtualEdges.forEach((data, key) => {
      const fromId = data.fromId ?? key.split('-')[0];
      const toId = data.toId ?? key.split('-')[1];
      const fromPos = nodePositions.get(fromId);
      const toPos = nodePositions.get(toId);

      if (!fromPos || !toPos) return;

      // Calculate arc path from position objects
      const arc = arcLogic.calculateArcPathFromPositions(
        fromPos,
        toPos,
        3,
        extent,
        rowHeight,
        data.direction === 'upward',
      );

      // Calculate stroke width from aggregated locations
      const totalLocations = data.hiddenEdgeData.reduce(
        (sum, locs) => sum + arcLogic.countLocations(locs),
        0,
      );
      const strokeWidth = arcLogic.calculateStrokeWidth(totalLocations);

      mergedEdges.set(key, {
        fromId,
        toId,
        count: data.count,
        hiddenEdgeData: data.hiddenEdgeData,
        arc,
        strokeWidth,
        direction: data.direction,
        originalArcs: data.originalArcs,
      });
    });

    return mergedEdges;
  },
};

export { VirtualEdgeLogic };
