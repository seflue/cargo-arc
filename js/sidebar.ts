import { CanvasSize } from './canvas_size.js';
import { DomAdapter, numberAttribute } from './dom_adapter.js';
import { PathFit } from './path_fit.js';
import { Selectors } from './selectors.js';
import { StaticData } from './static_data.js';

// sidebar.ts - Relation sidebar for arc usage details
// Shows usage locations when an arc is selected (pinned)
// HTML sidebar placed through one of two hosts: a foreignObject in the SVG
// (follows the scroll, grows the canvas) or a fixed element in the page
// (docked to the viewport's right edge below the toolbar)

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

type NodeRelations = ReturnType<typeof StaticData.getNodeRelations>;
// The rows the collapse-all button drives: the relation rows of a node
// sidebar and the symbol rows of an arc sidebar, both direct children of
// the content's usage groups. A node sidebar nests a second level of
// collapsible symbol rows inside each relation row; those start expanded and
// must not count, or the first click would collapse instead of expand.
const collapsibleSymbolSelector = (S: typeof STATIC_DATA.classes.sidebar) =>
  `:scope > .${S.usageGroup} > .${S.symbol}[data-collapsible]`;

const SidebarLogic = {
  _isTransient: false,
  _debounceTimer: null as number | null,
  _onBadgeClick: null as ((nodeId: string | undefined) => void) | null,
  _onCollapseToggle: null as ((target: string | undefined) => void) | null,
  _isNodeCollapsed: null as ((id: string) => boolean) | null,
  /**
   * Merge symbol groups: combine groups with same symbol, deduplicate locations by file+line.
   */
  mergeSymbolGroups(
    groups: Array<{
      symbol: string;
      modulePath: string | null;
      locations: Array<{ file: string; line: number }>;
      definition?: { file: string; line: number; jump: number };
    }>,
  ): Array<{
    symbol: string;
    modulePath: string | null;
    locations: Array<{ file: string; line: number }>;
    definition?: { file: string; line: number; jump: number };
  }> {
    const bySymbol = new Map<string, (typeof groups)[number]>();
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
   */
  buildContent(
    arcId: string,
    overrideData?: {
      from: string;
      to: string;
      usages: ArcData['usages'];
      originalArcs?: string[];
      cycleIds?: number[];
    },
  ): string {
    const S = STATIC_DATA.classes.sidebar;
    const arc = overrideData || STATIC_DATA.arcs[arcId];
    if (!arc) return '';

    const groups = arc.usages || [];

    const fromNode = StaticData.getNode(arc.from);
    const toNode = StaticData.getNode(arc.to);
    const fromName = this._formatNodeName(fromNode, arc.from);
    const toName = this._formatNodeName(toNode, arc.to);
    const fromClass = `${fromNode ? `sidebar-node-${fromNode.type} ` : ''}${S.nodeFrom}`;
    const toClass = `${toNode ? `sidebar-node-${toNode.type} ` : ''}${S.nodeTo}`;

    let html = `<div class="${S.header}">`;
    html += `<span class="${S.title}"><span class="${fromClass}" data-node-id="${arc.from}">${fromName}${this._renderCollapseIndicator(arc.from)}</span><span class="${S.arrow}">&#x2192;</span><span class="${toClass}" data-node-id="${arc.to}">${toName}${this._renderCollapseIndicator(arc.to)}</span></span>`;
    const hasSymbols = groups.some((g) => g.symbol);
    if (hasSymbols) {
      html += `<div class="${S.headerActions}">`;
      html += `<button class="${S.collapseAll}" title="Collapse all">&#x2212;</button>`;
      html += `<button class="${S.close}">&#x2715;</button>`;
      html += `</div>`;
    } else {
      html += `<button class="${S.close}">&#x2715;</button>`;
    }
    html += `</div>`;

    html += `<div class="${S.content}">`;
    const traffic = overrideData ? 0 : (STATIC_DATA.arcs[arcId]?.traffic ?? 0);
    if (traffic > 0) {
      const noun = traffic === 1 ? 'cycle' : 'cycles';
      const upward = STATIC_DATA.arcs[arcId]?.upward ? ' · upward edge' : '';
      html += `<div class="${S.cycleLine}">on ${traffic} ${noun}${upward}</div>`;
    }
    if (groups.length === 0) {
      html += `<div class="${S.usageGroup}">${STATIC_DATA.wording.manifest} dependency</div>`;
    } else {
      const sorted = [...groups].sort(
        (a, b) => b.locations.length - a.locations.length,
      );
      for (const group of sorted) {
        html += `<div class="${S.usageGroup}">`;
        if (group.symbol) {
          html += `<div class="${S.symbol}" data-collapsible="" title="${this._symbolTitle(group)}">`;
          html += `<span class="${S.toggle}">&#x25BE;</span>`;
          html += this._symbolLabel(group);
          html += this._definitionChip(group);
          html += this._renderLocalityTag(arc.to, group.symbol);
          html += `<span class="${S.refCount}">${group.locations.length}</span>`;
          html += `</div>`;
        }
        html += `<div class="${S.locations}">`;
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
        ? `${STATIC_DATA.wording.manifest} dependency`
        : `${totalLocs} References \u00b7 ${symbolCount} Symbols`;
    if (overrideData?.originalArcs?.length) {
      footerText += ` \u00b7 ${overrideData.originalArcs.length} hidden Dependencies`;
    }
    html += `<div class="${S.footer}">${footerText}</div>`;

    return html;
  },

  /**
   * Build HTML content string for node-mode sidebar.
   * Shows all incoming (dependents) and outgoing (dependencies) relations.
   * @param nodeId - The selected node ID
   * @param relations - From collectNodeRelations() (filtered + virtual arcs)
   */
  buildNodeContent(nodeId: string, relations: NodeRelations): string {
    const S = STATIC_DATA.classes.sidebar;
    const node = StaticData.getNode(nodeId);
    const nodeName = this._formatNodeName(node, nodeId);
    const nodeType = node ? node.type : '';
    const hasRelations =
      relations.incoming.length > 0 || relations.outgoing.length > 0;
    const hasCollapsible = [...relations.incoming, ...relations.outgoing].some(
      (rel) => (rel.usages || []).length > 0,
    );

    // Header: Node name + Collapse-All ("+", since all L1 start collapsed) + Close
    let html = `<div class="${S.header}">`;
    html += `<span class="${S.title}"><span class="sidebar-node-${nodeType} ${S.nodeSelected}" data-node-id="${nodeId}">${nodeName}${this._renderCollapseIndicator(nodeId)}</span></span>`;
    if (hasCollapsible) {
      html += `<div class="${S.headerActions}">`;
      html += `<button class="${S.collapseAll}">+</button>`;
      html += `<button class="${S.close}">&#x2715;</button>`;
      html += `</div>`;
    } else {
      html += `<button class="${S.close}">&#x2715;</button>`;
    }
    html += `</div>`;

    html += `<div class="${S.content}">`;

    if (!hasRelations) {
      html += `<div class="${S.usageGroup}">No relations</div>`;
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
        html += `<hr class="${S.divider}"/>`;
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
    html += `<div class="${S.footer}">${relations.incoming.length} Dependents \u00b7 ${relations.outgoing.length} Dependencies</div>`;

    return html;
  },

  /**
   * Build a single Level-1 relation section (collapsed) with nested Level-2 usages.
   * @param rel - Relation entry {targetId, weight, usages, arcId}
   * @param nodeId - Selected node ID
   * @param nodeName - Selected node display name
   * @param nodeType - Selected node type (package/module)
   * @param [maxFromLen=0] - Minimum badge width (ch) for from-badge
   * @param [maxToLen=0] - Minimum badge width (ch) for to-badge
   */
  _buildRelationSection(
    rel: NodeRelations['incoming'][number],
    nodeId: string,
    nodeName: string,
    nodeType: string,
    direction: 'incoming' | 'outgoing',
    maxFromLen: number = 0,
    maxToLen: number = 0,
  ): string {
    const S = STATIC_DATA.classes.sidebar;
    const target = StaticData.getNode(rel.targetId);
    const targetName = this._formatNodeName(target, rel.targetId);
    const targetType = target ? target.type : '';

    // Build From→To pair: direction determines which side the selected node is on
    let fromName: string,
      fromType: string,
      fromSelected: boolean,
      fromId: string,
      toName: string,
      toType: string,
      toSelected: boolean,
      toId: string;
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

    const fromClass = `sidebar-node-${fromType}${fromSelected ? ` ${S.nodeSelected}` : ` ${S.nodeFrom}`}`;
    const toClass = `sidebar-node-${toType}${toSelected ? ` ${S.nodeSelected}` : ` ${S.nodeTo}`}`;
    const fromStyle =
      maxFromLen > 0 ? ` style="min-width: ${maxFromLen}ch"` : '';
    const toStyle = maxToLen > 0 ? ` style="min-width: ${maxToLen}ch"` : '';

    const groups = rel.usages || [];
    let html = `<div class="${S.usageGroup}">`;

    // External dependency without source references: flat row, no expand
    if (groups.length === 0) {
      html += `<div class="${S.symbol}" style="cursor:default">`;
      html += `<span class="${S.toggle}"></span>`;
      html += `<span class="${fromClass} ${S.symbolName}"${fromStyle} data-node-id="${fromId}">${fromName}${this._renderCollapseIndicator(fromId)}</span>`;
      html += `<span class="${S.arrow}">&#x2192;</span>`;
      html += `<span class="${toClass} ${S.symbolName}"${toStyle} data-node-id="${toId}">${toName}${this._renderCollapseIndicator(toId)}</span>`;
      const words = STATIC_DATA.wording;
      html += `<span class="${S.extInfo}" title="${words.manifest} dependency &#8212; source references are not tracked for external ${words.packages}">i</span>`;
      html += `</div>`;
      html += `</div>`;
      return html;
    }

    // Level 1 header (collapsed)
    html += `<div class="${S.symbol}" data-collapsible="" data-collapsed="true">`;
    html += `<span class="${S.toggle}">&#x25B8;</span>`;
    html += `<span class="${fromClass} ${S.symbolName}"${fromStyle} data-node-id="${fromId}">${fromName}${this._renderCollapseIndicator(fromId)}</span>`;
    html += `<span class="${S.arrow}">&#x2192;</span>`;
    html += `<span class="${toClass} ${S.symbolName}"${toStyle} data-node-id="${toId}">${toName}${this._renderCollapseIndicator(toId)}</span>`;
    html += `<span class="${S.refCount}">${rel.weight}</span>`;
    html += `</div>`;

    // Level 2 content (hidden because L1 is collapsed)
    html += `<div class="${S.locations}" style="display:none">`;
    const sorted = [...groups].sort(
      (a, b) => b.locations.length - a.locations.length,
    );
    for (const group of sorted) {
      html += `<div class="${S.usageGroup}">`;
      if (group.symbol) {
        html += `<div class="${S.symbol}" data-collapsible="" title="${this._symbolTitle(group)}">`;
        html += `<span class="${S.toggle}">&#x25BE;</span>`;
        html += this._symbolLabel(group);
        html += this._definitionChip(group);
        html += `<span class="${S.refCount}">${group.locations.length}</span>`;
        html += `</div>`;
      }
      html += `<div class="${S.locations}">`;
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
   * @param nodeId - ID of the selected node
   * @param nodeName - Display name of the selected node
   */
  _computeMaxBadgeLengths(
    relations: NodeRelations,
    nodeId: string,
    nodeName: string,
  ): {
    incoming: { maxFrom: number; maxTo: number };
    outgoing: { maxFrom: number; maxTo: number };
  } {
    const effectiveLen = (id: string, name: string) => {
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
   * Format node display name, appending version for external packages.
   * @param node - Node data from StaticData
   * @param fallback - Fallback ID if node is null
   */
  _formatNodeName(node: NodeData | undefined, fallback: string): string {
    if (!node) return fallback;
    if (node.version) return `${node.name} v${node.version}`;
    return node.name;
  },

  /**
   * Render a +/- collapse indicator span for a node badge.
   * Returns empty string for leaf nodes or when state callback is not set.
   */
  _renderCollapseIndicator(nodeId: string): string {
    const S = STATIC_DATA.classes.sidebar;
    if (!StaticData.hasChildren(nodeId)) return '';
    const collapsed = this._isNodeCollapsed?.(nodeId);
    if (collapsed === undefined || collapsed === null) return '';
    const symbol = collapsed ? '+' : '\u2212';
    return `<span class="${S.collapseIndicator}" data-collapse-target="${nodeId}">${symbol}</span>`;
  },

  /**
   * Consumer-locality tag for one symbol of a provider. Empty unless
   * STATIC_DATA.symbolLocalities carries a locality for (providerId, symbol).
   * The tag states a descriptive fact about where the symbol is consumed, not a
   * verdict on whether to move it.
   * @param providerId - The provider node id (the arc's `to`).
   */
  _renderLocalityTag(providerId: string, symbol: string): string {
    const S = STATIC_DATA.classes.sidebar;
    const sl =
      typeof STATIC_DATA !== 'undefined' &&
      STATIC_DATA.symbolLocalities?.[providerId]?.[symbol];
    if (!sl) return '';
    const nameOf = (id: string) => StaticData.getNode(id)?.name ?? id;
    let label: string;
    if (sl.locality === 'singleConsumer') {
      label = `only used by ${nameOf(sl.module ?? '')}`;
    } else if (sl.locality === 'commonAncestor') {
      label = `used under ${nameOf(sl.module ?? '')}`;
    } else if (sl.locality === 'packageWide') {
      label = `widely used (${sl.consumers?.length ?? 0} modules)`;
    } else {
      return '';
    }
    return `<span class="${S.locality} sidebar-locality-${sl.locality}">${label}</span>`;
  },

  /**
   * The jump chip on a symbol row, opening the symbol's definition. Empty
   * unless the group carries a definition (RenderConfig::with_jump_ids and a
   * definer the analysis could see). Hidden until the row is hovered, like
   * the location rows' icon; the click is a jump handled on the document.
   */
  _definitionChip(group: {
    definition?: { file: string; line: number; jump: number };
  }): string {
    const S = STATIC_DATA.classes.sidebar;
    const definition = group.definition;
    if (!definition) return '';
    return (
      `<span class="${S.definition}" data-jump="${definition.jump}" title="${definition.file}:${definition.line}">` +
      `<svg class="${S.jump}" xmlns="http://www.w3.org/2000/svg"><use href="#jump-icon"></use></svg></span>`
    );
  },

  /**
   * Full `modulePath::symbol` for the row's `title`.
   */
  _symbolTitle(group: { symbol: string; modulePath?: string | null }): string {
    return group.modulePath
      ? `${group.modulePath}::${group.symbol}`
      : group.symbol;
  },

  /**
   * Module path and symbol name spans. The path span carries its full text
   * in `data-full` because `fitPaths` rewrites the visible text.
   */
  _symbolLabel(group: { symbol: string; modulePath?: string | null }): string {
    const S = STATIC_DATA.classes.sidebar;
    let html = '';
    if (group.modulePath) {
      const ns = `${group.modulePath}::`;
      html += `<span class="${S.ns}" data-full="${ns}">${ns}</span>`;
    }
    html += `<span class="${S.symbolName}">${group.symbol}</span>`;
    return html;
  },

  /**
   * One usage-location row. Carries `data-jump` only when the location has a
   * jump id (RenderConfig::with_jump_ids, i.e. `arc ui`); an image from
   * `cargo arc -o` never sets it, so the row stays inert there.
   */
  _locationRow(loc: { file: string; line: number; jump?: number }): string {
    const S = STATIC_DATA.classes.sidebar;
    const hasJump = typeof loc.jump === 'number';
    const jumpAttr = hasJump ? ` data-jump="${loc.jump}"` : '';
    // Same symbol the node icons use; JumpIcons defines it in the SVG root
    // at page load, so the <use> resolves whenever a jump id exists. The
    // xmlns is required: this HTML lands in an XML document via innerHTML,
    // where an unprefixed <svg> would stay in the XHTML namespace.
    const jumpIcon = hasJump
      ? `<svg class="${S.jump}" xmlns="http://www.w3.org/2000/svg"><use href="#jump-icon"></use></svg>`
      : '';
    return (
      `<div class="${S.location}"${jumpAttr} title="${loc.file}:${loc.line}">` +
      `<span class="${S.file}" data-full="${loc.file}">${loc.file}</span>` +
      `<span class="${S.lineBadge}">:${loc.line}</span>${jumpIcon}</div>`
    );
  },

  /**
   * Get the foreignObject element for the sidebar.
   */
  _getElement(): HTMLElement | null {
    return DomAdapter.getElementById('relation-sidebar');
  },

  /**
   * Find the rightmost X coordinate among all visible arc paths.
   * Uses cached value when available — cache is invalidated by
   * invalidateLayout() (collapse/relayout) and hide().
   */
  _getMaxArcRightX(): number {
    if (this._cachedMaxArcRightX != null) return this._cachedMaxArcRightX;
    const arcs = DomAdapter.querySelectorAll<SVGGraphicsElement>(
      Selectors.allArcPaths(),
    );
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
   * Find the right edge, in SVG coordinates, of the widest module label that
   * is neither hidden by a filter nor inside a collapsed parent. All such
   * modules count, not only the rows on screen, so the value does not change
   * while scrolling vertically. Cached like _getMaxArcRightX.
   */
  getMaxLabelRightX(): number {
    if (this._cachedMaxLabelRightX != null) return this._cachedMaxLabelRightX;
    const C = STATIC_DATA.classes;
    let maxX = 0;
    for (const nodeId of StaticData.getAllNodeIds()) {
      const node = DomAdapter.getNode(nodeId);
      if (!node) continue;
      if (node.classList.contains(C.nodes.collapsed)) continue;
      if (node.classList.contains(C.labels.hiddenByFilter)) continue;
      const right = numberAttribute(node, 'x') + numberAttribute(node, 'width');
      if (!Number.isNaN(right)) maxX = Math.max(maxX, right);
    }
    this._cachedMaxLabelRightX = maxX;
    return maxX;
  },

  /**
   * The widest label's right edge in viewport pixels. Converted on every call
   * because horizontal scrolling moves the SVG under the viewport.
   */
  _labelRightOnPage(): number {
    const svg = DomAdapter.getSvgRoot();
    if (!svg) return 0;
    const rect = svg.getBoundingClientRect();
    this._pageSvgLeft = rect.left;
    const scaleX = svg.viewBox.baseVal.width / rect.width;
    return rect.left + this.getMaxLabelRightX() / scaleX;
  },

  /** The SVG's left edge in viewport pixels when the page host last took its width. */
  _pageSvgLeft: null as number | null,

  /**
   * The narrowest canvas width that leaves a minimum-width sidebar right of
   * the labels when scrolled fully right.
   */
  minCanvasWidth(): number {
    return CanvasSize.minSvgWidth(
      this.getMaxLabelRightX(),
      SIDEBAR_GAP_X,
      SIDEBAR_MIN_WIDTH,
      SIDEBAR_MARGIN_RIGHT,
    );
  },

  /**
   * Invalidate cached layout values. Call after collapse/expand or relayout
   * so the next sidebar positioning recomputes arc extents.
   */
  invalidateLayout() {
    this._cachedMaxArcRightX = null;
    this._cachedMaxLabelRightX = null;
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

  /** The toolbar's measured height; more than TOOLBAR_HEIGHT once it wraps. */
  _toolbarHeight: TOOLBAR_HEIGHT,

  /**
   * Record the toolbar's measured height; the sidebar starts below it.
   */
  setToolbarHeight(height: number) {
    this._toolbarHeight = height;
  },

  /** Which element hosts the sidebar; see useHost(). */
  _host: 'svg' as 'svg' | 'page',
  /** Cached X position — set once in show(), reused by updatePosition(). */
  _cachedX: null as number | null,
  /** Cached max arc right X — only changes on collapse/relayout, not on hover. */
  _cachedMaxArcRightX: null as number | null,
  /** Cached widest label right X — only changes on collapse/relayout/filter. */
  _cachedMaxLabelRightX: null as number | null,
  /** Original SVG viewBox height — stored to restore after sidebar close. */
  _originalViewBoxHeight: null as number | null,
  /** Original SVG viewBox width — stored to restore after sidebar close. */
  _originalViewBoxWidth: null as number | null,
  /** Sidebar height from the last updatePosition(), reused by followScroll(). */
  _effectiveHeight: null as number | null,

  /**
   * Choose where the sidebar element lives: 'svg' (a foreignObject, placed in
   * SVG coordinates) or 'page' (a fixed element, placed in viewport pixels).
   */
  useHost(host: 'svg' | 'page') {
    this._host = host;
  },

  /**
   * The x the sidebar element is anchored at. Only the SVG host has one: it
   * sits right of the widest arc. `_placeOnPage` docks the page host at the
   * right edge, so it never measures arcs.
   */
  _calcHostX(): number {
    return this._host === 'page' ? 0 : this._calcX();
  },

  /**
   * Calculate sidebar x in SVG coordinates (right of widest visible arc).
   */
  _calcX(): number {
    const svg = DomAdapter.getSvgRoot();
    if (!svg) return 0;
    const rect = svg.getBoundingClientRect();
    const viewBox = svg.viewBox.baseVal;
    const scaleX = viewBox.width / rect.width;

    const maxArcRight = this._getMaxArcRightX();
    let x = maxArcRight + SIDEBAR_GAP_X;

    const viewportRight = (CanvasSize.visibleArea().width - rect.left) * scaleX;
    if (x + SIDEBAR_MIN_WIDTH > viewportRight) {
      x = viewportRight - SIDEBAR_MIN_WIDTH - SIDEBAR_MARGIN_RIGHT;
    }
    return Math.max(0, Math.round(x));
  },

  /**
   * Calculate sidebar y + height: in SVG coordinates following the scroll on
   * the SVG host, in viewport pixels below the toolbar on the page host.
   */
  _calcPosition(): { y: number; height: number } | null {
    if (this._host === 'page') {
      return {
        y: this._toolbarHeight + SIDEBAR_GAP_TOP,
        height: Math.round(
          CanvasSize.visibleArea().height -
            this._toolbarHeight -
            SIDEBAR_GAP_TOP,
        ),
      };
    }
    const svg = DomAdapter.getSvgRoot();
    if (!svg) return null;
    const rect = svg.getBoundingClientRect();
    const viewBox = svg.viewBox.baseVal;
    const scaleY = viewBox.height / rect.height;

    const scrollTop = Math.max(0, -rect.top) * scaleY;
    const y = scrollTop + this._toolbarHeight + SIDEBAR_GAP_TOP;
    const vpHeight = CanvasSize.visibleArea().height * scaleY;

    return {
      y: Math.round(y),
      height: Math.round(vpHeight - this._toolbarHeight - SIDEBAR_GAP_TOP),
    };
  },

  /**
   * Show sidebar transiently (hover preview). Debounced to prevent flicker.
   * No collapse handlers, adds sidebar-transient CSS class.
   */
  showTransient(
    arcId: string,
    overrideData?: {
      from: string;
      to: string;
      usages: ArcData['usages'];
      originalArcs?: string[];
      cycleIds?: number[];
    },
  ) {
    const S = STATIC_DATA.classes.sidebar;
    clearTimeout(this._debounceTimer ?? undefined);
    this._debounceTimer = setTimeout(() => {
      const el = this._getElement();
      if (!el) return;
      const innerDiv: HTMLElement | null = el.querySelector(`.${S.root}`);
      if (innerDiv) {
        innerDiv.innerHTML = this.buildContent(arcId, overrideData);
        innerDiv.classList.add(S.transient);
      }
      el.style.display = 'block';
      this._isTransient = true;
      this._cachedX = this._calcHostX();
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
   * Shared pinned-show logic: inject HTML, remove transient state, wire handlers, position.
   * @param html - Pre-built sidebar HTML content
   */
  _showWithContent(html: string) {
    const S = STATIC_DATA.classes.sidebar;
    const el = this._getElement();
    if (!el) return;
    const innerDiv: HTMLElement | null = el.querySelector(`.${S.root}`);
    if (innerDiv) {
      innerDiv.innerHTML = html;
      innerDiv.classList.remove(S.transient);
      this._setupCollapseHandlers(innerDiv);
    }
    el.style.display = 'block';
    this._isTransient = false;
    clearTimeout(this._debounceTimer ?? undefined);
    this._cachedMaxArcRightX = null;
    this._cachedX = this._calcHostX();
    this.updatePosition();
  },

  /**
   * Show sidebar with content for given arc.
   */
  show(
    arcId: string,
    overrideData?: {
      from: string;
      to: string;
      usages: ArcData['usages'];
      originalArcs?: string[];
      cycleIds?: number[];
    },
  ) {
    this._showWithContent(this.buildContent(arcId, overrideData));
  },

  /**
   * Path spans under `root`, each with the separator its kind is cut on.
   */
  _pathSpans(root: Element): Array<{ span: HTMLElement; separator: string }> {
    const S = STATIC_DATA.classes.sidebar;
    if (!root || !root.querySelectorAll) return [];
    const kinds = [
      { selector: `.${S.ns}[data-full]`, separator: '::' },
      { selector: `.${S.file}[data-full]`, separator: '/' },
    ];
    return kinds.flatMap(({ selector, separator }) =>
      [...root.querySelectorAll(selector)].map((span) => ({
        span: span as HTMLElement,
        separator,
      })),
    );
  },

  /**
   * Puts the full text back into every path span. Must run before the
   * sidebar measures its natural width, or the measurement sees the cut
   * text, the sidebar narrows, and the next pass cuts more.
   */
  resetPaths(root: Element) {
    for (const { span } of this._pathSpans(root)) PathFit.resetPath(span);
  },

  /**
   * Fits every path span under `root` to its box (`PathFit.fitPath`). Runs
   * after the sidebar width is final; the reset happens again on every call,
   * so a span that has more room than last time gets its segments back.
   * @param [overflows] - defaults to the DOM
   *   overflow test; tests pass a text-length predicate instead.
   */
  fitPaths(
    root: Element,
    overflows: (span: HTMLElement) => boolean = (span) =>
      span.scrollWidth > span.clientWidth,
  ) {
    for (const { span, separator } of this._pathSpans(root)) {
      PathFit.fitPath(span, separator, overflows);
    }
  },

  /**
   * Show or hide a collapsible head's list in place.
   */
  _setHeadExpanded(head: Element, expanded: boolean) {
    const S = STATIC_DATA.classes.sidebar;
    const locsEl = head.nextElementSibling as HTMLElement | null;
    const toggle = head.querySelector(`.${S.toggle}`);
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
   * Open every collapsed section above the rows carrying the given jump ids,
   * so the editor's cursor line is in view in the pinned sidebar. Ids without
   * a row, and rows whose sections are already open, change nothing.
   */
  expandLocations(jumpIds: number[]) {
    const S = STATIC_DATA.classes.sidebar;
    const root = this._getElement();
    const content = root?.querySelector(`.${S.content}`) as HTMLElement | null;
    if (!root || !content) return;
    const wanted = new Set(jumpIds.map(String));
    const rows = [...content.querySelectorAll('[data-jump]')].filter((row) =>
      wanted.has(row.getAttribute('data-jump') ?? ''),
    );
    let opened = false;
    for (const row of rows) {
      // A row sits in a `.sidebar-locations` list whose head is the element
      // before it; lists nest, so walk up through every enclosing one.
      let list = row.closest(`.${S.locations}`);
      while (list) {
        const head = list.previousElementSibling;
        if (
          head?.hasAttribute('data-collapsible') &&
          head.getAttribute('data-collapsed') === 'true'
        ) {
          this._setHeadExpanded(head, true);
          opened = true;
        }
        const parent = list.parentNode as Element | null;
        list = parent?.closest(`.${S.locations}`) ?? null;
      }
    }
    if (!opened) return;
    this._syncCollapseAllButton(root, content);
    this.updatePosition();
  },

  /**
   * Refresh the collapse-all button glyph from the rows' collapsed state.
   */
  _syncCollapseAllButton(root: HTMLElement, content: HTMLElement) {
    const S = STATIC_DATA.classes.sidebar;
    const allBtn = root.querySelector?.(`.${S.collapseAll}`);
    if (!allBtn || !content.querySelectorAll) return;
    const heads = [...content.querySelectorAll(collapsibleSymbolSelector(S))];
    const allCollapsed = heads.every(
      (s) => s.getAttribute('data-collapsed') === 'true',
    );
    allBtn.innerHTML = allCollapsed ? '+' : '−';
  },

  _setupCollapseHandlers(root: HTMLElement) {
    const S = STATIC_DATA.classes.sidebar;
    if (!root || !root.querySelector) return;
    const content = root.querySelector<HTMLElement>(`.${S.content}`);
    if (!content) return;
    content.addEventListener('click', (e) => {
      const target = e.target as Element;
      // A location row's, definition chip's or symbol row's click is a jump,
      // handled on the document; the row around it keeps its pin and
      // expansion state.
      if (target.closest?.(`.${S.location}[data-jump]`)) return;
      if (target.closest?.(`.${S.definition}[data-jump]`)) return;
      const symbolEl = target.closest<HTMLElement>(`.${S.symbol}`);
      if (!symbolEl) return;
      if (!symbolEl.hasAttribute('data-collapsible')) return;
      const locsEl = symbolEl.nextElementSibling as HTMLElement;
      const isCollapsed = symbolEl.getAttribute('data-collapsed') === 'true';
      if (isCollapsed) {
        symbolEl.removeAttribute('data-collapsed');
        locsEl.style.display = '';
        const toggle = symbolEl.querySelector(`.${S.toggle}`);
        if (toggle) toggle.innerHTML = '\u25BE';
      } else {
        symbolEl.setAttribute('data-collapsed', 'true');
        locsEl.style.display = 'none';
        const toggle = symbolEl.querySelector(`.${S.toggle}`);
        if (toggle) toggle.innerHTML = '\u25B8';
      }
      SidebarLogic._syncCollapseAllButton(root, content);
      SidebarLogic.updatePosition();
    });
    if (root.querySelectorAll) {
      const indicators = root.querySelectorAll<HTMLElement>(
        `.${S.collapseIndicator}`,
      );
      for (const indicator of indicators) {
        indicator.addEventListener('click', (e) => {
          e.stopPropagation();
          if (SidebarLogic._onCollapseToggle) {
            SidebarLogic._onCollapseToggle(indicator.dataset.collapseTarget);
          }
        });
      }
      const badges = root.querySelectorAll<HTMLElement>('[data-node-id]');
      for (const badge of badges) {
        badge.addEventListener('click', (e) => {
          e.stopPropagation();
          if (SidebarLogic._onBadgeClick) {
            SidebarLogic._onBadgeClick(badge.dataset.nodeId);
          }
        });
      }
    }
    const collapseAllBtn = root.querySelector(`.${S.collapseAll}`);
    if (!collapseAllBtn) return;
    collapseAllBtn.addEventListener('click', () => {
      const symbols = [
        ...content.querySelectorAll<HTMLElement>(collapsibleSymbolSelector(S)),
      ];
      if (!symbols.length) return;
      const anyExpanded = symbols.some(
        (s) => s.getAttribute('data-collapsed') !== 'true',
      );
      for (const symbolEl of symbols) {
        const locsEl = symbolEl.nextElementSibling as HTMLElement;
        const toggle = symbolEl.querySelector(`.${S.toggle}`);
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
   */
  showNode(nodeId: string, relations: NodeRelations) {
    this._showWithContent(this.buildNodeContent(nodeId, relations));
  },

  /**
   * Show sidebar transiently for node hover. Debounced.
   */
  showTransientNode(nodeId: string, relations: NodeRelations) {
    const S = STATIC_DATA.classes.sidebar;
    clearTimeout(this._debounceTimer ?? undefined);
    this._debounceTimer = setTimeout(() => {
      const el = this._getElement();
      if (!el) return;
      const innerDiv: HTMLElement | null = el.querySelector(`.${S.root}`);
      if (innerDiv) {
        innerDiv.innerHTML = this.buildNodeContent(nodeId, relations);
        innerDiv.classList.add(S.transient);
      }
      el.style.display = 'block';
      this._isTransient = true;
      this._cachedX = this._calcHostX();
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
    this._effectiveHeight = null;
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
   */
  isVisible(): boolean {
    const el = this._getElement();
    if (!el) return false;
    return el.style.display === 'block';
  },

  /**
   * Update sidebar position and size based on current scroll and viewport.
   */
  updatePosition() {
    const S = STATIC_DATA.classes.sidebar;
    const el = this._getElement();
    if (!el) return;
    const pos = this._calcPosition();
    if (!pos) return;

    // Dynamic width: on the SVG host expand the foreignObject first, shrink-wrap
    // .sidebar-root with max-content to measure the natural content width, then
    // clamp to bounds. Previous approach (shrink → measure scrollWidth) failed
    // because nested overflow containers (sidebar-content has implicit
    // overflow-x:auto) don't propagate scrollWidth reliably in foreignObject
    // context.
    const innerDiv: HTMLElement | null = el.querySelector(`.${S.root}`);
    if (innerDiv) this.resetPaths(innerDiv);
    if (this._host === 'svg') el.setAttribute('width', '9999');
    if (innerDiv) innerDiv.style.width = 'max-content';
    const naturalW = innerDiv ? innerDiv.offsetWidth : 0;
    if (innerDiv) innerDiv.style.width = '';

    // Measure natural content height (analogous to width measurement above).
    // Must happen after width is finalized — width affects text wrap → height.
    if (innerDiv) innerDiv.style.height = 'auto';
    const naturalH = innerDiv ? innerDiv.offsetHeight : 0;
    const effectiveH =
      naturalH > 0 ? Math.min(naturalH, pos.height) : pos.height;

    const vpWidth = CanvasSize.visibleArea().width;
    const width =
      this._host === 'page'
        ? CanvasSize.sidebarWidth(
            naturalW,
            CanvasSize.sidebarRoom(
              vpWidth,
              this._labelRightOnPage(),
              SIDEBAR_GAP_X,
              SIDEBAR_MARGIN_RIGHT,
            ),
            SIDEBAR_MIN_WIDTH,
          )
        : Math.max(SIDEBAR_MIN_WIDTH, Math.min(naturalW, vpWidth * 0.5));

    if (this._host === 'page') {
      this._placeOnPage(el, innerDiv, pos.y, width, effectiveH);
    } else {
      this._placeInSvg(el, innerDiv, pos.y, width, effectiveH, vpWidth);
    }
    this._effectiveHeight = effectiveH;
  },

  /**
   * Apply the measured size to a fixed page element docked at the right edge.
   * @param top - Distance from the viewport top, in px
   */
  _placeOnPage(
    el: HTMLElement,
    innerDiv: HTMLElement | null,
    top: number,
    width: number,
    height: number,
  ) {
    el.style.top = `${top}px`;
    el.style.right = `${SIDEBAR_MARGIN_RIGHT}px`;
    el.style.width = `${Math.round(width)}px`;
    if (innerDiv) {
      innerDiv.style.height = `${height}px`;
      this.fitPaths(innerDiv);
    }
  },

  /**
   * Apply the measured size to the foreignObject in SVG coordinates and grow
   * the canvas when the sidebar extends beyond it.
   */
  _placeInSvg(
    el: HTMLElement,
    innerDiv: HTMLElement | null,
    y: number,
    width: number,
    effectiveH: number,
    vpWidth: number,
  ) {
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
    el.setAttribute('y', String(y));
    el.setAttribute('height', String(effectiveH + SIDEBAR_SHADOW_PAD));
    if (innerDiv) {
      innerDiv.style.height = `${effectiveH}px`;
      this.fitPaths(innerDiv);
    }

    if (svg) {
      const vb = svg.viewBox.baseVal;
      this._growCanvasHeight(svg, y + effectiveH + SIDEBAR_SHADOW_PAD);

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

  /**
   * Move the sidebar to the current scroll position. On the SVG host width,
   * height and x depend only on content and viewport, so they keep the values
   * of the last updatePosition(). The page host is fixed in the viewport, but horizontal
   * scrolling changes the room left of it, so it takes its width again when
   * the SVG moved sideways.
   */
  followScroll() {
    if (this._host === 'page') {
      const left = DomAdapter.getSvgRoot()?.getBoundingClientRect().left;
      if (left !== this._pageSvgLeft) this.updatePosition();
      return;
    }
    if (this._effectiveHeight === null) {
      this.updatePosition();
      return;
    }
    const el = this._getElement();
    if (!el) return;
    const pos = this._calcPosition();
    if (!pos) return;
    el.setAttribute('y', String(pos.y));
    const svg = DomAdapter.getSvgRoot();
    if (svg) {
      this._growCanvasHeight(
        svg,
        pos.y + this._effectiveHeight + SIDEBAR_SHADOW_PAD,
      );
    }
  },

  /**
   * Expand the SVG canvas if the sidebar extends beyond the viewBox.
   */
  _growCanvasHeight(svg: SVGSVGElement, sidebarBottom: number) {
    const vb = svg.viewBox.baseVal;
    const originalH = this._originalViewBoxHeight ?? vb.height;
    if (this._originalViewBoxHeight === null) {
      this._originalViewBoxHeight = vb.height;
    }
    const neededH = Math.max(originalH, sidebarBottom);
    if (vb.height !== neededH) {
      vb.height = neededH;
      svg.setAttribute('height', String(neededH));
    }
  },
};

export { SidebarLogic };
