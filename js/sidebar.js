// @module SidebarLogic
// @deps StaticData, DomAdapter
// @config
// sidebar.js - Relation sidebar for arc usage details
// Shows usage locations when an arc is selected (pinned)
// foreignObject-based HTML sidebar with scroll tracking

const SIDEBAR_MAX_HEIGHT = 500;

const SidebarLogic = {
  /**
   * Parse usage strings into structured groups.
   * Input:  ["ModuleInfo  <- src/cli.rs:7", "             <- src/render.rs:12", "analyze  <- src/cli.rs:7"]
   * Output: [{ symbol: "ModuleInfo", locations: ["src/cli.rs:7", "src/render.rs:12"] }, ...]
   * Lines starting with non-space = new symbol, space-start = continuation of previous.
   * @param {string[]|undefined|null} usages
   * @returns {{ symbol: string, locations: string[] }[]}
   */
  parseUsages(usages) {
    if (!usages || usages.length === 0) return [];

    const groups = [];
    let current = null;

    for (const line of usages) {
      const arrowIdx = line.indexOf("<-");
      const location = arrowIdx >= 0 ? line.substring(arrowIdx + 3).trim() : line.trim();

      if (line.length > 0 && line[0] !== " ") {
        // New symbol: text before "  <-"
        const symbol = arrowIdx >= 0 ? line.substring(0, arrowIdx).trim() : line.trim();
        current = { symbol, locations: [location] };
        groups.push(current);
      } else {
        // Continuation line
        if (!current) {
          current = { symbol: "", locations: [] };
          groups.push(current);
        }
        current.locations.push(location);
      }
    }

    return groups;
  },

  /**
   * Build HTML content string for the sidebar.
   * Uses overrideData if provided, otherwise STATIC_DATA.arcs[arcId].
   * @param {string} arcId
   * @param {{ from: string, to: string, usages: string[] }} [overrideData]
   * @returns {string}
   */
  buildContent(arcId, overrideData) {
    const arc = overrideData || STATIC_DATA.arcs[arcId];
    if (!arc) return "";
    const from = arc.from;
    const to = arc.to;
    const groups = this.parseUsages(arc.usages);

    let html = `<div class="sidebar-header">`;
    html += `<span class="sidebar-title">${from} &#x2192; ${to}</span>`;
    html += `<button class="sidebar-close">&#x2715;</button>`;
    html += `</div>`;

    html += `<div class="sidebar-content">`;
    if (groups.length === 0) {
      html += `<div class="sidebar-usage-group">No usages</div>`;
    } else {
      for (const group of groups) {
        html += `<div class="sidebar-usage-group">`;
        html += `<div class="sidebar-symbol">${group.symbol}</div>`;
        for (const loc of group.locations) {
          html += `<div class="sidebar-location">${loc}</div>`;
        }
        html += `</div>`;
      }
    }
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
   * Calculate sidebar x/y in SVG coordinates based on visible viewport.
   * Centers sidebar horizontally in the browser window, tracks scroll vertically.
   * @returns {{ x: number, y: number, height: number }|null}
   */
  _calcPosition() {
    const svg = DomAdapter.getSvgRoot();
    if (!svg) return null;
    const rect = svg.getBoundingClientRect();
    const viewBox = svg.viewBox.baseVal;
    const scaleX = viewBox.width / rect.width;
    const scaleY = viewBox.height / rect.height;

    // Viewport center in SVG-coordinates
    const vpCenterX = (window.innerWidth / 2 - rect.left) * scaleX;
    const sidebarWidth = 280;
    const x = Math.max(0, vpCenterX - sidebarWidth / 2);

    // Scroll offset in SVG-coordinates
    const scrollTop = Math.max(0, -rect.top) * scaleY;
    const vpHeight = window.innerHeight * scaleY;

    return { x: Math.round(x), y: Math.round(scrollTop), height: Math.round(Math.min(vpHeight, SIDEBAR_MAX_HEIGHT)) };
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
    }
    el.style.display = "block";
    this.updatePosition();
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
    el.setAttribute("x", String(pos.x));
    el.setAttribute("y", String(pos.y));
    el.setAttribute("height", String(pos.height));
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== "undefined") {
  module.exports = { SidebarLogic };
}
