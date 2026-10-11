import { AppState } from './app_state.js';
import { DomAdapter } from './dom_adapter.js';
import { StaticData } from './static_data.js';

// search.ts - Substring search with scope selector and highlight dimming

const SearchLogic = {
  _state: {
    active: false,
    query: '',
    scope: 'all',
    matchedNodeIds: new Set<string>(),
    matchParentIds: new Set<string>(),
    matchedArcIds: new Set<string>(),
    contextNodeIds: new Set<string>(),
    debounceTimer: null as number | null,
  },
  _appState: null as AppState | null,

  /**
   * Initialize search event listeners.
   * @param appState - AppState instance (for collapse checks)
   */
  init(appState: AppState) {
    this._appState = appState;

    const input = DomAdapter.querySelector<HTMLInputElement>('#search-input');
    const clearBtn = DomAdapter.querySelector<HTMLElement>('#search-clear');
    const scopeSelector =
      DomAdapter.querySelector<HTMLElement>('#scope-selector');

    if (input) {
      input.addEventListener('input', (e) => this._onInput(e));
    }
    if (clearBtn) {
      clearBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        if (input) input.value = '';
        clearBtn.style.display = 'none';
        this.clearSearch();
      });
    }
    if (scopeSelector) {
      scopeSelector.addEventListener('click', (e) => {
        if (!(e.target instanceof Element)) return;
        const btn = e.target.closest<HTMLElement>('[data-scope]');
        if (!btn) return;
        e.stopPropagation();
        this.setScope(btn.dataset.scope ?? '');
      });
    }
  },

  _onInput(e: Event) {
    if (!(e.target instanceof HTMLInputElement)) return;
    const query = e.target.value;
    const clearBtn = DomAdapter.querySelector<HTMLElement>('#search-clear');
    if (clearBtn) clearBtn.style.display = query ? 'block' : 'none';

    clearTimeout(this._state.debounceTimer ?? undefined);

    if (!query.trim()) {
      this.clearSearch();
      return;
    }

    this._state.debounceTimer = setTimeout(() => {
      this.executeSearch(query, this._state.scope);
    }, 150);
  },

  /**
   * Execute search with given query and scope.
   * @param scope - 'all', 'package', 'module', 'symbol'
   * @returns Number of matches
   */
  executeSearch(query: string, scope: string): number {
    const q = query.toLowerCase().trim();
    if (!q) {
      this.clearSearch();
      return 0;
    }

    this._state.query = q;
    this._state.scope = scope;

    const matchedNodes = new Set<string>();

    if (scope === 'all' || scope === 'package' || scope === 'module') {
      for (const nodeId of StaticData.getAllNodeIds()) {
        const node = StaticData.getNode(nodeId);
        if (!node) continue;
        if (scope === 'package' && node.type !== 'package') continue;
        if (scope === 'module' && node.type !== 'module') continue;
        if (node.name.toLowerCase().includes(q)) {
          matchedNodes.add(nodeId);
        }
      }
    }

    // An arc matches by its own symbols only; a name match on a
    // node no longer spreads to the arcs touching it.
    const matchedArcs = new Set<string>();
    const contextNodeIds = new Set<string>();

    if (scope === 'all' || scope === 'symbol') {
      for (const arcId of StaticData.getAllArcIds()) {
        const arc = StaticData.getArc(arcId);
        if (!arc || !arc.usages) continue;
        for (const group of arc.usages) {
          if (group.symbol?.toLowerCase().includes(q)) {
            matchedArcs.add(arcId);
            contextNodeIds.add(arc.from);
            contextNodeIds.add(arc.to);
            break;
          }
        }
      }
    }

    // Collapsed-parent-resolution
    const directMatches = new Set<string>();
    const parentMatches = new Set<string>();

    for (const nodeId of matchedNodes) {
      const visible = this._resolveVisibleAncestor(nodeId);
      if (visible === nodeId) {
        directMatches.add(nodeId);
      } else {
        parentMatches.add(visible);
      }
    }

    // A matched arc's endpoints are context, not matches: visible,
    // no match style, not counted. A node that is already a real match
    // keeps that match instead.
    const contextMatches = new Set<string>();
    for (const nodeId of contextNodeIds) {
      const visible = this._resolveVisibleAncestor(nodeId);
      if (directMatches.has(visible) || parentMatches.has(visible)) continue;
      contextMatches.add(visible);
    }

    // Diff-based DOM updates for match/parent/context highlight classes
    this._applySearchDiff(
      this._state.matchedNodeIds,
      this._state.matchParentIds,
      directMatches,
      parentMatches,
    );
    this._applyContextDiff(this._state.contextNodeIds, contextMatches);

    // Clear previous arc matches, then mark new matched arc elements
    this._clearArcMatches();
    this._applyArcMatches(matchedArcs);

    this._state.matchedNodeIds = directMatches;
    this._state.matchParentIds = parentMatches;
    this._state.matchedArcIds = matchedArcs;
    this._state.contextNodeIds = contextMatches;
    this._state.active = true;
    const svg = DomAdapter.getSvgRoot();
    if (svg) svg.classList.add(STATIC_DATA.classes.search.searchActive);

    const nodeMatchCount = directMatches.size + parentMatches.size;
    const total = nodeMatchCount + matchedArcs.size;
    const countEl = DomAdapter.getElementById('search-result-count');
    if (countEl) {
      countEl.textContent = this._formatResultCount(
        directMatches,
        parentMatches,
        matchedArcs.size,
      );
    }

    // Update scope button active state
    this._updateScopeButtons(scope);

    return total;
  },

  /**
   * Format the result count split by kind, e.g. "1 module, 2 edges".
   * Kinds with zero matches are omitted; an all-zero result reads "0 matches".
   */
  _formatResultCount(
    directMatches: Set<string>,
    parentMatches: Set<string>,
    edgeCount: number,
  ): string {
    const nodeKindCounts = new Map();
    for (const nodeId of [...directMatches, ...parentMatches]) {
      const kind = this._nodeKind(nodeId);
      nodeKindCounts.set(kind, (nodeKindCounts.get(kind) ?? 0) + 1);
    }

    const words = STATIC_DATA.wording;
    const nouns = {
      package: [words.package, words.packages],
      module: ['module', 'modules'],
    };
    const parts = [];
    for (const kind of ['package', 'module'] as const) {
      const n = nodeKindCounts.get(kind) ?? 0;
      if (n > 0) parts.push(`${n} ${nouns[kind][n !== 1 ? 1 : 0]}`);
    }
    if (edgeCount > 0) {
      parts.push(`${edgeCount} edge${edgeCount !== 1 ? 's' : ''}`);
    }

    return parts.length > 0 ? parts.join(', ') : '0 matches';
  },

  /**
   * A node's kind for the result count: 'module', or 'package' for anything
   * else (including external and transitive-external nodes).
   */
  _nodeKind(nodeId: string): 'package' | 'module' {
    const node = StaticData.getNode(nodeId);
    return node?.type === 'module' ? 'module' : 'package';
  },

  /**
   * Clear all search highlights.
   */
  clearSearch() {
    this._clearDom();
    const svg = DomAdapter.getSvgRoot();
    if (svg) svg.classList.remove(STATIC_DATA.classes.search.searchActive);
    this._state.active = false;
    this._state.query = '';
    this._state.matchedNodeIds = new Set<string>();
    this._state.matchParentIds = new Set<string>();
    this._state.matchedArcIds = new Set<string>();
    this._state.contextNodeIds = new Set<string>();

    const countEl = DomAdapter.getElementById('search-result-count');
    if (countEl) countEl.textContent = '';
  },

  /**
   * Change scope and re-execute current query.
   */
  setScope(scope: string) {
    this._state.scope = scope;
    this._updateScopeButtons(scope);
    if (this._state.query) {
      this.executeSearch(this._state.query, scope);
    }
  },

  isActive() {
    return this._state.active;
  },

  refresh() {
    if (this._state.active && this._state.query) {
      this.executeSearch(this._state.query, this._state.scope);
    }
  },

  getMatchedNodeIds() {
    return this._state.matchedNodeIds;
  },

  // --- Internal helpers ---

  _clearDom() {
    const C = STATIC_DATA.classes;

    for (const nodeId of this._state.matchedNodeIds) {
      this._setNodeClass(nodeId, C.search.searchMatch, false);
    }
    for (const nodeId of this._state.matchParentIds) {
      this._setNodeClass(nodeId, C.search.searchMatchParent, false);
    }
    for (const nodeId of this._state.contextNodeIds) {
      this._setNodeClass(nodeId, C.search.searchContext, false);
    }

    this._clearArcMatches();
  },

  /** Remove search-match class from all arc elements (paths, arrows, labels). */
  _clearArcMatches() {
    const C = STATIC_DATA.classes;

    for (const arcId of this._state.matchedArcIds) {
      const arc = DomAdapter.getVisibleArc(arcId);
      if (arc) arc.classList.remove(C.search.searchMatch);
      for (const arrow of DomAdapter.getVisibleArrows(arcId)) {
        arrow.classList.remove(C.search.searchMatch);
      }
      const labelGroup = DomAdapter.getLabelGroup(arcId);
      if (labelGroup) {
        for (const child of labelGroup.children) {
          child.classList.remove(C.search.searchMatch);
        }
      }
      this._setVirtualArcMatch(arcId, false);
    }
  },

  /**
   * Toggle search-match on the virtual arc standing in for a real arc whose
   * endpoint sits behind a collapsed ancestor: the real arc has no
   * visible element then, only the virtual one aggregating it does.
   * No-op when both endpoints are visible, since no virtual arc exists.
   */
  _setVirtualArcMatch(arcId: string, add: boolean) {
    const arc = StaticData.getArc(arcId);
    if (!arc) return;
    const visibleFrom = this._resolveVisibleAncestor(arc.from);
    const visibleTo = this._resolveVisibleAncestor(arc.to);
    if (visibleFrom === arc.from && visibleTo === arc.to) return;
    if (visibleFrom === visibleTo) return;

    const C = STATIC_DATA.classes;
    const virtualArcId = `${visibleFrom}-${visibleTo}`;
    const toggle = (el: Element) =>
      el.classList[add ? 'add' : 'remove'](C.search.searchMatch);

    DomAdapter.querySelectorAll(
      `.${C.direction.virtualArc}[data-arc-id="${virtualArcId}"]`,
    ).forEach(toggle);
    DomAdapter.querySelectorAll(
      `.${C.direction.virtualArrow}[data-vedge="${virtualArcId}"]`,
    ).forEach(toggle);
    DomAdapter.querySelectorAll(
      `.${C.labels.arcCount}[data-vedge="${virtualArcId}"]`,
    ).forEach(toggle);
  },

  /**
   * Diff old vs new match sets; only touch DOM elements whose state changed.
   * Reduces DOM mutations from O(total) to O(delta) during incremental typing.
   */
  _applySearchDiff(
    oldDirect: Set<string>,
    oldParent: Set<string>,
    newDirect: Set<string>,
    newParent: Set<string>,
  ) {
    const C = STATIC_DATA.classes;

    // Remove classes from nodes that left their set
    for (const nodeId of oldDirect) {
      if (!newDirect.has(nodeId)) {
        this._setNodeClass(nodeId, C.search.searchMatch, false);
      }
    }
    for (const nodeId of oldParent) {
      if (!newParent.has(nodeId) || newDirect.has(nodeId)) {
        this._setNodeClass(nodeId, C.search.searchMatchParent, false);
      }
    }

    // Add classes to nodes that joined their set
    for (const nodeId of newDirect) {
      if (!oldDirect.has(nodeId)) {
        this._setNodeClass(nodeId, C.search.searchMatch, true);
      }
    }
    for (const nodeId of newParent) {
      if (newDirect.has(nodeId)) continue;
      if (!oldParent.has(nodeId)) {
        this._setNodeClass(nodeId, C.search.searchMatchParent, true);
      }
    }
  },

  /**
   * Diff old vs new context-node sets, same delta-only approach as
   * _applySearchDiff.
   */
  _applyContextDiff(oldContext: Set<string>, newContext: Set<string>) {
    const C = STATIC_DATA.classes;

    for (const nodeId of oldContext) {
      if (!newContext.has(nodeId)) {
        this._setNodeClass(nodeId, C.search.searchContext, false);
      }
    }
    for (const nodeId of newContext) {
      if (!oldContext.has(nodeId)) {
        this._setNodeClass(nodeId, C.search.searchContext, true);
      }
    }
  },

  _setNodeClass(nodeId: string, className: string, add: boolean) {
    const rect = DomAdapter.getNode(nodeId);
    if (!rect) return;
    rect.classList.toggle(className, add);
    const label = rect.nextElementSibling;
    if (label?.classList.contains(STATIC_DATA.classes.nodes.label)) {
      label.classList.toggle(className, add);
    }
  },

  /** Add search-match class to matched arc elements (CSS-only dimming via svg.search-active). */
  _applyArcMatches(matchedArcs: Set<string>) {
    const C = STATIC_DATA.classes;

    // Add search-match to matched arc elements so CSS excludes them from dimming
    for (const arcId of matchedArcs) {
      const arc = DomAdapter.getVisibleArc(arcId);
      if (arc) arc.classList.add(C.search.searchMatch);
      for (const arrow of DomAdapter.getVisibleArrows(arcId)) {
        arrow.classList.add(C.search.searchMatch);
      }
      const labelGroup = DomAdapter.getLabelGroup(arcId);
      if (labelGroup) {
        for (const child of labelGroup.children) {
          child.classList.add(C.search.searchMatch);
        }
      }
      this._setVirtualArcMatch(arcId, true);
    }
  },

  _resolveVisibleAncestor(nodeId: string) {
    const appState = this._appState;
    let current = nodeId;
    while (current) {
      const node = StaticData.getNode(current);
      if (!node || node.parent == null) return current;
      if (appState && AppState.isCollapsed(appState, node.parent)) {
        current = node.parent;
      } else {
        return current;
      }
    }
    return nodeId;
  },

  _updateScopeButtons(scope: string) {
    const C = STATIC_DATA.classes;
    const selector = DomAdapter.querySelector('#scope-selector');
    if (!selector) return;
    const buttons = selector.querySelectorAll<HTMLElement>('[data-scope]');
    buttons.forEach((btn) => {
      btn.classList.toggle(C.toolbar.scopeActive, btn.dataset.scope === scope);
    });
  },
};

export { SearchLogic };
