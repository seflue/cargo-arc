// @module SidebarLogic
// @deps StaticData, DomAdapter, Selectors
// @config TOOLBAR_HEIGHT, SIDEBAR_SHADOW_PAD
// sidebar.js - Relation sidebar for arc usage details
// Shows usage locations when an arc is selected (pinned)
// foreignObject-based HTML sidebar with scroll tracking

const SIDEBAR_MAX_HEIGHT = 500;
const TOOLBAR_HEIGHT = typeof __TOOLBAR_HEIGHT__ !== 'undefined' ? __TOOLBAR_HEIGHT__ : 0;
const SIDEBAR_GAP_X = 24;
const SIDEBAR_MARGIN_RIGHT = 16;
const SIDEBAR_GAP_TOP = 20;
// foreignObject must be taller than the visible sidebar so box-shadow
// (which renders outside the div) is not clipped by the foreignObject boundary.
// Value derived from box-shadow offset+blur in render.rs layout constants.
const SIDEBAR_SHADOW_PAD = typeof __SIDEBAR_SHADOW_PAD__ !== 'undefined' ? __SIDEBAR_SHADOW_PAD__ : 12;
const SIDEBAR_MIN_WIDTH = 280;

const SidebarLogic = {
  _isTransient: false,
  _debounceTimer: null,
  /**
   * Merge symbol groups: combine groups with same symbol, deduplicate locations by file+line.
   * @param {Array<{symbol: string, modulePath: string|null, locations: Array<{file: string, line: number}>}>} groups
   * @returns {Array<{symbol: string, modulePath: string|null, locations: Array<{file: string, line: number}>}>}
   */
  mergeSymbolGroups(groups) {
    const bySymbol = new Map();
    for (const g of groups) {
      const key = g.symbol || '';
      const existing = bySymbol.get(key);
      if (existing) {
        for (const loc of g.locations) {
          const isDup = existing.locations.some(e => e.file === loc.file && e.line === loc.line);
          if (!isDup) existing.locations.push(loc);
        }
      } else {
        bySymbol.set(key, { symbol: g.symbol, modulePath: g.modulePath, locations: [...g.locations] });
      }
    }
    return [...bySymbol.values()];
  },

  /**
   * Build HTML content string for the sidebar.
   * Uses overrideData if provided, otherwise STATIC_DATA.arcs[arcId].
   * Expects structured usages: [{ symbol, modulePath, locations: [{ file, line }] }]
   * @param {string} arcId
   * @param {{ from: string, to: string, usages: Array, originalArcs?: string[] }} [overrideData]
   * @returns {string}
   */
  buildContent(arcId, overrideData) {
    const arc = overrideData || STATIC_DATA.arcs[arcId];
    if (!arc) return "";
    const groups = arc.usages || [];

    const fromNode = StaticData.getNode(arc.from);
    const toNode = StaticData.getNode(arc.to);
    const fromName = fromNode ? fromNode.name : arc.from;
    const toName = toNode ? toNode.name : arc.to;
    const fromClass = (fromNode ? `sidebar-node-${fromNode.type} ` : '') + 'sidebar-node-from';
    const toClass = (toNode ? `sidebar-node-${toNode.type} ` : '') + 'sidebar-node-to';

    let html = `<div class="sidebar-header">`;
    html += `<span class="sidebar-title"><span class="${fromClass}">${fromName}</span><span class="sidebar-arrow">&#x2192;</span><span class="${toClass}">${toName}</span></span>`;
    const hasSymbols = groups.some(g => g.symbol);
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
      const sorted = [...groups].sort((a, b) => b.locations.length - a.locations.length);
      for (const group of sorted) {
        const collapsed = false;
        html += `<div class="sidebar-usage-group">`;
        if (group.symbol) {
          html += `<div class="sidebar-symbol"${collapsed ? ' data-collapsed="true"' : ''}>`;
          html += `<span class="sidebar-toggle">${collapsed ? '&#x25B8;' : '&#x25BE;'}</span>`;
          if (group.modulePath) {
            html += `<span class="sidebar-ns">${group.modulePath}::</span>`;
          }
          html += `<span class="sidebar-symbol-name">${group.symbol}</span>`;
          html += `<span class="sidebar-ref-count">${group.locations.length}</span>`;
          html += `</div>`;
        }
        html += `<div class="sidebar-locations"${collapsed ? ' style="display:none"' : ''}>`;
        for (const loc of group.locations) {
          html += `<div class="sidebar-location">${loc.file}<span class="sidebar-line-badge">:${loc.line}</span></div>`;
        }
        html += `</div>`;
        html += `</div>`;
      }
    }
    html += `</div>`;

    // Footer
    const totalLocs = groups.reduce((sum, g) => sum + g.locations.length, 0);
    const symbolCount = groups.filter(g => g.symbol).length;
    let footerText = `${totalLocs} Referenzen \u00b7 ${symbolCount} Symbole`;
    if (overrideData && overrideData.originalArcs) {
      footerText += ` \u00b7 ${overrideData.originalArcs.length} Relations`;
    }
    html += `<div class="sidebar-footer">${footerText}</div>`;

    return html;
  },

  /**
   * Build HTML content string for node-mode sidebar.
   * Shows all incoming (dependents) and outgoing (dependencies) relations.
   * @param {string} nodeId - The selected node ID
   * @param {{ incoming: Array, outgoing: Array }} relations - From StaticData.getNodeRelations()
   * @returns {string}
   */
  buildNodeContent(nodeId, relations) {
    const node = StaticData.getNode(nodeId);
    const nodeName = node ? node.name : nodeId;
    const nodeType = node ? node.type : '';
    const hasRelations = relations.incoming.length > 0 || relations.outgoing.length > 0;

    // Header: Node name + Collapse-All ("+", since all L1 start collapsed) + Close
    let html = `<div class="sidebar-header">`;
    html += `<span class="sidebar-title"><span class="sidebar-node-${nodeType}">${nodeName}</span></span>`;
    if (hasRelations) {
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
      // Incoming (Dependents) first — selected node on the right
      for (const rel of relations.incoming) {
        html += this._buildRelationSection(rel, nodeId, nodeName, nodeType, 'incoming');
      }

      // Divider only if both directions non-empty
      if (relations.incoming.length > 0 && relations.outgoing.length > 0) {
        html += `<hr class="sidebar-divider"/>`;
      }

      // Outgoing (Dependencies) — selected node on the left
      for (const rel of relations.outgoing) {
        html += this._buildRelationSection(rel, nodeId, nodeName, nodeType, 'outgoing');
      }
    }

    html += `</div>`;

    // Footer
    const total = relations.incoming.length + relations.outgoing.length;
    html += `<div class="sidebar-footer">${total} Relations \u00b7 ${relations.incoming.length} Dependents \u00b7 ${relations.outgoing.length} Dependencies</div>`;

    return html;
  },

  /**
   * Build a single Level-1 relation section (collapsed) with nested Level-2 usages.
   * @param {Object} rel - Relation entry {targetId, weight, usages, arcId}
   * @param {string} nodeId - Selected node ID
   * @param {string} nodeName - Selected node display name
   * @param {string} nodeType - Selected node type (crate/module)
   * @param {'incoming'|'outgoing'} direction
   * @returns {string}
   */
  _buildRelationSection(rel, nodeId, nodeName, nodeType, direction) {
    const target = StaticData.getNode(rel.targetId);
    const targetName = target ? target.name : rel.targetId;
    const targetType = target ? target.type : '';

    // Build From→To pair: direction determines which side the selected node is on
    let fromName, fromType, fromSelected, toName, toType, toSelected;
    if (direction === 'incoming') {
      // source → [selected]: selected is on the right
      fromName = targetName; fromType = targetType; fromSelected = false;
      toName = nodeName; toType = nodeType; toSelected = true;
    } else {
      // [selected] → target: selected is on the left
      fromName = nodeName; fromType = nodeType; fromSelected = true;
      toName = targetName; toType = targetType; toSelected = false;
    }

    const fromClass = `sidebar-node-${fromType}${fromSelected ? ' sidebar-node-selected' : ''}`;
    const toClass = `sidebar-node-${toType}${toSelected ? ' sidebar-node-selected' : ''}`;

    let html = `<div class="sidebar-usage-group">`;
    // Level 1 header (collapsed)
    html += `<div class="sidebar-symbol" data-collapsed="true">`;
    html += `<span class="sidebar-toggle">&#x25B8;</span>`;
    html += `<span class="${fromClass} sidebar-symbol-name">${fromName}</span>`;
    html += `<span class="sidebar-arrow">&#x2192;</span>`;
    html += `<span class="${toClass} sidebar-symbol-name">${toName}</span>`;
    html += `<span class="sidebar-ref-count">${rel.weight}</span>`;
    html += `</div>`;

    // Level 2 content (hidden because L1 is collapsed)
    html += `<div class="sidebar-locations" style="display:none">`;
    const groups = rel.usages || [];
    if (groups.length === 0) {
      html += `<div class="sidebar-usage-group">Cargo.toml dependency</div>`;
    } else {
      const sorted = [...groups].sort((a, b) => b.locations.length - a.locations.length);
      for (const group of sorted) {
        html += `<div class="sidebar-usage-group">`;
        if (group.symbol) {
          html += `<div class="sidebar-symbol">`;
          html += `<span class="sidebar-toggle">&#x25BE;</span>`;
          if (group.modulePath) {
            html += `<span class="sidebar-ns">${group.modulePath}::</span>`;
          }
          html += `<span class="sidebar-symbol-name">${group.symbol}</span>`;
          html += `<span class="sidebar-ref-count">${group.locations.length}</span>`;
          html += `</div>`;
        }
        html += `<div class="sidebar-locations">`;
        for (const loc of group.locations) {
          html += `<div class="sidebar-location">${loc.file}<span class="sidebar-line-badge">:${loc.line}</span></div>`;
        }
        html += `</div>`;
        html += `</div>`;
      }
    }
    html += `</div>`;

    html += `</div>`;
    return html;
  },

  /**
   * Get the foreignObject element for the sidebar.
   * @returns {Element|null}
   */
  _getElement() {
    return DomAdapter.getElementById("relation-sidebar");
  },

  /**
   * Find the rightmost X coordinate among all visible arc paths.
   * @returns {number}
   */
  _getMaxArcRightX() {
    const arcs = DomAdapter.querySelectorAll(Selectors.allArcPaths());
    let maxX = 0;
    for (const arc of arcs) {
      if (arc.style.display === 'none') continue;
      const bbox = arc.getBBox();
      maxX = Math.max(maxX, bbox.x + bbox.width);
    }
    return maxX;
  },

  /** Cached X position — set once in show(), reused by updatePosition(). */
  _cachedX: null,
  /** Original SVG viewBox height — stored to restore after sidebar close. */
  _originalViewBoxHeight: null,

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

    const viewportRight = (window.innerWidth - rect.left) * scaleX;
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
    const vpHeight = window.innerHeight * scaleY;

    return {
      y: Math.round(y),
      height: Math.round(Math.min(vpHeight - TOOLBAR_HEIGHT - SIDEBAR_GAP_TOP, SIDEBAR_MAX_HEIGHT)),
    };
  },

  /**
   * Show sidebar transiently (hover preview). Debounced to prevent flicker.
   * No collapse handlers, adds sidebar-transient CSS class.
   * @param {string} arcId
   * @param {{ from: string, to: string, usages: Array, originalArcs?: string[] }} [overrideData]
   */
  showTransient(arcId, overrideData) {
    clearTimeout(this._debounceTimer);
    this._debounceTimer = setTimeout(() => {
      const el = this._getElement();
      if (!el) return;
      const innerDiv = el.querySelector(".sidebar-root");
      if (innerDiv) {
        innerDiv.innerHTML = this.buildContent(arcId, overrideData);
        innerDiv.classList.add("sidebar-transient");
      }
      el.style.display = "block";
      this._isTransient = true;
      this._cachedX = this._calcX();
      this.updatePosition();
    }, 30);
  },

  /**
   * Hide a transient sidebar. Does nothing if sidebar is pinned.
   */
  hideTransient() {
    clearTimeout(this._debounceTimer);
    if (!this._isTransient) return;
    this.hide();
    this._isTransient = false;
  },

  /**
   * Show sidebar with content for given arc.
   * @param {string} arcId
   * @param {{ from: string, to: string, usages: string[] }} [overrideData]
   */
  show(arcId, overrideData) {
    const el = this._getElement();
    if (!el) return;
    const innerDiv = el.querySelector(".sidebar-root");
    if (innerDiv) {
      innerDiv.innerHTML = this.buildContent(arcId, overrideData);
      innerDiv.classList.remove("sidebar-transient");
      this._setupCollapseHandlers(innerDiv);
    }
    el.style.display = "block";
    this._isTransient = false;
    clearTimeout(this._debounceTimer);
    this._cachedX = this._calcX();
    this.updatePosition();
  },

  _setupCollapseHandlers(root) {
    if (!root || !root.querySelector) return;
    const content = root.querySelector(".sidebar-content");
    if (!content) return;
    content.addEventListener("click", (e) => {
      const symbolEl = e.target.closest(".sidebar-symbol");
      if (!symbolEl) return;
      const locsEl = symbolEl.nextElementSibling;
      if (!locsEl || !locsEl.classList.contains("sidebar-locations")) return;
      const isCollapsed = symbolEl.getAttribute("data-collapsed") === "true";
      if (isCollapsed) {
        symbolEl.removeAttribute("data-collapsed");
        locsEl.style.display = "";
        const toggle = symbolEl.querySelector(".sidebar-toggle");
        if (toggle) toggle.innerHTML = "\u25BE";
      } else {
        symbolEl.setAttribute("data-collapsed", "true");
        locsEl.style.display = "none";
        const toggle = symbolEl.querySelector(".sidebar-toggle");
        if (toggle) toggle.innerHTML = "\u25B8";
      }
      const allBtn = root.querySelector(".sidebar-collapse-all");
      if (allBtn) {
        const allCollapsed = Array.from(content.querySelectorAll(".sidebar-symbol"))
          .every(s => s.getAttribute("data-collapsed") === "true");
        allBtn.innerHTML = allCollapsed ? "+" : "\u2212";
      }
      SidebarLogic.updatePosition();
    });
    const collapseAllBtn = root.querySelector(".sidebar-collapse-all");
    if (!collapseAllBtn) return;
    collapseAllBtn.addEventListener("click", () => {
      const symbols = content.querySelectorAll(".sidebar-symbol");
      if (!symbols.length) return;
      const anyExpanded = Array.from(symbols).some(
        s => s.getAttribute("data-collapsed") !== "true"
      );
      for (const symbolEl of symbols) {
        const locsEl = symbolEl.nextElementSibling;
        if (!locsEl || !locsEl.classList.contains("sidebar-locations")) continue;
        const toggle = symbolEl.querySelector(".sidebar-toggle");
        if (anyExpanded) {
          symbolEl.setAttribute("data-collapsed", "true");
          locsEl.style.display = "none";
          if (toggle) toggle.innerHTML = "\u25B8";
        } else {
          symbolEl.removeAttribute("data-collapsed");
          locsEl.style.display = "";
          if (toggle) toggle.innerHTML = "\u25BE";
        }
      }
      collapseAllBtn.innerHTML = anyExpanded ? "+" : "\u2212";
      SidebarLogic.updatePosition();
    });
  },

  /**
   * Show sidebar with content for given node (pinned click).
   * @param {string} nodeId
   * @param {{ incoming: Array, outgoing: Array }} relations
   */
  showNode(nodeId, relations) {
    const el = this._getElement();
    if (!el) return;
    const innerDiv = el.querySelector(".sidebar-root");
    if (innerDiv) {
      innerDiv.innerHTML = this.buildNodeContent(nodeId, relations);
      innerDiv.classList.remove("sidebar-transient");
      this._setupCollapseHandlers(innerDiv);
    }
    el.style.display = "block";
    this._isTransient = false;
    clearTimeout(this._debounceTimer);
    this._cachedX = this._calcX();
    this.updatePosition();
  },

  /**
   * Show sidebar transiently for node hover. Debounced.
   * @param {string} nodeId
   * @param {{ incoming: Array, outgoing: Array }} relations
   */
  showTransientNode(nodeId, relations) {
    clearTimeout(this._debounceTimer);
    this._debounceTimer = setTimeout(() => {
      const el = this._getElement();
      if (!el) return;
      const innerDiv = el.querySelector(".sidebar-root");
      if (innerDiv) {
        innerDiv.innerHTML = this.buildNodeContent(nodeId, relations);
        innerDiv.classList.add("sidebar-transient");
      }
      el.style.display = "block";
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
    el.style.display = "none";
    this._cachedX = null;
    this._isTransient = false;
    clearTimeout(this._debounceTimer);

    // Restore original SVG canvas height
    if (this._originalViewBoxHeight !== null) {
      const svg = DomAdapter.getSvgRoot();
      if (svg) {
        svg.viewBox.baseVal.height = this._originalViewBoxHeight;
        svg.setAttribute("height", String(this._originalViewBoxHeight));
      }
      this._originalViewBoxHeight = null;
    }
  },

  /**
   * Check if sidebar is currently visible.
   * @returns {boolean}
   */
  isVisible() {
    const el = this._getElement();
    if (!el) return false;
    return el.style.display === "block";
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
    const innerDiv = el.querySelector(".sidebar-root");
    el.setAttribute("width", "9999");
    if (innerDiv) innerDiv.style.width = "max-content";
    const naturalW = innerDiv ? innerDiv.offsetWidth : 0;
    if (innerDiv) innerDiv.style.width = "";
    const vpWidth = window.innerWidth;
    const width = Math.max(SIDEBAR_MIN_WIDTH, Math.min(naturalW, vpWidth * 0.5));

    el.setAttribute("width", String(Math.round(width) + SIDEBAR_SHADOW_PAD));
    el.setAttribute("x", String(this._cachedX != null ? this._cachedX : this._calcX()));
    el.setAttribute("y", String(pos.y));
    el.setAttribute("height", String(pos.height + SIDEBAR_SHADOW_PAD));
    if (innerDiv) innerDiv.style.height = pos.height + 'px';

    // Expand SVG canvas if sidebar extends beyond viewBox
    const svg = DomAdapter.getSvgRoot();
    if (svg) {
      const vb = svg.viewBox.baseVal;
      if (this._originalViewBoxHeight === null) {
        this._originalViewBoxHeight = vb.height;
      }
      const sidebarBottom = pos.y + pos.height + SIDEBAR_SHADOW_PAD;
      const needed = Math.max(this._originalViewBoxHeight, sidebarBottom);
      if (vb.height !== needed) {
        vb.height = needed;
        svg.setAttribute("height", String(needed));
      }
    }
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== "undefined") {
  module.exports = { SidebarLogic };
}
