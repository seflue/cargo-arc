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
    if (overrideData && overrideData.originalArcs) {
      html += `<span class="sidebar-badge-relations">${overrideData.originalArcs.length} relations</span>`;
    }
    html += `<button class="sidebar-close">&#x2715;</button>`;
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
      this._setupCollapseHandlers(innerDiv);
    }
    el.style.display = "block";
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
    });
  },

  /**
   * Hide the sidebar.
   */
  hide() {
    const el = this._getElement();
    if (!el) return;
    el.style.display = "none";
    this._cachedX = null;
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

    // Dynamic width: measure content, then size foreignObject to fit.
    // Shrink to min-width first so scrollWidth reflects content, not container.
    const innerDiv = el.querySelector(".sidebar-root");
    el.setAttribute("width", String(SIDEBAR_MIN_WIDTH));
    const scrollW = innerDiv ? (innerDiv.scrollWidth || 0) : 0;
    const vpWidth = window.innerWidth;
    const width = Math.max(SIDEBAR_MIN_WIDTH, Math.min(scrollW + 20, vpWidth * 0.5));

    el.setAttribute("width", String(Math.round(width) + SIDEBAR_SHADOW_PAD));
    el.setAttribute("x", String(this._cachedX != null ? this._cachedX : this._calcX()));
    el.setAttribute("y", String(pos.y));
    el.setAttribute("height", String(pos.height + SIDEBAR_SHADOW_PAD));
    if (innerDiv) innerDiv.style.height = pos.height + 'px';
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== "undefined") {
  module.exports = { SidebarLogic };
}
