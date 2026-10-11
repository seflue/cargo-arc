import type { ArcLogic } from './arc_logic.js';

// virtual_edge_logic.ts - Pure logic for virtual edge aggregation
// No DOM dependencies - uses Maps as data structures

const VirtualEdgeLogic = {
  /**
   * Determine aggregated direction from multiple directions.
   * Returns unanimous direction or 'downward' as fallback.
   * @param directions - Array of direction strings ('upward' | 'downward')
   * @returns - Aggregated direction
   */
  determineAggregatedDirection(directions: string[]): string {
    if (directions.length === 0) return 'downward';
    return directions.every((d) => d === directions[0])
      ? directions[0]
      : 'downward';
  },

  /**
   * Aggregate hidden edges by their visible endpoints.
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
  ): Map<
    string,
    {
      count: number;
      hiddenEdgeData: ArcData['usages'][];
      directions: string[];
      originalArcs: string[];
      fromId: string;
      toId: string;
    }
  > {
    const virtualEdges = new Map();

    for (const edge of edges) {
      const {
        arcId,
        fromId,
        toId,
        fromHidden,
        toHidden,
        sourceLocations,
        direction,
      } = edge;

      // Skip if both endpoints are visible
      if (!fromHidden && !toHidden) continue;

      // Calculate visible endpoints
      const visibleFrom = fromHidden ? getVisibleAncestorFn(fromId) : fromId;
      const visibleTo = toHidden ? getVisibleAncestorFn(toId) : toId;

      // Skip self-loops (both map to same visible ancestor)
      if (!visibleFrom || !visibleTo || visibleFrom === visibleTo) continue;

      const key = `${visibleFrom}-${visibleTo}`;
      if (!virtualEdges.has(key)) {
        virtualEdges.set(key, {
          count: 0,
          hiddenEdgeData: [],
          directions: [],
          originalArcs: [],
          fromId: visibleFrom,
          toId: visibleTo,
        });
      }
      const existing = virtualEdges.get(key);

      existing.count++;
      if (sourceLocations) existing.hiddenEdgeData.push(sourceLocations);
      if (direction) existing.directions.push(direction);
      existing.originalArcs.push(arcId);
    }

    return virtualEdges;
  },

  /**
   * Prepare virtual edge data for rendering.
   * @param maxRight - Rightmost X coordinate
   * @param arcLogic - ArcLogic module with calculateArcPath, countLocations, calculateStrokeWidth
   * @param rowHeight - Row height for arc calculation
   */
  prepareVirtualEdgeData(
    virtualEdges: Map<
      string,
      {
        count: number;
        hiddenEdgeData: ArcData['usages'][];
        directions: string[];
        originalArcs: string[];
        fromId: string;
        toId: string;
      }
    >,
    nodePositions: Map<
      string,
      { x: number; y: number; width: number; height: number }
    >,
    maxRight: number,
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
      direction: string;
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
        maxRight,
        rowHeight,
      );

      // Calculate stroke width from aggregated locations
      const totalLocations = data.hiddenEdgeData.reduce(
        (sum, locs) => sum + arcLogic.countLocations(locs),
        0,
      );
      const strokeWidth = arcLogic.calculateStrokeWidth(totalLocations);

      // Determine direction
      const direction = this.determineAggregatedDirection(data.directions);

      mergedEdges.set(key, {
        fromId,
        toId,
        count: data.count,
        hiddenEdgeData: data.hiddenEdgeData,
        arc,
        strokeWidth,
        direction,
        originalArcs: data.originalArcs,
      });
    });

    return mergedEdges;
  },
};

export { VirtualEdgeLogic };
