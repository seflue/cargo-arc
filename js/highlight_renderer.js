// @module HighlightRenderer
// @deps ArcLogic, LayerManager
// @config
// highlight_renderer.js - Single entry point for applying highlight state to DOM
// Takes a HighlightState (from DerivedState.deriveHighlightState) and applies it.
// Reset uses data-iteration (StaticData/virtualArcUsages), not CSS selectors.

const HighlightRenderer = {
  /**
   * Apply highlight state to DOM. Resets everything first, then applies state.
   * @param {Object} dom - DomAdapter instance
   * @param {Object} staticData - StaticData accessor
   * @param {Map<string, Array>} virtualArcUsages - Runtime virtual arc usage map
   * @param {HighlightState|null} state - Highlight state from deriveHighlightState, or null to reset
   */
  apply(dom, staticData, virtualArcUsages, state) {
    const C = STATIC_DATA.classes;
    this.resetToBase(dom, C, staticData, virtualArcUsages);

    if (!state) return;

    this._applyNodeClasses(dom, C, state.nodeHighlights);
    this._applyArcHighlights(dom, C, state.arcHighlights);
    this._applyArrowScaling(dom, C, state.arcHighlights);
    this._createShadows(dom, C, state.shadowData);
    this._promoteToHighlightLayers(
      dom,
      C,
      state.arcHighlights,
      state.promotedHitareas,
    );
    this._activateDimming(dom, C);
  },

  /**
   * Reset all highlight-related DOM state to base.
   * Uses data-iteration (no CSS selector queries for finding highlighted elements).
   * @param {Object} dom - DomAdapter instance
   * @param {Object} C - STATIC_DATA.classes
   * @param {Object} staticData - StaticData accessor
   * @param {Map<string, Array>} virtualArcUsages - Runtime virtual arc usage map
   */
  resetToBase(dom, C, staticData, virtualArcUsages) {
    this._resetDimming(dom, C);
    this._resetLayers(dom);
    this._clearShadowLayer(dom);
    this._resetNodeClasses(dom, C, staticData);
    this._resetArcStyles(dom, C, staticData);
    this._resetVirtualArcStyles(dom, C, virtualArcUsages);
  },

  /**
   * Remove has-highlight class from SVG root (CSS-only dimming reset).
   */
  _resetDimming(dom, C) {
    const svg = dom.getSvgRoot();
    if (svg) svg.classList.remove(C.hasHighlight);
  },

  /**
   * Add has-highlight class to SVG root (CSS-only dimming activation).
   */
  _activateDimming(dom, C) {
    const svg = dom.getSvgRoot();
    if (svg) svg.classList.add(C.hasHighlight);
  },

  /**
   * Move all elements from highlight layers back to base layers.
   */
  _resetLayers(dom) {
    const moveBack = (highlightId, baseId) => {
      const hl = dom.getElementById(highlightId);
      const bl = dom.getElementById(baseId);
      if (hl && bl) {
        while (hl.firstChild) bl.appendChild(hl.firstChild);
      }
    };
    moveBack(LayerManager.LAYERS.HIGHLIGHT_ARCS, LayerManager.LAYERS.BASE_ARCS);
    moveBack(
      LayerManager.LAYERS.HIGHLIGHT_LABELS,
      LayerManager.LAYERS.BASE_LABELS,
    );
    moveBack(
      LayerManager.LAYERS.HIGHLIGHT_HITAREAS,
      LayerManager.LAYERS.HITAREAS,
    );
  },

  /**
   * Clear all shadow paths from shadow layer.
   */
  _clearShadowLayer(dom) {
    LayerManager.clearLayer(LayerManager.LAYERS.SHADOWS, dom);
  },

  /**
   * Remove highlight CSS classes from all nodes (data-iteration).
   */
  _resetNodeClasses(dom, C, staticData) {
    const nodeClasses = [
      C.selectedCrate,
      C.selectedModule,
      C.selectedExternal,
      C.groupMember,
      C.cycleMember,
      C.depNode,
      C.dependentNode,
    ];
    for (const nodeId of staticData.getAllNodeIds()) {
      const node = dom.getNode(nodeId);
      if (node) {
        for (const cls of nodeClasses) node.classList.remove(cls);
      }
    }
  },

  /**
   * Reset all regular arcs to base styling (data-iteration).
   */
  _resetArcStyles(dom, C, staticData) {
    for (const arcId of staticData.getAllArcIds()) {
      const arc = dom.getArc(arcId);
      if (arc) {
        arc.classList.remove(C.highlightedArc);
        arc.style.strokeWidth = `${staticData.getArcStrokeWidth(arcId)}px`;
      }

      // Reset arrows (including hidden — prevents stale state after collapse/expand)
      const originalWidth = staticData.getArcStrokeWidth(arcId);
      const scale = ArcLogic.scaleFromStrokeWidth(originalWidth);
      dom.getArrows(arcId).forEach((arrow) => {
        arrow.classList.remove(C.highlightedArrow);
        const tip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
        if (tip) {
          arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, scale));
        }
      });

      // Reset labels
      const labelGroup = dom.getLabelGroup(arcId);
      if (labelGroup) {
        const labels = labelGroup.querySelectorAll(`.${C.arcCount}`);
        labels.forEach((el) => {
          el.classList.remove(C.highlightedLabel);
        });
      }
    }
  },

  /**
   * Set CSS classes on highlighted nodes.
   */
  _applyNodeClasses(dom, C, nodeHighlights) {
    for (const [nodeId, { cssClass }] of nodeHighlights) {
      const node = dom.getNode(nodeId);
      if (node) node.classList.add(C[cssClass]);
    }
  },

  /**
   * Set highlighted class and stroke-width on arcs, plus label highlighting.
   * Keys: "from-to" for regular arcs, "v:from-to" for virtual arcs.
   */
  _applyArcHighlights(dom, C, arcHighlights) {
    for (const [key, { highlightWidth, isVirtual }] of arcHighlights) {
      const arcId = isVirtual ? key.slice(2) : key;

      if (isVirtual) {
        dom
          .querySelectorAll(`.${C.virtualArc}[data-arc-id="${arcId}"]`)
          .forEach((arc) => {
            arc.classList.add(C.highlightedArc);
            arc.style.strokeWidth = `${highlightWidth}px`;
          });
        dom
          .querySelectorAll(`.${C.arcCount}[data-vedge="${arcId}"]`)
          .forEach((el) => {
            el.classList.add(C.highlightedLabel);
          });
      } else {
        const arc = dom.getVisibleArc(arcId);
        if (arc) {
          arc.classList.add(C.highlightedArc);
          arc.style.strokeWidth = `${highlightWidth}px`;
        }
        const labelGroup = dom.getLabelGroup(arcId);
        if (labelGroup) {
          labelGroup.querySelectorAll(`.${C.arcCount}`).forEach((el) => {
            el.classList.add(C.highlightedLabel);
          });
        }
      }
    }
  },

  /**
   * Scale arrows to match highlight widths.
   */
  _applyArrowScaling(dom, C, arcHighlights) {
    for (const [key, { arrowScale, isVirtual }] of arcHighlights) {
      const arcId = isVirtual ? key.slice(2) : key;
      const arrows = isVirtual
        ? dom.querySelectorAll(`.${C.virtualArrow}[data-vedge="${arcId}"]`)
        : dom.getVisibleArrows(arcId);

      arrows.forEach((arrow) => {
        arrow.classList.add(C.highlightedArrow);
        const tip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
        if (tip) {
          arrow.setAttribute(
            'points',
            ArcLogic.getArrowPoints(tip, arrowScale),
          );
        }
      });
    }
  },

  /**
   * Create shadow glow paths by cloning arc elements and applying pre-computed styling.
   * pathLength is estimated from the actual DOM path (authoritative for display).
   */
  _createShadows(dom, C, shadowData) {
    const shadowLayer = dom.getElementById(LayerManager.LAYERS.SHADOWS);
    if (!shadowLayer) return;

    for (const [
      key,
      { shadowWidth, visibleLength, dashOffset, glowClass },
    ] of shadowData) {
      const isVirtual = key.startsWith('v:');
      const arcId = isVirtual ? key.slice(2) : key;
      const arcs = isVirtual
        ? dom.querySelectorAll(`.${C.virtualArc}[data-arc-id="${arcId}"]`)
        : [dom.getVisibleArc(arcId)];

      for (const arc of arcs) {
        if (!arc) continue;
        const shadow = arc.cloneNode(false);
        shadow.classList.remove(C.downward, C.upward);
        shadow.classList.add(C.shadowPath);
        shadow.classList.add(C[glowClass]);
        shadow.removeAttribute('id');
        shadow.removeAttribute('data-arc-id');

        const pathLength = ArcLogic.estimatePathLength(arc.getAttribute('d'));
        shadow.style.strokeWidth = `${shadowWidth}px`;
        shadow.setAttribute('opacity', '0.25');
        shadow.style.strokeLinecap = 'round';
        shadow.style.strokeDasharray = `${visibleLength} ${pathLength}`;
        shadow.style.strokeDashoffset = `${dashOffset}px`;

        shadowLayer.appendChild(shadow);
      }
    }
  },

  /**
   * Move highlighted arcs, arrows, labels, and hitareas to highlight layers (higher z-order).
   */
  _promoteToHighlightLayers(dom, C, arcHighlights, promotedHitareas) {
    for (const [key, { isVirtual }] of arcHighlights) {
      const arcId = isVirtual ? key.slice(2) : key;

      if (isVirtual) {
        dom
          .querySelectorAll(`.${C.virtualArc}[data-arc-id="${arcId}"]`)
          .forEach((el) => {
            LayerManager.moveToHighlightLayer(el, dom);
          });
        dom.getVirtualArrows(arcId).forEach((el) => {
          LayerManager.moveToHighlightLayer(el, dom);
        });
      } else {
        LayerManager.moveToHighlightLayer(dom.getVisibleArc(arcId), dom);
        dom.getVisibleArrows(arcId).forEach((el) => {
          LayerManager.moveToHighlightLayer(el, dom);
        });
      }
      LayerManager.moveToHighlightLayer(dom.getLabelGroup(arcId), dom);
    }

    for (const arcId of promotedHitareas) {
      LayerManager.moveToLayer(
        dom.getHitarea(arcId),
        LayerManager.LAYERS.HIGHLIGHT_HITAREAS,
        dom,
      );
    }
  },

  /**
   * Reset all virtual arcs to base styling (data-iteration over virtualArcUsages).
   */
  _resetVirtualArcStyles(dom, C, virtualArcUsages) {
    for (const [arcId, usages] of virtualArcUsages) {
      const count = usages.reduce((sum, g) => sum + g.locations.length, 0);
      const strokeWidth = ArcLogic.calculateStrokeWidth(count);
      const scale = ArcLogic.scaleFromStrokeWidth(strokeWidth);

      // Virtual arc paths
      dom
        .querySelectorAll(`.${C.virtualArc}[data-arc-id="${arcId}"]`)
        .forEach((arc) => {
          arc.classList.remove(C.highlightedArc);
          arc.style.strokeWidth = `${strokeWidth}px`;
        });

      // Virtual arrows
      dom
        .querySelectorAll(`.${C.virtualArrow}[data-vedge="${arcId}"]`)
        .forEach((arrow) => {
          arrow.classList.remove(C.highlightedArrow);
          const tip = ArcLogic.parseTipFromPoints(arrow.getAttribute('points'));
          if (tip) {
            arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, scale));
          }
        });

      // Virtual labels
      dom
        .querySelectorAll(`.${C.arcCount}[data-vedge="${arcId}"]`)
        .forEach((el) => {
          el.classList.remove(C.highlightedLabel);
        });
    }
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { HighlightRenderer };
}
