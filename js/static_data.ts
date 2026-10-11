import { ArcLogic } from './arc_logic.js';

// static_data.ts - Helper for accessing STATIC_DATA
// Provides typed access to pre-rendered node/arc data from Rust
// Eliminates DOM reads for static properties (positions, parents, arc weights)

const StaticData = {
  _cycleNodeIds: null as Set<string> | null,

  /**
   * Get node data by ID
   * @param id - Node ID
   */
  getNode(id: string): NodeData | undefined {
    return STATIC_DATA.nodes[id];
  },

  /**
   * Get arc data by ID
   * @param id - Arc ID (format: "from-to")
   */
  getArc(id: string): ArcData | undefined {
    return STATIC_DATA.arcs[id];
  },

  /**
   * Get original position and dimensions for node (from initial layout)
   */
  getOriginalPosition(
    nodeId: string,
  ): { x: number; y: number; width: number; height: number } | null {
    const node = STATIC_DATA.nodes[nodeId];
    return node
      ? { x: node.x, y: node.y, width: node.width, height: node.height }
      : null;
  },

  /**
   * Get arc weight (number of usages)
   */
  getArcWeight(arcId: string): number {
    const usages = STATIC_DATA.arcs[arcId]?.usages ?? [];
    return usages.reduce((sum, g) => sum + g.locations.length, 0);
  },

  /**
   * Get arc usages (source locations array)
   */
  getArcUsages(arcId: string): ArcData['usages'] {
    return STATIC_DATA.arcs[arcId]?.usages ?? [];
  },

  /**
   * Get calculated stroke width for an arc based on usage count.
   * Uses ArcLogic.calculateStrokeWidth for consistent scaling.
   * @returns Stroke width in pixels (0.5 to 2.5)
   */
  getArcStrokeWidth(arcId: string): number {
    return ArcLogic.calculateStrokeWidth(this.getArcWeight(arcId));
  },

  /**
   * Get cluster data (cycle blocks etc.) for an SCC by ID.
   */
  getCluster(sccId: string | number):
    | {
        package: string;
        moduleCount: number;
        cycleCount: number;
        cycles: Array<Array<{ fromId: string; toId: string; symbols: number }>>;
      }
    | undefined {
    return STATIC_DATA.clusters ? STATIC_DATA.clusters[sccId] : undefined;
  },

  /**
   * Nodes lying on a cycle: both endpoints of every arc that carries
   * cycleIds. Derived once from the arcs, since STATIC_DATA never changes.
   */
  getCycleNodeIds(): Set<string> {
    if (!this._cycleNodeIds) {
      this._cycleNodeIds = new Set();
      for (const arc of Object.values(STATIC_DATA.arcs)) {
        if (!arc.cycleIds?.length) continue;
        this._cycleNodeIds.add(arc.from);
        this._cycleNodeIds.add(arc.to);
      }
    }
    return this._cycleNodeIds;
  },

  /**
   * Check if node has children
   */
  hasChildren(nodeId: string): boolean {
    return STATIC_DATA.nodes[nodeId]?.hasChildren ?? false;
  },

  /**
   * Get all node IDs
   */
  getAllNodeIds(): string[] {
    return Object.keys(STATIC_DATA.nodes);
  },

  /**
   * The node whose `file` matches, or `null` when none does - how
   * `?select=<file>` and the hotspot map's editor-follow events resolve to
   * a node on this page.
   */
  findNodeIdByFile(file: string): string | null {
    const found = Object.entries(STATIC_DATA.nodes).find(
      ([, node]) => node.file === file,
    );
    return found ? found[0] : null;
  },

  /**
   * Get all arc IDs
   */
  getAllArcIds(): string[] {
    return Object.keys(STATIC_DATA.arcs);
  },

  /**
   * Build parent map for TreeLogic (replaces TreeLogic.buildParentMap)
   * @returns childId -> parentId
   */
  buildParentMap(): Map<string, string> {
    const parentMap = new Map();
    for (const [nodeId, node] of Object.entries(STATIC_DATA.nodes)) {
      if (node.parent !== null) {
        parentMap.set(nodeId, node.parent);
      }
    }
    return parentMap;
  },

  /**
   * Split a node into its owning package and package-relative module path.
   *
   * Walks the `parent` chain up to the owning package so that distinct modules
   * sharing a leaf name (e.g. `resource` and `device::resource`) stay
   * distinguishable via `path` alone. `crate` is the Cargo package name (dash
   * form), or null for nodes without a package ancestor. `path` falls back to the
   * id itself for unknown nodes.
   */
  qualifiedParts(nodeId: string): { package: string | null; path: string } {
    const node = STATIC_DATA.nodes[nodeId];
    if (!node) return { package: null, path: nodeId };
    const segments = [node.name];
    let packageName = null;
    let parentId = node.parent;
    while (parentId != null) {
      const parent = STATIC_DATA.nodes[parentId];
      if (!parent) break;
      if (parent.type === 'package') {
        packageName = parent.name;
        break;
      }
      segments.push(parent.name);
      parentId = parent.parent;
    }
    segments.reverse();
    return { package: packageName, path: segments.join('::') };
  },

  /**
   * Check if a node is an external type (external-section or external package).
   */
  isExternalNode(nodeId: string): boolean {
    const node = STATIC_DATA.nodes[nodeId];
    if (!node) return false;
    return (
      node.type === 'external-section' ||
      node.type === 'external' ||
      node.type === 'external-transitive'
    );
  },

  /**
   * Check if an arc connects to any external node.
   */
  isExternalArc(arcId: string): boolean {
    const arc = STATIC_DATA.arcs[arcId];
    if (!arc) return false;
    return this.isExternalNode(arc.from) || this.isExternalNode(arc.to);
  },

  /**
   * Check if a node is a transitive external dependency.
   */
  isTransitiveNode(nodeId: string): boolean {
    const node = STATIC_DATA.nodes[nodeId];
    if (!node) return false;
    return node.type === 'external-transitive';
  },

  /**
   * Check if an arc connects to any transitive external node.
   */
  isTransitiveArc(arcId: string): boolean {
    const arc = STATIC_DATA.arcs[arcId];
    if (!arc) return false;
    return this.isTransitiveNode(arc.from) || this.isTransitiveNode(arc.to);
  },

  /**
   * Get nesting level of a node.
   * @param id - Node ID
   */
  getNodeNesting(id: string): number | undefined {
    return STATIC_DATA.nodes[id]?.nesting;
  },

  /**
   * Get the expand level from config (null if not set = all expanded).
   */
  getExpandLevel(): number | null {
    return STATIC_DATA.expandLevel ?? null;
  },

  /**
   * Get external package nodes grouped by name.
   * Packages with same name but different versions are grouped together.
   * @returns name -> [nodeId, ...] sorted by version
   */
  getExternalGroups(): Map<string, string[]> {
    const groups = new Map();
    for (const [nodeId, node] of Object.entries(STATIC_DATA.nodes)) {
      if (node.type !== 'external' && node.type !== 'external-transitive')
        continue;
      const name = node.name;
      if (!groups.has(name)) groups.set(name, []);
      groups.get(name).push(nodeId);
    }
    // Only return groups with multiple versions
    const result = new Map();
    for (const [name, ids] of groups) {
      if (ids.length > 1) result.set(name, ids);
    }
    return result;
  },

  /**
   * Get all relations (incoming + outgoing arcs) for a node.
   */
  getNodeRelations(nodeId: string): {
    outgoing: Array<{
      targetId: string;
      weight: number;
      usages: Array<any>;
      arcId: string;
    }>;
    incoming: Array<{
      targetId: string;
      weight: number;
      usages: Array<any>;
      arcId: string;
    }>;
  } {
    const outgoing = [];
    const incoming = [];
    for (const [arcId, arc] of Object.entries(STATIC_DATA.arcs)) {
      if (arc.from === nodeId) {
        outgoing.push({
          targetId: arc.to,
          weight: this.getArcWeight(arcId),
          usages: arc.usages ?? [],
          arcId,
        });
      } else if (arc.to === nodeId) {
        incoming.push({
          targetId: arc.from,
          weight: this.getArcWeight(arcId),
          usages: arc.usages ?? [],
          arcId,
        });
      }
    }
    const byTreeOrder = (a, b) => {
      const yA = this.getOriginalPosition(a.targetId)?.y ?? Infinity;
      const yB = this.getOriginalPosition(b.targetId)?.y ?? Infinity;
      return yA - yB;
    };
    outgoing.sort(byTreeOrder);
    incoming.sort(byTreeOrder);
    return { outgoing, incoming };
  },
};

export { StaticData };
