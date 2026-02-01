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

    let html = `<div class="sidebar-header">`;
    html += `<span class="sidebar-title">${arc.from} &#x2192; ${arc.to}</span>`;
    if (overrideData && overrideData.originalArcs) {
      html += `<span class="sidebar-badge-relations">${overrideData.originalArcs.length} relations</span>`;
    }
    html += `<button class="sidebar-close">&#x2715;</button>`;
    html += `</div>`;

    html += `<div class="sidebar-content">`;
    if (groups.length === 0) {
      html += `<div class="sidebar-usage-group">Cargo.toml dependency</div>`;
    } else {
      for (const group of groups) {
        const collapsed = group.locations.length >= 5;
        html += `<div class="sidebar-usage-group">`;
        if (group.symbol) {
          html += `<div class="sidebar-symbol"${collapsed ? ' data-collapsed="true"' : ''}>`;
          html += `<span class="sidebar-toggle">${collapsed ? '&#x25B8;' : '&#x25BE;'}</span>`;
          html += `${group.symbol}`;
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

  /**
   * Calculate sidebar x/y in SVG coordinates based on visible viewport.
   * Positions sidebar right of the widest visible arc, tracks scroll vertically.
   * @returns {{ x: number, y: number, height: number }|null}
   */
  _calcPosition() {
    const svg = DomAdapter.getSvgRoot();
    if (!svg) return null;
    const rect = svg.getBoundingClientRect();
    const viewBox = svg.viewBox.baseVal;
    const scaleX = viewBox.width / rect.width;
    const scaleY = viewBox.height / rect.height;

    const sidebarWidth = 280;

    // X: right of the widest visible arc + gap
    const maxArcRight = this._getMaxArcRightX();
    let x = maxArcRight + SIDEBAR_GAP_X;

    // Fallback: right viewport edge - width - margin
    const viewportRight = (window.innerWidth - rect.left) * scaleX;
    if (x + sidebarWidth > viewportRight) {
      x = viewportRight - sidebarWidth - SIDEBAR_MARGIN_RIGHT;
    }
    x = Math.max(0, x);

    // Y: scroll offset + toolbar height + gap
    const scrollTop = Math.max(0, -rect.top) * scaleY;
    const y = scrollTop + TOOLBAR_HEIGHT + SIDEBAR_GAP_TOP;

    const vpHeight = window.innerHeight * scaleY;

    return {
      x: Math.round(x),
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

    // Dynamic width based on content
    const innerDiv = el.querySelector(".sidebar-root");
    const scrollW = innerDiv ? (innerDiv.scrollWidth || 0) : 0;
    const vpWidth = window.innerWidth;
    const width = Math.max(280, Math.min(scrollW + 20, vpWidth * 0.5));

    el.setAttribute("width", String(Math.round(width)));
    el.setAttribute("x", String(pos.x));
    el.setAttribute("y", String(pos.y));
    el.setAttribute("height", String(pos.height + SIDEBAR_SHADOW_PAD));
    if (innerDiv) innerDiv.style.height = pos.height + 'px';
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== "undefined") {
  module.exports = { SidebarLogic };
}
