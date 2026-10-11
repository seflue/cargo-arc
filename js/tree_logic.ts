// tree_logic.ts - Pure tree traversal logic
// No DOM dependencies - operates on Maps/Sets
// State management moved to AppState module

const TreeLogic = {
  /**
   * Get all descendants of a node (recursive)
   * @param parentMap - childId -> parentId
   */
  getDescendants(nodeId: string, parentMap: Map<string, string>): string[] {
    const descendants = [];
    for (const [childId, parentId] of parentMap) {
      if (parentId === nodeId) {
        descendants.push(childId);
        descendants.push(...this.getDescendants(childId, parentMap));
      }
    }
    return descendants;
  },

  /**
   * Count all descendants
   */
  countDescendants(nodeId: string, parentMap: Map<string, string>): number {
    return this.getDescendants(nodeId, parentMap).length;
  },

  /**
   * Whether any descendant (not the node itself) lies on a cycle.
   * @param cycleNodeIds - Nodes lying on a cycle
   */
  hasCycleDescendant(
    nodeId: string,
    parentMap: Map<string, string>,
    cycleNodeIds: Set<string>,
  ): boolean {
    return this.getDescendants(nodeId, parentMap).some((id) =>
      cycleNodeIds.has(id),
    );
  },

  /**
   * Find visible ancestor (or self if visible)
   * A node is hidden if ANY ancestor is collapsed.
   * @param collapsedSet - Set of collapsed node IDs
   * @param parentMap - childId -> parentId
   */
  getVisibleAncestor(
    nodeId: string,
    collapsedSet: Set<string>,
    parentMap: Map<string, string>,
  ): string | null {
    const parentId = parentMap.get(nodeId);
    if (!parentId) return nodeId;

    if (collapsedSet.has(parentId)) {
      return this.getVisibleAncestor(parentId, collapsedSet, parentMap);
    }

    const parentsVisibleAncestor = this.getVisibleAncestor(
      parentId,
      collapsedSet,
      parentMap,
    );
    if (parentsVisibleAncestor !== parentId) {
      return parentsVisibleAncestor;
    }

    return nodeId;
  },
};

export { TreeLogic };
