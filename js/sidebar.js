// @module SidebarLogic
// @deps StaticData, DomAdapter, Selectors, PathFit
// @config TOOLBAR_HEIGHT, SIDEBAR_SHADOW_PAD
// sidebar.js - Relation sidebar for arc usage details
// Shows usage locations when an arc is selected (pinned)
// foreignObject-based HTML sidebar with scroll tracking

const TOOLBAR_HEIGHT =
  typeof __TOOLBAR_HEIGHT__ !== 'undefined' ? __TOOLBAR_HEIGHT__ : 0;
const SIDEBAR_GAP_X = 24;
const SIDEBAR_MARGIN_RIGHT = 16;
const SIDEBAR_GAP_TOP = 20;
// foreignObject must be taller than the visible sidebar so box-shadow
// (which renders outside the div) is not clipped by the foreignObject boundary.
// Value derived from box-shadow offset+blur in render.rs layout constants.
const SIDEBAR_SHADOW_PAD =
  typeof __SIDEBAR_SHADOW_PAD__ !== 'undefined' ? __SIDEBAR_SHADOW_PAD__ : 12;
const SIDEBAR_MIN_WIDTH = 280;
// Generous character budget for a cycle-block header path. The panel auto-widens
// to the content up to half the viewport, so a header only fails to fit for very
// long labels; below this budget the second node stays, above it the header drops
// to head + closing edge so the CSS net never has to cut the closing edge itself.
const CYCLE_HEADER_MAX_CHARS = 48;
// The rows the collapse-all button drives: the relation rows of a node
// sidebar and the symbol rows of an arc sidebar, both direct children of
// the content's usage groups. A node sidebar nests a second level of
// collapsible symbol rows inside each relation row; those start expanded and
// must not count, or the first click would collapse instead of expand.
const COLLAPSIBLE_SYMBOL_SELECTOR =
  ':scope > .sidebar-usage-group > .sidebar-symbol[data-collapsible]';

/**
 * The visible page area in CSS pixels. `window.innerWidth`/`innerHeight`
 * include the scrollbars, so a sidebar clamped to them ends under the
 * vertical scrollbar whenever the page scrolls, as it does in an editor pane.
 * @returns {{ width: number, height: number }}
 */
function visibleArea() {
  const root = document.documentElement;
  return { width: root.clientWidth, height: root.clientHeight };
}

const SidebarLogic = {
  _isTransient: false,
  /** @type {number | null} */
  _debounceTimer: null,
  /** @type {((nodeId: string | undefined) => void) | null} */
  _onBadgeClick: null,
  /** @type {((target: string | undefined) => void) | null} */
  _onCollapseToggle: null,
  /** @type {((id: string) => boolean) | null} */
  _isNodeCollapsed: null,
  /** @type {(() => boolean) | null} */
  _isClusterMode: null,
  /** @type {(() => (string | null)) | null} */
  _resolvedFocusArc: null,
  /** @type {((arcId: string | undefined) => void) | null} */
  _onEdgeHover: null,
  /** @type {(() => void) | null} */
  _onEdgeHoverEnd: null,
  /** @type {((arcId: string, expandable: boolean, expanded: boolean) => boolean) | null} */
  _onEdgeClick: null,
  /**
   * Merge symbol groups: combine groups with same symbol, deduplicate locations by file+line.
   * @param {Array<{symbol: string, modulePath: string|null, locations: Array<{file: string, line: number}>, definition?: {file: string, line: number, jump: number}}>} groups
   * @returns {Array<{symbol: string, modulePath: string|null, locations: Array<{file: string, line: number}>, definition?: {file: string, line: number, jump: number}}>}
   */
  mergeSymbolGroups(groups) {
    const bySymbol = new Map();
    for (const g of groups) {
      const key = g.symbol || '';
      const existing = bySymbol.get(key);
      if (existing) {
        for (const loc of g.locations) {
          const isDup = existing.locations.some(
            (e) => e.file === loc.file && e.line === loc.line,
          );
          if (!isDup) existing.locations.push(loc);
        }
      } else {
        bySymbol.set(key, {
          symbol: g.symbol,
          modulePath: g.modulePath,
          locations: [...g.locations],
          ...(g.definition ? { definition: g.definition } : {}),
        });
      }
    }
    return [...bySymbol.values()];
  },

  /**
   * Build HTML content string for the sidebar.
   * Uses overrideData if provided, otherwise STATIC_DATA.arcs[arcId].
   * Expects structured usages: [{ symbol, modulePath, locations: [{ file, line }] }]
   * @param {string} arcId
   * @param {{ from: string, to: string, usages: StaticArcData["usages"], originalArcs?: string[], cycleIds?: number[] }} [overrideData]
   * @returns {string}
   */
  buildContent(arcId, overrideData) {
    const arc = overrideData || STATIC_DATA.arcs[arcId];
    if (!arc) return '';

    // Cluster view: when cluster mode is on and the arc lies in an SCC with
    // known cycles, describe the whole cluster instead of the single edge.
    const sccId = STATIC_DATA.arcs[arcId]?.sccId;
    if (
      !overrideData &&
      this._isClusterMode?.() &&
      sccId != null &&
      STATIC_DATA.clusters &&
      STATIC_DATA.clusters[sccId]
    ) {
      return this._buildClusterContent(
        String(sccId),
        this._resolvedFocusArc?.() ?? undefined,
      );
    }
    const groups = arc.usages || [];

    const fromNode = StaticData.getNode(arc.from);
    const toNode = StaticData.getNode(arc.to);
    const fromName = this._formatNodeName(fromNode, arc.from);
    const toName = this._formatNodeName(toNode, arc.to);
    const fromClass = `${fromNode ? `sidebar-node-${fromNode.type} ` : ''}sidebar-node-from`;
    const toClass = `${toNode ? `sidebar-node-${toNode.type} ` : ''}sidebar-node-to`;

    let html = `<div class="sidebar-header">`;
    html += `<span class="sidebar-title"><span class="${fromClass}" data-node-id="${arc.from}">${fromName}${this._renderCollapseIndicator(arc.from)}</span><span class="sidebar-arrow">&#x2192;</span><span class="${toClass}" data-node-id="${arc.to}">${toName}${this._renderCollapseIndicator(arc.to)}</span></span>`;
    const hasSymbols = groups.some((g) => g.symbol);
    if (hasSymbols) {
      html += `<div class="sidebar-header-actions">`;
      html += `<button class="sidebar-collapse-all" title="Collapse all">&#x2212;</button>`;
      html += `<button class="sidebar-close">&#x2715;</button>`;
      html += `</div>`;
    } else {
      html += `<button class="sidebar-close">&#x2715;</button>`;
    }
    html += `</div>`;

    html += `<div class="sidebar-content">`;
    if (groups.length === 0) {
      html += `<div class="sidebar-usage-group">Cargo.toml dependency</div>`;
    } else {
      const sorted = [...groups].sort(
        (a, b) => b.locations.length - a.locations.length,
      );
      for (const group of sorted) {
        html += `<div class="sidebar-usage-group">`;
        if (group.symbol) {
          html += `<div class="sidebar-symbol" data-collapsible="" title="${this._symbolTitle(group)}">`;
          html += `<span class="sidebar-toggle">&#x25BE;</span>`;
          html += this._symbolLabel(group);
          html += this._definitionChip(group);
          html += this._renderLocalityTag(arc.to, group.symbol);
          html += `<span class="sidebar-ref-count">${group.locations.length}</span>`;
          html += `</div>`;
        }
        html += `<div class="sidebar-locations">`;
        for (const loc of group.locations) {
          html += this._locationRow(loc);
        }
        html += `</div>`;
        html += `</div>`;
      }
    }
    html += `</div>`;

    // Footer
    const totalLocs = groups.reduce((sum, g) => sum + g.locations.length, 0);
    const symbolCount = groups.filter((g) => g.symbol).length;
    let footerText =
      groups.length === 0
        ? 'Cargo.toml dependency'
        : `${totalLocs} References \u00b7 ${symbolCount} Symbols`;
    if (overrideData?.originalArcs?.length) {
      footerText += ` \u00b7 ${overrideData.originalArcs.length} hidden Dependencies`;
    }
    html += `<div class="sidebar-footer">${footerText}</div>`;

    return html;
  },

  /**
   * Build HTML content string for node-mode sidebar.
   * Shows all incoming (dependents) and outgoing (dependencies) relations.
   * @param {string} nodeId - The selected node ID
   * @param {{ incoming: Array, outgoing: Array }} relations - From collectNodeRelations() (filtered + virtual arcs)
   * @returns {string}
   */
  buildNodeContent(nodeId, relations) {
    const node = StaticData.getNode(nodeId);
    const nodeName = this._formatNodeName(node, nodeId);
    const nodeType = node ? node.type : '';
    const hasRelations =
      relations.incoming.length > 0 || relations.outgoing.length > 0;
    const hasCollapsible = [...relations.incoming, ...relations.outgoing].some(
      (rel) => (rel.usages || []).length > 0,
    );

    // Header: Node name + Collapse-All ("+", since all L1 start collapsed) + Close
    let html = `<div class="sidebar-header">`;
    html += `<span class="sidebar-title"><span class="sidebar-node-${nodeType} sidebar-node-selected" data-node-id="${nodeId}">${nodeName}${this._renderCollapseIndicator(nodeId)}</span></span>`;
    if (hasCollapsible) {
      html += `<div class="sidebar-header-actions">`;
      html += `<button class="sidebar-collapse-all">+</button>`;
      html += `<button class="sidebar-close">&#x2715;</button>`;
      html += `</div>`;
    } else {
      html += `<button class="sidebar-close">&#x2715;</button>`;
    }
    html += `</div>`;

    html += `<div class="sidebar-content">`;

    if (!hasRelations) {
      html += `<div class="sidebar-usage-group">No relations</div>`;
    } else {
      const badgeLens = this._computeMaxBadgeLengths(
        relations,
        nodeId,
        nodeName,
      );

      // Incoming (Dependents) first — selected node on the right
      for (const rel of relations.incoming) {
        html += this._buildRelationSection(
          rel,
          nodeId,
          nodeName,
          nodeType,
          'incoming',
          badgeLens.incoming.maxFrom,
          badgeLens.incoming.maxTo,
        );
      }

      // Divider only if both directions non-empty
      if (relations.incoming.length > 0 && relations.outgoing.length > 0) {
        html += `<hr class="sidebar-divider"/>`;
      }

      // Outgoing (Dependencies) — selected node on the left
      for (const rel of relations.outgoing) {
        html += this._buildRelationSection(
          rel,
          nodeId,
          nodeName,
          nodeType,
          'outgoing',
          badgeLens.outgoing.maxFrom,
          badgeLens.outgoing.maxTo,
        );
      }
    }

    html += `</div>`;

    // Footer
    html += `<div class="sidebar-footer">${relations.incoming.length} Dependents \u00b7 ${relations.outgoing.length} Dependencies</div>`;

    return html;
  },

  /**
   * Build a single Level-1 relation section (collapsed) with nested Level-2 usages.
   * @param {Object} rel - Relation entry {targetId, weight, usages, arcId}
   * @param {string} nodeId - Selected node ID
   * @param {string} nodeName - Selected node display name
   * @param {string} nodeType - Selected node type (crate/module)
   * @param {'incoming'|'outgoing'} direction
   * @param {number} [maxFromLen=0] - Minimum badge width (ch) for from-badge
   * @param {number} [maxToLen=0] - Minimum badge width (ch) for to-badge
   * @returns {string}
   */
  _buildRelationSection(
    rel,
    nodeId,
    nodeName,
    nodeType,
    direction,
    maxFromLen = 0,
    maxToLen = 0,
  ) {
    const target = StaticData.getNode(rel.targetId);
    const targetName = this._formatNodeName(target, rel.targetId);
    const targetType = target ? target.type : '';

    // Build From→To pair: direction determines which side the selected node is on
    let fromName,
      fromType,
      fromSelected,
      fromId,
      toName,
      toType,
      toSelected,
      toId;
    if (direction === 'incoming') {
      // source → [selected]: selected is on the right
      fromName = targetName;
      fromType = targetType;
      fromSelected = false;
      fromId = rel.targetId;
      toName = nodeName;
      toType = nodeType;
      toSelected = true;
      toId = nodeId;
    } else {
      // [selected] → target: selected is on the left
      fromName = nodeName;
      fromType = nodeType;
      fromSelected = true;
      fromId = nodeId;
      toName = targetName;
      toType = targetType;
      toSelected = false;
      toId = rel.targetId;
    }

    const fromClass = `sidebar-node-${fromType}${fromSelected ? ' sidebar-node-selected' : ' sidebar-node-from'}`;
    const toClass = `sidebar-node-${toType}${toSelected ? ' sidebar-node-selected' : ' sidebar-node-to'}`;
    const fromStyle =
      maxFromLen > 0 ? ` style="min-width: ${maxFromLen}ch"` : '';
    const toStyle = maxToLen > 0 ? ` style="min-width: ${maxToLen}ch"` : '';

    const groups = rel.usages || [];
    let html = `<div class="sidebar-usage-group">`;

    // External dependency without source references: flat row, no expand
    if (groups.length === 0) {
      html += `<div class="sidebar-symbol" style="cursor:default">`;
      html += `<span class="sidebar-toggle"></span>`;
      html += `<span class="${fromClass} sidebar-symbol-name"${fromStyle} data-node-id="${fromId}">${fromName}${this._renderCollapseIndicator(fromId)}</span>`;
      html += `<span class="sidebar-arrow">&#x2192;</span>`;
      html += `<span class="${toClass} sidebar-symbol-name"${toStyle} data-node-id="${toId}">${toName}${this._renderCollapseIndicator(toId)}</span>`;
      html += `<span class="sidebar-ext-info" title="Cargo.toml dependency &#8212; source references are not tracked for external crates">i</span>`;
      html += `</div>`;
      html += `</div>`;
      return html;
    }

    // Level 1 header (collapsed)
    html += `<div class="sidebar-symbol" data-collapsible="" data-collapsed="true">`;
    html += `<span class="sidebar-toggle">&#x25B8;</span>`;
    html += `<span class="${fromClass} sidebar-symbol-name"${fromStyle} data-node-id="${fromId}">${fromName}${this._renderCollapseIndicator(fromId)}</span>`;
    html += `<span class="sidebar-arrow">&#x2192;</span>`;
    html += `<span class="${toClass} sidebar-symbol-name"${toStyle} data-node-id="${toId}">${toName}${this._renderCollapseIndicator(toId)}</span>`;
    html += `<span class="sidebar-ref-count">${rel.weight}</span>`;
    html += `</div>`;

    // Level 2 content (hidden because L1 is collapsed)
    html += `<div class="sidebar-locations" style="display:none">`;
    const sorted = [...groups].sort(
      (a, b) => b.locations.length - a.locations.length,
    );
    for (const group of sorted) {
      html += `<div class="sidebar-usage-group">`;
      if (group.symbol) {
        html += `<div class="sidebar-symbol" data-collapsible="" title="${this._symbolTitle(group)}">`;
        html += `<span class="sidebar-toggle">&#x25BE;</span>`;
        html += this._symbolLabel(group);
        html += this._definitionChip(group);
        html += `<span class="sidebar-ref-count">${group.locations.length}</span>`;
        html += `</div>`;
      }
      html += `<div class="sidebar-locations">`;
      for (const loc of group.locations) {
        html += this._locationRow(loc);
      }
      html += `</div>`;
      html += `</div>`;
    }
    html += `</div>`;

    html += `</div>`;
    return html;
  },

  /**
   * Compute maximum badge text lengths per section for width normalization.
   * Accounts for the +/- collapse indicator (1ch extra) when present.
   * @param {{ incoming: Array, outgoing: Array }} relations
   * @param {string} nodeId - ID of the selected node
   * @param {string} nodeName - Display name of the selected node
   * @returns {{ incoming: { maxFrom: number, maxTo: number }, outgoing: { maxFrom: number, maxTo: number } }}
   */
  _computeMaxBadgeLengths(relations, nodeId, nodeName) {
    const effectiveLen = (id, name) => {
      let len = name.length;
      if (StaticData.hasChildren(id)) {
        const collapsed = this._isNodeCollapsed?.(id);
        if (collapsed !== undefined && collapsed !== null) len += 1;
      }
      return len;
    };
    const selectedLen = effectiveLen(nodeId, nodeName);
    let inMaxFrom = 0;
    for (const rel of relations.incoming) {
      const target = StaticData.getNode(rel.targetId);
      const targetName = this._formatNodeName(target, rel.targetId);
      inMaxFrom = Math.max(inMaxFrom, effectiveLen(rel.targetId, targetName));
    }
    let outMaxTo = 0;
    for (const rel of relations.outgoing) {
      const target = StaticData.getNode(rel.targetId);
      const targetName = this._formatNodeName(target, rel.targetId);
      outMaxTo = Math.max(outMaxTo, effectiveLen(rel.targetId, targetName));
    }
    return {
      incoming: {
        maxFrom: inMaxFrom,
        maxTo: relations.incoming.length > 0 ? selectedLen : 0,
      },
      outgoing: {
        maxFrom: relations.outgoing.length > 0 ? selectedLen : 0,
        maxTo: outMaxTo,
      },
    };
  },

  /**
   * Format node display name, appending version for external crates.
   * @param {Object|null} node - Node data from StaticData
   * @param {string} fallback - Fallback ID if node is null
   * @returns {string}
   */
  _formatNodeName(node, fallback) {
    if (!node) return fallback;
    if (node.version) return `${node.name} v${node.version}`;
    return node.name;
  },

  /**
   * Render a +/- collapse indicator span for a node badge.
   * Returns empty string for leaf nodes or when state callback is not set.
   * @param {string} nodeId
   * @returns {string}
   */
  _renderCollapseIndicator(nodeId) {
    if (!StaticData.hasChildren(nodeId)) return '';
    const collapsed = this._isNodeCollapsed?.(nodeId);
    if (collapsed === undefined || collapsed === null) return '';
    const symbol = collapsed ? '+' : '\u2212';
    return `<span class="sidebar-collapse-indicator" data-collapse-target="${nodeId}">${symbol}</span>`;
  },

  /**
   * Consumer-locality tag for one symbol of a provider. Empty unless
   * STATIC_DATA.symbolLocalities carries a locality for (providerId, symbol).
   * The tag states a descriptive fact about where the symbol is consumed, not a
   * verdict on whether to move it.
   * @param {string} providerId - The provider node id (the arc's `to`).
   * @param {string} symbol
   * @returns {string}
   */
  _renderLocalityTag(providerId, symbol) {
    const sl =
      typeof STATIC_DATA !== 'undefined' &&
      STATIC_DATA.symbolLocalities?.[providerId]?.[symbol];
    if (!sl) return '';
    const nameOf = (id) => StaticData.getNode(id)?.name ?? id;
    let label;
    if (sl.locality === 'singleConsumer') {
      label = `only used by ${nameOf(sl.module)}`;
    } else if (sl.locality === 'commonAncestor') {
      label = `used under ${nameOf(sl.module)}`;
    } else if (sl.locality === 'crateWide') {
      label = `widely used (${sl.consumers?.length ?? 0} modules)`;
    } else {
      return '';
    }
    return `<span class="sidebar-locality sidebar-locality-${sl.locality}">${label}</span>`;
  },

  /**
   * The jump chip on a symbol row, opening the symbol's definition. Empty
   * unless the group carries a definition (RenderConfig::with_jump_ids and a
   * definer the analysis could see). Hidden until the row is hovered, like
   * the location rows' icon; the click is a jump handled on the document.
   * @param {{ definition?: { file: string, line: number, jump: number } }} group
   * @returns {string}
   */
  _definitionChip(group) {
    const definition = group.definition;
    if (!definition) return '';
    return (
      `<span class="sidebar-definition" data-jump="${definition.jump}" title="${definition.file}:${definition.line}">` +
      '<svg class="sidebar-jump" xmlns="http://www.w3.org/2000/svg"><use href="#jump-icon"></use></svg></span>'
    );
  },

  /**
   * Full `modulePath::symbol` for the row's `title`.
   * @param {{ symbol: string, modulePath?: string | null }} group
   * @returns {string}
   */
  _symbolTitle(group) {
    return group.modulePath
      ? `${group.modulePath}::${group.symbol}`
      : group.symbol;
  },

  /**
   * Module path and symbol name spans. The path span carries its full text
   * in `data-full` because `fitPaths` rewrites the visible text.
   * @param {{ symbol: string, modulePath?: string | null }} group
   * @returns {string}
   */
  _symbolLabel(group) {
    let html = '';
    if (group.modulePath) {
      const ns = `${group.modulePath}::`;
      html += `<span class="sidebar-ns" data-full="${ns}">${ns}</span>`;
    }
    html += `<span class="sidebar-symbol-name">${group.symbol}</span>`;
    return html;
  },

  /**
   * One usage-location row. Carries `data-jump` only when the location has a
   * jump id (RenderConfig::with_jump_ids, i.e. `arc ui`); an image from
   * `cargo arc -o` never sets it, so the row stays inert there.
   * @param {{ file: string, line: number, jump?: number }} loc
   * @returns {string}
   */
  _locationRow(loc) {
    const hasJump = typeof loc.jump === 'number';
    const jumpAttr = hasJump ? ` data-jump="${loc.jump}"` : '';
    // Same symbol the node icons use; JumpIcons defines it in the SVG root
    // at page load, so the <use> resolves whenever a jump id exists. The
    // xmlns is required: this HTML lands in an XML document via innerHTML,
    // where an unprefixed <svg> would stay in the XHTML namespace.
    const jumpIcon = hasJump
      ? '<svg class="sidebar-jump" xmlns="http://www.w3.org/2000/svg"><use href="#jump-icon"></use></svg>'
      : '';
    return (
      `<div class="sidebar-location"${jumpAttr} title="${loc.file}:${loc.line}">` +
      `<span class="sidebar-file" data-full="${loc.file}">${loc.file}</span>` +
      `<span class="sidebar-line-badge">:${loc.line}</span>${jumpIcon}</div>`
    );
  },

  /**
   * Get the foreignObject element for the sidebar.
   * @returns {HTMLElement|null}
   */
  _getElement() {
    return DomAdapter.getElementById('relation-sidebar');
  },

  /**
   * Find the rightmost X coordinate among all visible arc paths.
   * Uses cached value when available — cache is invalidated by
   * invalidateLayout() (collapse/relayout) and hide().
   * @returns {number}
   */
  _getMaxArcRightX() {
    if (this._cachedMaxArcRightX != null) return this._cachedMaxArcRightX;
    const arcs = DomAdapter.querySelectorAll(Selectors.allArcPaths());
    let maxX = 0;
    for (const arc of arcs) {
      if (arc.style.display === 'none') continue;
      const bbox = arc.getBBox();
      maxX = Math.max(maxX, bbox.x + bbox.width);
    }
    this._cachedMaxArcRightX = maxX;
    return maxX;
  },

  /**
   * Invalidate cached layout values. Call after collapse/expand or relayout
   * so the next sidebar positioning recomputes arc extents.
   */
  invalidateLayout() {
    this._cachedMaxArcRightX = null;
    this._cachedX = null;
  },

  /**
   * Reset stored original viewBox dimensions. Call when the base SVG size
   * changes (e.g. after relayout resizes the viewport) so the sidebar
   * recaptures correct dimensions on next updatePosition().
   */
  resetStoredViewBox() {
    this._originalViewBoxHeight = null;
    this._originalViewBoxWidth = null;
  },

  /** Cached X position — set once in show(), reused by updatePosition(). @type {number | null} */
  _cachedX: null,
  /** Cached max arc right X — only changes on collapse/relayout, not on hover. @type {number | null} */
  _cachedMaxArcRightX: null,
  /** Original SVG viewBox height — stored to restore after sidebar close. @type {number | null} */
  _originalViewBoxHeight: null,
  /** Original SVG viewBox width — stored to restore after sidebar close. @type {number | null} */
  _originalViewBoxWidth: null,

  /**
   * Calculate sidebar x in SVG coordinates (right of widest visible arc).
   * @returns {number}
   */
  _calcX() {
    const svg = DomAdapter.getSvgRoot();
    if (!svg) return 0;
    const rect = svg.getBoundingClientRect();
    const viewBox = svg.viewBox.baseVal;
    const scaleX = viewBox.width / rect.width;

    const maxArcRight = this._getMaxArcRightX();
    let x = maxArcRight + SIDEBAR_GAP_X;

    const viewportRight = (visibleArea().width - rect.left) * scaleX;
    if (x + SIDEBAR_MIN_WIDTH > viewportRight) {
      x = viewportRight - SIDEBAR_MIN_WIDTH - SIDEBAR_MARGIN_RIGHT;
    }
    return Math.max(0, Math.round(x));
  },

  /**
   * Calculate sidebar y + height in SVG coordinates (tracks scroll).
   * @returns {{ y: number, height: number }|null}
   */
  _calcPosition() {
    const svg = DomAdapter.getSvgRoot();
    if (!svg) return null;
    const rect = svg.getBoundingClientRect();
    const viewBox = svg.viewBox.baseVal;
    const scaleY = viewBox.height / rect.height;

    const scrollTop = Math.max(0, -rect.top) * scaleY;
    const y = scrollTop + TOOLBAR_HEIGHT + SIDEBAR_GAP_TOP;
    const vpHeight = visibleArea().height * scaleY;

    return {
      y: Math.round(y),
      height: Math.round(vpHeight - TOOLBAR_HEIGHT - SIDEBAR_GAP_TOP),
    };
  },

  /**
   * Show sidebar transiently (hover preview). Debounced to prevent flicker.
   * No collapse handlers, adds sidebar-transient CSS class.
   * @param {string} arcId
   * @param {{ from: string, to: string, usages: StaticArcData["usages"], originalArcs?: string[], cycleIds?: number[] }} [overrideData]
   */
  showTransient(arcId, overrideData) {
    clearTimeout(this._debounceTimer ?? undefined);
    this._debounceTimer = setTimeout(() => {
      const el = this._getElement();
      if (!el) return;
      /** @type {HTMLElement|null} */
      const innerDiv = el.querySelector('.sidebar-root');
      if (innerDiv) {
        innerDiv.innerHTML = this.buildContent(arcId, overrideData);
        innerDiv.classList.add('sidebar-transient');
      }
      el.style.display = 'block';
      this._isTransient = true;
      this._cachedX = this._calcX();
      this.updatePosition();
    }, 30);
  },

  /**
   * Hide a transient sidebar. Does nothing if sidebar is pinned.
   */
  hideTransient() {
    clearTimeout(this._debounceTimer ?? undefined);
    if (!this._isTransient) return;
    this.hide();
    this._isTransient = false;
  },

  /**
   * Re-mark the focused cluster row in place (no rebuild). Used while an SCC is
   * selected: the cluster sidebar stays open and hover only moves the focus.
   */
  refreshClusterFocus() {
    const el = this._getElement();
    const content = el?.querySelector('.sidebar-content');
    if (content)
      this._refreshEdgeRowFocus(/** @type {HTMLElement} */ (content));
  },

  /**
   * Shared pinned-show logic: inject HTML, remove transient state, wire handlers, position.
   * @param {string} html - Pre-built sidebar HTML content
   */
  _showWithContent(html) {
    const el = this._getElement();
    if (!el) return;
    /** @type {HTMLElement|null} */
    const innerDiv = el.querySelector('.sidebar-root');
    if (innerDiv) {
      innerDiv.innerHTML = html;
      innerDiv.classList.remove('sidebar-transient');
      this._setupCollapseHandlers(innerDiv);
    }
    el.style.display = 'block';
    this._isTransient = false;
    clearTimeout(this._debounceTimer ?? undefined);
    this._cachedMaxArcRightX = null;
    this._cachedX = this._calcX();
    this.updatePosition();
  },

  /**
   * Show sidebar with content for given arc.
   * @param {string} arcId
   * @param {{ from: string, to: string, usages: StaticArcData["usages"], originalArcs?: string[], cycleIds?: number[] }} [overrideData]
   */
  show(arcId, overrideData) {
    this._showWithContent(this.buildContent(arcId, overrideData));
  },

  /**
   * Build cluster (SCC) sidebar: header + one collapsible block per
   * cycle, edges in path order.
   * @param {string} sccId - Key into STATIC_DATA.clusters
   * @param {string} [focusArcId] - Arc whose row should render as focused
   *   (the edge that triggered this view \u2014 the resolved click/hover focus).
   * @returns {string}
   */
  _buildClusterContent(sccId, focusArcId) {
    const cl = STATIC_DATA.clusters?.[sccId];
    if (!cl) return '';

    // Cache each edge's crossing symbols once; the hasCollapsible scan and
    // the row builder would otherwise both call _edgeSymbols per edge.
    const symbolsByArcId = new Map();
    for (const cycle of cl.cycles) {
      for (const edge of cycle) {
        const arcId = `${edge.fromId}-${edge.toId}`;
        if (!symbolsByArcId.has(arcId)) {
          symbolsByArcId.set(arcId, this._edgeSymbols(edge));
        }
      }
    }
    const hasCollapsible = [...symbolsByArcId.values()].some(
      (symbols) => symbols.length > 0,
    );

    let html = `<div class="sidebar-header">`;
    html += `<span class="sidebar-title">Cluster \u00b7 ${cl.crate}</span>`;
    if (hasCollapsible) {
      html += `<div class="sidebar-header-actions">`;
      html += `<button class="sidebar-collapse-all">+</button>`;
      html += `<button class="sidebar-close">&#x2715;</button>`;
      html += `</div>`;
    } else {
      html += `<button class="sidebar-close">&#x2715;</button>`;
    }
    html += `</div>`;

    html += `<div class="sidebar-subheader">${cl.moduleCount} modules \u00b7 ${cl.cycleCount} cycles</div>`;

    html += `<div class="sidebar-content">`;
    const sccNodeIds = new Set();
    cl.cycles.forEach((cycle) => {
      cycle.forEach((edge) => {
        sccNodeIds.add(edge.fromId);
        sccNodeIds.add(edge.toId);
      });
    });
    // Path segments and shortLabel results are the same for a node no
    // matter which cycle-block it appears in; compute each once and reuse
    // across blocks instead of re-splitting/re-resolving per occurrence.
    const segmentsById = new Map(
      [...sccNodeIds].map((id) => [
        id,
        StaticData.qualifiedParts(id).path.split('::'),
      ]),
    );
    const labelCache = new Map();
    const seenArcIds = new Set();
    cl.cycles.forEach((cycle, index) => {
      html += this._buildCycleBlock(
        cycle,
        index,
        focusArcId,
        seenArcIds,
        sccNodeIds,
        symbolsByArcId,
        segmentsById,
        labelCache,
      );
    });
    html += `</div>`;

    return html;
  },

  /**
   * One cycle block: collapsible header (ordinal, leaf-name path,
   * module count) plus one `_buildEdgeRow` per edge in path order. The last
   * edge closes the cycle and gets the closing class; an edge whose arc
   * id already appeared in an earlier block gets the repeat class \u2014 except
   * the closing edge, which always renders full.
   * @param {StaticCycleArcData[]} cycle - Edges in path order, closing edge last.
   * @param {number} index - 0-based cycle index (rendered as 1-based ordinal).
   * @param {string|undefined} focusArcId
   * @param {Set<string>} seenArcIds - Arc ids rendered in earlier blocks
   *   (mutated in place as this block's edges are rendered).
   * @param {Set<string>} sccNodeIds - All node ids in the SCC, for shortLabel.
   * @param {Map<string, Array>} symbolsByArcId - Precomputed _edgeSymbols per arc id.
   * @param {Map<string, string[]>} [segmentsById] - Precomputed shortLabel path
   *   segments per node id.
   * @param {Map<string, string>} [labelCache] - Memoized shortLabel results per
   *   node id, shared across cycle blocks.
   * @returns {string}
   */
  _buildCycleBlock(
    cycle,
    index,
    focusArcId,
    seenArcIds,
    sccNodeIds,
    symbolsByArcId,
    segmentsById,
    labelCache,
  ) {
    let html = `<details class="cycle-block">`;
    html += `<summary class="block-head">`;
    html += `<span class="block-chevron">\u25b8</span>`;
    html += `<span class="block-ordinal">${index + 1}</span>`;
    html += `<span class="block-path">${this._cyclePathLabel(cycle, sccNodeIds, segmentsById, labelCache)}</span>`;
    html += `<span class="block-module-count">${cycle.length} Module</span>`;
    html += `</summary>`;
    html += `<div class="cycle-block-body">`;
    cycle.forEach((edge, i) => {
      const arcId = `${edge.fromId}-${edge.toId}`;
      const isClosing = i === cycle.length - 1;
      const extraClasses = [];
      if (isClosing) {
        extraClasses.push('edge-closing');
      } else if (seenArcIds.has(arcId)) {
        extraClasses.push('edge-repeat');
      }
      html += this._buildEdgeRow(
        edge,
        symbolsByArcId.get(arcId) ?? [],
        focusArcId,
        extraClasses,
      );
      seenArcIds.add(arcId);
    });
    html += `</div>`;
    html += `</details>`;
    return html;
  },

  /**
   * Path label for a cycle block header: each node's shortest unique suffix
   * within the SCC (D6), in path order, elided via `elideCyclePath`.
   * @param {StaticCycleArcData[]} cycle
   * @param {Set<string>} sccNodeIds
   * @param {Map<string, string[]>} [segmentsById]
   * @param {Map<string, string>} [labelCache] - Memoized per node id (mutated
   *   in place); a node repeated across cycle blocks resolves its label once.
   * @returns {string}
   */
  _cyclePathLabel(cycle, sccNodeIds, segmentsById, labelCache) {
    const labels = cycle.map((edge) => {
      const cached = labelCache?.get(edge.fromId);
      if (cached !== undefined) return cached;
      const label = this.shortLabel(edge.fromId, sccNodeIds, segmentsById);
      labelCache?.set(edge.fromId, label);
      return label;
    });
    return this.elideCyclePath(labels);
  },

  /**
   * Shortest unique suffix of a node's crate-relative module path, computed
   * against every other node in the same SCC. Starts at the leaf and grows
   * left segment by segment until no other node in `sccNodeIds` shares the
   * same suffix. Never includes the crate prefix.
   * @param {string} nodeId
   * @param {Iterable<string>} sccNodeIds - All node ids in the SCC.
   * @param {Map<string, string[]>} [segmentsById] - Precomputed path segments
   *   per node id, to skip re-splitting on repeated calls.
   * @returns {string}
   */
  shortLabel(nodeId, sccNodeIds, segmentsById) {
    const pathSegments = (id) =>
      segmentsById?.get(id) ?? StaticData.qualifiedParts(id).path.split('::');
    const segments = pathSegments(nodeId);
    const otherSegments = [...sccNodeIds]
      .filter((id) => id !== nodeId)
      .map((id) => pathSegments(id));
    for (let len = 1; len <= segments.length; len++) {
      const suffix = segments.slice(-len).join('::');
      const collides = otherSegments.some(
        (other) => other.slice(-len).join('::') === suffix,
      );
      if (!collides) return suffix;
    }
    return segments.join('::');
  },

  /**
   * Header path label for a cycle: `labels` are the k distinct node labels
   * in path order (labels[0] is the start/sort anchor), closed back to
   * labels[0]. Shows the full closed path when k <= n; otherwise elides the
   * middle. The head, the second node, and the closing edge are kept, so the
   * shape follows the sequence and the loop-closer both stay visible; the
   * second node is dropped only when the label would exceed `maxChars`, a
   * degenerate width where keeping it would push the closing edge under the
   * CSS clip.
   * @param {string[]} labels
   * @param {number} [n]
   * @param {number} [maxChars]
   * @returns {string}
   */
  elideCyclePath(labels, n = 4, maxChars = CYCLE_HEADER_MAX_CHARS) {
    const head = labels[0];
    if (labels.length <= n) {
      return `${labels.join(' \u2192 ')} \u2192 ${head}`;
    }
    const arrow = ' \u2192 ';
    const tail = labels[labels.length - 1];
    const withSecond = `${head}${arrow}${labels[1]}${arrow}\u2026${arrow}${tail}${arrow}${head}`;
    if (withSecond.length <= maxChars) return withSecond;
    return `${head}${arrow}\u2026${arrow}${tail}${arrow}${head}`;
  },

  /**
   * Path spans under `root`, each with the separator its kind is cut on.
   * @param {Element} root
   * @returns {Array<{ span: HTMLElement, separator: string }>}
   */
  _pathSpans(root) {
    if (!root || !root.querySelectorAll) return [];
    const kinds = [
      { selector: '.sidebar-ns[data-full]', separator: '::' },
      { selector: '.sidebar-file[data-full]', separator: '/' },
    ];
    return kinds.flatMap(({ selector, separator }) =>
      [...root.querySelectorAll(selector)].map((span) => ({
        span: /** @type {HTMLElement} */ (span),
        separator,
      })),
    );
  },

  /**
   * Puts the full text back into every path span. Must run before the
   * sidebar measures its natural width, or the measurement sees the cut
   * text, the sidebar narrows, and the next pass cuts more.
   * @param {Element} root
   */
  resetPaths(root) {
    for (const { span } of this._pathSpans(root)) PathFit.resetPath(span);
  },

  /**
   * Fits every path span under `root` to its box (`PathFit.fitPath`). Runs
   * after the sidebar width is final; the reset happens again on every call,
   * so a span that has more room than last time gets its segments back.
   * @param {Element} root
   * @param {(span: HTMLElement) => boolean} [overflows] - defaults to the DOM
   *   overflow test; tests pass a text-length predicate instead.
   */
  fitPaths(root, overflows = (span) => span.scrollWidth > span.clientWidth) {
    for (const { span, separator } of this._pathSpans(root)) {
      PathFit.fitPath(span, separator, overflows);
    }
  },

  /**
   * Crossing symbols for a cluster edge, real imports only. `pub use`
   * re-exports ride the same edge but are not part of the cycle (ADR-022).
   * @param {StaticCycleArcData} edge
   * @returns {Array}
   */
  _edgeSymbols(edge) {
    const arcId = `${edge.fromId}-${edge.toId}`;
    return (
      (typeof STATIC_DATA !== 'undefined' &&
        STATIC_DATA.arcs?.[arcId]?.usages) ||
      []
    ).filter((u) => u.symbol && !u.viaReexport);
  },

  /**
   * One cluster-edge row: the edge (dependent \u2192 dependency, colour-framed
   * like the node view) plus its cycle/symbol meta. When the edge's crossing
   * symbols are known, the row expands to list them with their locality
   * tags.
   * @param {StaticCycleArcData} edge
   * @param {Array} symbols - Pre-computed crossing symbols for this edge.
   * @param {string|undefined} focusArcId - Arc whose row should render as focused.
   * @param {string[]} [extraClasses] - Extra row classes (e.g. edge-closing, edge-repeat).
   * @returns {string}
   */
  _buildEdgeRow(edge, symbols, focusArcId, extraClasses = []) {
    const arcId = `${edge.fromId}-${edge.toId}`;
    const fromName = StaticData.qualifiedParts(edge.fromId).path;
    const toName = StaticData.qualifiedParts(edge.toId).path;
    const fromType = StaticData.getNode(edge.fromId)?.type;
    const toType = StaticData.getNode(edge.toId)?.type;
    const fromClass = `sidebar-cycle-node ${fromType ? `sidebar-node-${fromType} ` : ''}sidebar-node-from`;
    const toClass = `sidebar-cycle-node ${toType ? `sidebar-node-${toType} ` : ''}sidebar-node-to`;
    const expandable = symbols.length > 0;
    const focusClass = arcId === focusArcId ? ' sidebar-edge-row-focus' : '';
    const extraClass = extraClasses.length ? ` ${extraClasses.join(' ')}` : '';
    const isClosing = extraClasses.includes('edge-closing');

    let html = `<div class="sidebar-usage-group sidebar-edge-row${extraClass}${focusClass}" data-arc-id="${arcId}">`;
    const headAttrs = expandable
      ? ' data-collapsible="" data-collapsed="true"'
      : ' style="cursor:default"';
    html += `<div class="sidebar-symbol sidebar-edge-head"${headAttrs}>`;
    html += `<span class="sidebar-toggle">${expandable ? '\u25b8' : ''}</span>`;
    html += `<div class="sidebar-cycle-edge">`;
    if (isClosing) {
      html += `<span class="sidebar-edge-closing-marker" title="closes the cycle">&#x21ba;</span>`;
    }
    html += `<span class="${fromClass}" data-node-id="${edge.fromId}" title="${fromName}">${fromName}</span>`;
    html += `<span class="sidebar-arrow">&#x2192;</span>`;
    html += `<span class="${toClass}" data-node-id="${edge.toId}" title="${toName}">${toName}</span>`;
    html += `</div>`;
    html += `<span class="sidebar-edge-meta">${symbols.length} symbols</span>`;
    html += `</div>`;

    if (expandable) {
      html += `<div class="sidebar-locations" style="display:none">`;
      for (const u of symbols) {
        html += `<div class="sidebar-edge-symbol" title="${this._symbolTitle(u)}">`;
        html += this._symbolLabel(u);
        html += this._definitionChip(u);
        html += this._renderLocalityTag(edge.toId, u.symbol);
        html += `</div>`;
        for (const loc of u.locations || []) {
          html += this._locationRow(loc);
        }
      }
      html += `</div>`;
    }
    html += `</div>`;
    return html;
  },

  /**
   * A cluster row was clicked: let the state machine couple pin and expansion
   * (AppState.clickClusterRow via _onEdgeClick), then sync the row's DOM, focus
   * marker and the collapse-all button in place (no sidebar rebuild).
   * @param {HTMLElement} row
   * @param {HTMLElement} root
   * @param {HTMLElement} content
   */
  _handleEdgeRowClick(row, root, content) {
    const arcId = row.dataset.arcId;
    if (!arcId) return;
    const head = row.querySelector('.sidebar-edge-head');
    const expandable = !!head && head.hasAttribute('data-collapsible');
    const expanded =
      expandable && head.getAttribute('data-collapsed') !== 'true';
    const endExpanded = this._onEdgeClick
      ? this._onEdgeClick(arcId, expandable, expanded)
      : false;
    if (head && expandable) this._setEdgeRowExpanded(head, endExpanded);
    this._refreshEdgeRowFocus(content);
    this._syncCollapseAllButton(root, content);
    this.updatePosition();
  },

  /**
   * Show or hide a cluster row's crossing-symbol list in place.
   * @param {Element} head - The row's `.sidebar-edge-head`
   * @param {boolean} expanded
   */
  _setEdgeRowExpanded(head, expanded) {
    const locsEl = /** @type {HTMLElement|null} */ (head.nextElementSibling);
    const toggle = head.querySelector('.sidebar-toggle');
    if (expanded) {
      head.removeAttribute('data-collapsed');
      if (locsEl) locsEl.style.display = '';
      if (toggle) toggle.innerHTML = '▾';
    } else {
      head.setAttribute('data-collapsed', 'true');
      if (locsEl) locsEl.style.display = 'none';
      if (toggle) toggle.innerHTML = '▸';
    }
  },

  /**
   * Collect the edge-row heads of a cluster sidebar that have symbols to show.
   * @param {HTMLElement} content
   * @returns {Element[]}
   */
  _expandableEdgeHeads(content) {
    return [
      ...content.querySelectorAll('.sidebar-edge-head[data-collapsible]'),
    ];
  },

  /**
   * Tell whether any cycle block or expandable edge row is still closed.
   * @param {Element[]} blocks
   * @param {Element[]} heads
   * @returns {boolean}
   */
  _anyClusterSectionClosed(blocks, heads) {
    return (
      blocks.some((b) => !b.hasAttribute('open')) ||
      heads.some((h) => h.getAttribute('data-collapsed') === 'true')
    );
  },

  /**
   * Open every collapsed section above the rows carrying the given jump ids,
   * so the editor's cursor line is in view in the pinned sidebar. Ids without
   * a row, and rows whose sections are already open, change nothing.
   * @param {number[]} jumpIds
   */
  expandLocations(jumpIds) {
    const root = this._getElement();
    const content = /** @type {HTMLElement|null} */ (
      root?.querySelector('.sidebar-content')
    );
    if (!root || !content) return;
    const wanted = new Set(jumpIds.map(String));
    const rows = [...content.querySelectorAll('[data-jump]')].filter((row) =>
      wanted.has(row.getAttribute('data-jump') ?? ''),
    );
    let opened = false;
    for (const row of rows) {
      // A row sits in a `.sidebar-locations` list whose head is the element
      // before it; lists nest, so walk up through every enclosing one.
      let list = row.closest('.sidebar-locations');
      while (list) {
        const head = list.previousElementSibling;
        if (
          head?.hasAttribute('data-collapsible') &&
          head.getAttribute('data-collapsed') === 'true'
        ) {
          this._setEdgeRowExpanded(head, true);
          opened = true;
        }
        const parent = /** @type {Element|null} */ (list.parentNode);
        list = parent?.closest('.sidebar-locations') ?? null;
      }
    }
    if (!opened) return;
    this._syncCollapseAllButton(root, content);
    this.updatePosition();
  },

  /**
   * Mark the row of the resolved focus edge, clearing the others. Keeps the
   * sidebar in sync with the graph without a rebuild.
   * @param {HTMLElement} content
   */
  _refreshEdgeRowFocus(content) {
    if (!content.querySelectorAll) return;
    const focusArc = this._resolvedFocusArc ? this._resolvedFocusArc() : null;
    for (const el of content.querySelectorAll('.sidebar-edge-row')) {
      const row = /** @type {HTMLElement} */ (el);
      row.classList?.toggle(
        'sidebar-edge-row-focus',
        row.dataset.arcId === focusArc,
      );
    }
  },

  /**
   * Refresh the collapse-all button glyph from the rows' collapsed state.
   * @param {HTMLElement} root
   * @param {HTMLElement} content
   */
  _syncCollapseAllButton(root, content) {
    const allBtn = root.querySelector?.('.sidebar-collapse-all');
    if (!allBtn || !content.querySelectorAll) return;
    // Cycle blocks exist only in cluster view.
    const blocks = [...content.querySelectorAll('.cycle-block')];
    if (blocks.length) {
      const anyClosed = this._anyClusterSectionClosed(
        blocks,
        this._expandableEdgeHeads(content),
      );
      allBtn.innerHTML = anyClosed ? '+' : '−';
      return;
    }
    const heads = [...content.querySelectorAll(COLLAPSIBLE_SYMBOL_SELECTOR)];
    const allCollapsed = heads.every(
      (s) => s.getAttribute('data-collapsed') === 'true',
    );
    allBtn.innerHTML = allCollapsed ? '+' : '−';
  },

  _setupCollapseHandlers(root) {
    if (!root || !root.querySelector) return;
    const content = root.querySelector('.sidebar-content');
    if (!content) return;
    content.addEventListener('click', (e) => {
      // A location row's or definition chip's click is a jump, handled on
      // the document; the row around it keeps its pin and expansion state.
      if (e.target.closest?.('.sidebar-location[data-jump]')) return;
      if (e.target.closest?.('.sidebar-definition[data-jump]')) return;
      // Cluster rows couple pin and expansion; the state machine decides.
      const edgeRow = e.target.closest?.('.sidebar-edge-row');
      if (edgeRow) {
        SidebarLogic._handleEdgeRowClick(edgeRow, root, content);
        return;
      }
      const symbolEl = e.target.closest('.sidebar-symbol');
      if (!symbolEl) return;
      if (!symbolEl.hasAttribute('data-collapsible')) return;
      const locsEl = symbolEl.nextElementSibling;
      const isCollapsed = symbolEl.getAttribute('data-collapsed') === 'true';
      if (isCollapsed) {
        symbolEl.removeAttribute('data-collapsed');
        locsEl.style.display = '';
        const toggle = symbolEl.querySelector('.sidebar-toggle');
        if (toggle) toggle.innerHTML = '\u25BE';
      } else {
        symbolEl.setAttribute('data-collapsed', 'true');
        locsEl.style.display = 'none';
        const toggle = symbolEl.querySelector('.sidebar-toggle');
        if (toggle) toggle.innerHTML = '\u25B8';
      }
      SidebarLogic._syncCollapseAllButton(root, content);
      SidebarLogic.updatePosition();
    });
    if (root.querySelectorAll) {
      const indicators = root.querySelectorAll('.sidebar-collapse-indicator');
      for (const indicator of indicators) {
        indicator.addEventListener('click', (e) => {
          e.stopPropagation();
          if (SidebarLogic._onCollapseToggle) {
            SidebarLogic._onCollapseToggle(indicator.dataset.collapseTarget);
          }
        });
      }
      const badges = root.querySelectorAll('[data-node-id]');
      for (const badge of badges) {
        badge.addEventListener('click', (e) => {
          e.stopPropagation();
          if (SidebarLogic._onBadgeClick) {
            SidebarLogic._onBadgeClick(badge.dataset.nodeId);
          }
        });
      }
      // Row hover transiently focuses the graph edge; the row click (wired
      // above) pins it and drives expansion. Hover changes neither.
      const edgeRows = root.querySelectorAll('.sidebar-edge-row');
      for (const row of edgeRows) {
        row.addEventListener('mouseenter', () => {
          if (SidebarLogic._onEdgeHover) {
            SidebarLogic._onEdgeHover(row.dataset.arcId);
          }
        });
        row.addEventListener('mouseleave', () => {
          if (SidebarLogic._onEdgeHoverEnd) {
            SidebarLogic._onEdgeHoverEnd();
          }
        });
      }
      // A cycle block's native toggle changes how many blocks are open, so keep
      // the collapse-all glyph in sync with a manual open/close.
      for (const block of root.querySelectorAll('.cycle-block')) {
        block.addEventListener('toggle', () => {
          SidebarLogic._syncCollapseAllButton(root, content);
          SidebarLogic.updatePosition();
        });
      }
    }
    const collapseAllBtn = root.querySelector('.sidebar-collapse-all');
    if (!collapseAllBtn) return;
    collapseAllBtn.addEventListener('click', () => {
      const blocks = [...content.querySelectorAll('.cycle-block')];
      if (blocks.length) {
        const heads = SidebarLogic._expandableEdgeHeads(content);
        const anyClosed = SidebarLogic._anyClusterSectionClosed(blocks, heads);
        for (const b of blocks) {
          if (anyClosed) b.setAttribute('open', '');
          else b.removeAttribute('open');
        }
        // Rows open without going through AppState.clickClusterRow. The pin
        // holds one arc, so clicking every row would pin only the last one.
        for (const head of heads) {
          SidebarLogic._setEdgeRowExpanded(head, anyClosed);
        }
        SidebarLogic._syncCollapseAllButton(root, content);
        SidebarLogic.updatePosition();
        return;
      }
      const symbols = [
        ...content.querySelectorAll(COLLAPSIBLE_SYMBOL_SELECTOR),
      ];
      if (!symbols.length) return;
      const anyExpanded = symbols.some(
        (s) => s.getAttribute('data-collapsed') !== 'true',
      );
      for (const symbolEl of symbols) {
        const locsEl = symbolEl.nextElementSibling;
        const toggle = symbolEl.querySelector('.sidebar-toggle');
        if (anyExpanded) {
          symbolEl.setAttribute('data-collapsed', 'true');
          locsEl.style.display = 'none';
          if (toggle) toggle.innerHTML = '\u25B8';
        } else {
          symbolEl.removeAttribute('data-collapsed');
          locsEl.style.display = '';
          if (toggle) toggle.innerHTML = '\u25BE';
        }
      }
      collapseAllBtn.innerHTML = anyExpanded ? '+' : '\u2212';
      SidebarLogic.updatePosition();
    });
  },

  /**
   * Show sidebar with content for given node (pinned click).
   * @param {string} nodeId
   * @param {{ incoming: Array, outgoing: Array }} relations
   */
  showNode(nodeId, relations) {
    this._showWithContent(this.buildNodeContent(nodeId, relations));
  },

  /**
   * Show sidebar transiently for node hover. Debounced.
   * @param {string} nodeId
   * @param {{ incoming: Array, outgoing: Array }} relations
   */
  showTransientNode(nodeId, relations) {
    clearTimeout(this._debounceTimer ?? undefined);
    this._debounceTimer = setTimeout(() => {
      const el = this._getElement();
      if (!el) return;
      /** @type {HTMLElement|null} */
      const innerDiv = el.querySelector('.sidebar-root');
      if (innerDiv) {
        innerDiv.innerHTML = this.buildNodeContent(nodeId, relations);
        innerDiv.classList.add('sidebar-transient');
      }
      el.style.display = 'block';
      this._isTransient = true;
      this._cachedX = this._calcX();
      this.updatePosition();
    }, 30);
  },

  /**
   * Hide the sidebar.
   */
  hide() {
    const el = this._getElement();
    if (!el) return;
    el.style.display = 'none';
    this._cachedX = null;
    this._cachedMaxArcRightX = null;
    this._isTransient = false;
    clearTimeout(this._debounceTimer ?? undefined);

    // Restore original SVG canvas dimensions
    if (
      this._originalViewBoxHeight !== null ||
      this._originalViewBoxWidth !== null
    ) {
      const svg = DomAdapter.getSvgRoot();
      if (svg) {
        if (this._originalViewBoxHeight !== null) {
          svg.viewBox.baseVal.height = this._originalViewBoxHeight;
          svg.setAttribute('height', String(this._originalViewBoxHeight));
        }
        if (this._originalViewBoxWidth !== null) {
          svg.viewBox.baseVal.width = this._originalViewBoxWidth;
          svg.setAttribute('width', String(this._originalViewBoxWidth));
        }
      }
      this._originalViewBoxHeight = null;
      this._originalViewBoxWidth = null;
    }
  },

  /**
   * Check if sidebar is currently visible.
   * @returns {boolean}
   */
  isVisible() {
    const el = this._getElement();
    if (!el) return false;
    return el.style.display === 'block';
  },

  /**
   * Update sidebar position (x + y) based on current scroll and viewport.
   */
  updatePosition() {
    const el = this._getElement();
    if (!el) return;
    const pos = this._calcPosition();
    if (!pos) return;

    // Dynamic width: expand foreignObject first, shrink-wrap .sidebar-root with
    // max-content to measure the natural content width, then clamp to bounds.
    // Previous approach (shrink → measure scrollWidth) failed because nested
    // overflow containers (sidebar-content has implicit overflow-x:auto) don't
    // propagate scrollWidth reliably in foreignObject context.
    /** @type {HTMLElement|null} */
    const innerDiv = el.querySelector('.sidebar-root');
    if (innerDiv) this.resetPaths(innerDiv);
    el.setAttribute('width', '9999');
    if (innerDiv) innerDiv.style.width = 'max-content';
    const naturalW = innerDiv ? innerDiv.offsetWidth : 0;
    if (innerDiv) innerDiv.style.width = '';

    // Measure natural content height (analogous to width measurement above).
    // Must happen after width is finalized — width affects text wrap → height.
    if (innerDiv) innerDiv.style.height = 'auto';
    const naturalH = innerDiv ? innerDiv.offsetHeight : 0;
    const effectiveH =
      naturalH > 0 ? Math.min(naturalH, pos.height) : pos.height;

    const vpWidth = visibleArea().width;
    const width = Math.max(
      SIDEBAR_MIN_WIDTH,
      Math.min(naturalW, vpWidth * 0.5),
    );

    // Re-clamp X with actual width — _calcX() clamps with SIDEBAR_MIN_WIDTH
    // but actual width can be larger, pushing the sidebar beyond viewport
    let x = this._cachedX != null ? this._cachedX : this._calcX();
    const svg = DomAdapter.getSvgRoot();
    if (svg) {
      const svgRect = svg.getBoundingClientRect();
      const vb = svg.viewBox.baseVal;
      const scaleX = vb.width / svgRect.width;
      const viewportRight = (vpWidth - svgRect.left) * scaleX;
      if (x + width + SIDEBAR_MARGIN_RIGHT > viewportRight) {
        x = Math.max(
          0,
          Math.round(viewportRight - width - SIDEBAR_MARGIN_RIGHT),
        );
      }
    }
    x = Math.round(x);

    el.setAttribute('width', String(Math.round(width) + SIDEBAR_SHADOW_PAD));
    el.setAttribute('x', String(x));
    el.setAttribute('y', String(pos.y));
    el.setAttribute('height', String(effectiveH + SIDEBAR_SHADOW_PAD));
    if (innerDiv) {
      innerDiv.style.height = `${effectiveH}px`;
      this.fitPaths(innerDiv);
    }

    // Expand SVG canvas if sidebar extends beyond viewBox
    if (svg) {
      const vb = svg.viewBox.baseVal;
      const originalH = this._originalViewBoxHeight ?? vb.height;
      if (this._originalViewBoxHeight === null) {
        this._originalViewBoxHeight = vb.height;
      }
      const sidebarBottom = pos.y + effectiveH + SIDEBAR_SHADOW_PAD;
      const neededH = Math.max(originalH, sidebarBottom);
      if (vb.height !== neededH) {
        vb.height = neededH;
        svg.setAttribute('height', String(neededH));
      }

      // Also expand width when sidebar extends beyond viewBox
      const originalW = this._originalViewBoxWidth ?? vb.width;
      if (this._originalViewBoxWidth === null) {
        this._originalViewBoxWidth = vb.width;
      }
      const sidebarRight = x + Math.round(width) + SIDEBAR_SHADOW_PAD;
      const neededW = Math.max(originalW, sidebarRight);
      if (vb.width !== neededW) {
        vb.width = neededW;
        svg.setAttribute('width', String(neededW));
      }
    }
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { SidebarLogic };
}
