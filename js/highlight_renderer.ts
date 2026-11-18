import { ArcLogic } from './arc_logic.js';
import { ArcStroke } from './arc_stroke.js';
import type { HighlightState } from './derived_state.js';
import type { DomAdapter } from './dom_adapter.js';
import { LayerManager } from './layer_manager.js';
import type { StaticData } from './static_data.js';

// highlight_renderer.ts - Single entry point for applying highlight state to DOM
// Takes a HighlightState (from DerivedState.deriveHighlightState) and applies it.
// Reset uses data-iteration (StaticData/virtualArcUsages), not CSS selectors.

type Dom = typeof DomAdapter;
type Classes = typeof STATIC_DATA.classes;

const HighlightRenderer = {
  // Dirty sets: track which elements were styled in last apply() for O(1) reset
  _prevNodeIds: new Set<string>(),
  _prevRegularArcIds: new Set<string>(),
  _prevVirtualArcIds: new Set<string>(),

  /**
   * Apply highlight state to DOM. Resets only previously-styled elements, then applies state.
   * @param dom - DomAdapter instance
   * @param staticData - StaticData accessor
   * @param virtualArcUsages - Runtime virtual arc usage map
   * @param state - Highlight state from deriveHighlightState, or null to reset
   * @param dpr - Device pixels per CSS px, for the hairlines of the arcs
   */
  apply(
    dom: Dom,
    staticData: typeof StaticData,
    virtualArcUsages: Map<string, ArcData['usages']>,
    state: HighlightState | null,
    dpr = 1,
  ) {
    const C = STATIC_DATA.classes;
    this.resetToBase(dom, C, staticData, virtualArcUsages, dpr);

    if (!state) {
      this._prevNodeIds = new Set<string>();
      this._prevRegularArcIds = new Set<string>();
      this._prevVirtualArcIds = new Set<string>();
      return;
    }

    this._applyNodeClasses(dom, C, state.nodeHighlights);
    this._applyArcHighlights(dom, C, state.arcHighlights, dpr);
    this._applyArrowScaling(dom, C, state.arcHighlights);
    this._createShadows(dom, C, state.shadowData);
    this._promoteToHighlightLayers(
      dom,
      C,
      state.arcHighlights,
      state.promotedHitareas,
    );
    this._activateDimming(dom, C, state.isPinned);

    // Update dirty sets for next reset
    this._prevNodeIds = new Set(state.nodeHighlights.keys());
    this._prevRegularArcIds = new Set<string>();
    this._prevVirtualArcIds = new Set<string>();
    for (const [key, { isVirtual }] of state.arcHighlights) {
      if (isVirtual) {
        this._prevVirtualArcIds.add(key.slice(2));
      } else {
        this._prevRegularArcIds.add(key);
      }
    }
  },

  /**
   * Reset all highlight-related DOM state to base.
   * Uses data-iteration (no CSS selector queries for finding highlighted elements).
   * @param dom - DomAdapter instance
   * @param C - STATIC_DATA.classes
   * @param staticData - StaticData accessor
   * @param virtualArcUsages - Runtime virtual arc usage map
   * @param dpr - Device pixels per CSS px
   */
  resetToBase(
    dom: Dom,
    C: Classes,
    staticData: typeof StaticData,
    virtualArcUsages: Map<string, ArcData['usages']>,
    dpr = 1,
  ) {
    this._resetDimming(dom, C);
    this._resetLayers(dom);
    this._clearShadowLayer(dom);
    this._resetNodeClasses(dom, C);
    this._resetArcStyles(dom, C, staticData, dpr);
    this._resetVirtualArcStyles(dom, C, virtualArcUsages, dpr);
  },

  /**
   * Remove has-highlight class from SVG root (CSS-only dimming reset).
   */
  _resetDimming(dom: Dom, C: Classes) {
    const svg = dom.getSvgRoot();
    if (svg) {
      svg.classList.remove(C.relation.hasHighlight);
      svg.classList.remove(C.relation.hasPinned);
    }
  },

  /**
   * Add has-highlight class to SVG root (CSS-only dimming activation).
   */
  _activateDimming(dom: Dom, C: Classes, isPinned: boolean) {
    const svg = dom.getSvgRoot();
    if (svg) {
      svg.classList.add(C.relation.hasHighlight);
      if (isPinned) svg.classList.add(C.relation.hasPinned);
    }
  },

  /**
   * Move all elements from highlight layers back to base layers.
   */
  _resetLayers(dom: Dom) {
    const moveBack = (highlightId: string, baseId: string) => {
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
  _clearShadowLayer(dom: Dom) {
    LayerManager.clearLayer(LayerManager.LAYERS.SHADOWS, dom);
  },

  /**
   * Remove highlight CSS classes from previously-highlighted nodes (dirty-set).
   */
  _resetNodeClasses(dom: Dom, C: Classes) {
    const nodeClasses = [
      C.nodeSelection.selectedPackage,
      C.nodeSelection.selectedModule,
      C.nodeSelection.selectedExternal,
      C.nodeSelection.selectedExternalTransitive,
      C.nodeSelection.groupMember,
      C.nodeSelection.cycleMember,
      C.relation.depNode,
      C.relation.dependentNode,
    ];
    for (const nodeId of this._prevNodeIds) {
      const node = dom.getNode(nodeId);
      if (node) {
        for (const cls of nodeClasses) node.classList.remove(cls);
      }
    }
  },

  /**
   * Reset previously-highlighted regular arcs to base styling (dirty-set).
   */
  _resetArcStyles(
    dom: Dom,
    C: Classes,
    staticData: typeof StaticData,
    dpr: number,
  ) {
    for (const arcId of this._prevRegularArcIds) {
      const originalWidth = staticData.getArcStrokeWidth(arcId);
      const arc = dom.getArc(arcId);
      if (arc) {
        arc.classList.remove(C.relation.highlightedArc);
        ArcStroke.redraw(arc, originalWidth, dpr);
      }

      // Reset arrows (including hidden — prevents stale state after collapse/expand)
      const scale = ArcLogic.scaleFromStrokeWidth(originalWidth);
      dom.getArrows(arcId).forEach((arrow) => {
        arrow.classList.remove(C.relation.highlightedArrow);
        const points = arrow.getAttribute('points');
        if (points === null) return;
        const tip = ArcLogic.parseTipFromPoints(points);
        if (tip) {
          arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, scale));
        }
      });

      // Reset labels
      const labelGroup = dom.getLabelGroup(arcId);
      if (labelGroup) {
        const labels = labelGroup.querySelectorAll(`.${C.labels.arcCount}`);
        labels.forEach((el) => {
          el.classList.remove(C.relation.highlightedLabel);
        });
      }
    }
  },

  /**
   * Set CSS classes on highlighted nodes.
   */
  _applyNodeClasses(
    dom: Dom,
    C: Classes,
    nodeHighlights: HighlightState['nodeHighlights'],
  ) {
    const nodeSelection: Record<string, string> = C.nodeSelection;
    const relation: Record<string, string> = C.relation;
    for (const [nodeId, { cssClass }] of nodeHighlights) {
      const node = dom.getNode(nodeId);
      // A selection role (selected*, groupMember, cycleMember) or a relation
      // role (depNode, dependentNode).
      const cls = nodeSelection[cssClass] ?? relation[cssClass];
      if (node) node.classList.add(cls);
    }
  },

  /**
   * Mark highlighted arcs and draw them at their highlight width, plus label highlighting.
   * Keys: "from-to" for regular arcs, "v:from-to" for virtual arcs.
   */
  _applyArcHighlights(
    dom: Dom,
    C: Classes,
    arcHighlights: HighlightState['arcHighlights'],
    dpr: number,
  ) {
    for (const [key, { highlightWidth, isVirtual }] of arcHighlights) {
      const arcId = isVirtual ? key.slice(2) : key;

      if (isVirtual) {
        dom
          .querySelectorAll(
            `.${C.direction.virtualArc}[data-arc-id="${arcId}"]`,
          )
          .forEach((arc) => {
            arc.classList.add(C.relation.highlightedArc);
            ArcStroke.redraw(arc, highlightWidth, dpr);
          });
        dom
          .querySelectorAll(`.${C.labels.arcCount}[data-vedge="${arcId}"]`)
          .forEach((el) => {
            el.classList.add(C.relation.highlightedLabel);
          });
      } else {
        const arc = dom.getVisibleArc(arcId);
        if (arc) {
          arc.classList.add(C.relation.highlightedArc);
          ArcStroke.redraw(arc, highlightWidth, dpr);
        }
        const labelGroup = dom.getLabelGroup(arcId);
        if (labelGroup) {
          labelGroup.querySelectorAll(`.${C.labels.arcCount}`).forEach((el) => {
            el.classList.add(C.relation.highlightedLabel);
          });
        }
      }
    }
  },

  /**
   * Scale arrows to match highlight widths.
   */
  _applyArrowScaling(
    dom: Dom,
    C: Classes,
    arcHighlights: HighlightState['arcHighlights'],
  ) {
    for (const [key, { arrowScale, isVirtual }] of arcHighlights) {
      const arcId = isVirtual ? key.slice(2) : key;
      const arrows = isVirtual
        ? dom.querySelectorAll(
            `.${C.direction.virtualArrow}[data-vedge="${arcId}"]`,
          )
        : dom.getVisibleArrows(arcId);

      arrows.forEach((arrow) => {
        arrow.classList.add(C.relation.highlightedArrow);
        const points = arrow.getAttribute('points');
        if (points === null) return;
        const tip = ArcLogic.parseTipFromPoints(points);
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
   * Create shadow glow paths along the curve of each arc, with its classes
   * and pre-computed styling. A shadow stays one antialiased stroke, since
   * only highlighted arcs have one.
   */
  _createShadows(
    dom: Dom,
    C: Classes,
    shadowData: HighlightState['shadowData'],
  ) {
    const relation: Record<string, string> = C.relation;
    const shadowLayer = dom.getElementById(LayerManager.LAYERS.SHADOWS);
    if (!shadowLayer) return;

    for (const [
      key,
      { shadowWidth, visibleLength, dashOffset, glowClass, closesCycle },
    ] of shadowData) {
      const isVirtual = key.startsWith('v:');
      const arcId = isVirtual ? key.slice(2) : key;
      const arcs = isVirtual
        ? dom.querySelectorAll(
            `.${C.direction.virtualArc}[data-arc-id="${arcId}"]`,
          )
        : [dom.getVisibleArc(arcId)];

      for (const arc of arcs) {
        if (!arc) continue;
        const curve = arc.getAttribute('data-path') ?? '';
        const shadow = dom.createSvgElement('path');
        shadow.classList.add(...arc.classList);
        shadow.classList.remove(C.direction.downward, C.direction.upward);
        shadow.classList.add(C.relation.shadowPath);
        shadow.classList.add(relation[glowClass]);
        if (closesCycle) shadow.classList.add(C.relation.glowCycle);
        shadow.setAttribute('d', curve);

        const pathLength = ArcLogic.estimatePathLength(curve);
        shadow.style.strokeWidth = `${shadowWidth}px`;
        shadow.setAttribute('opacity', STATIC_DATA.theme.shadowOpacity);
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
  _promoteToHighlightLayers(
    dom: Dom,
    C: Classes,
    arcHighlights: HighlightState['arcHighlights'],
    promotedHitareas: HighlightState['promotedHitareas'],
  ) {
    for (const [key, { isVirtual }] of arcHighlights) {
      const arcId = isVirtual ? key.slice(2) : key;

      if (isVirtual) {
        dom
          .querySelectorAll(
            `.${C.direction.virtualArc}[data-arc-id="${arcId}"]`,
          )
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
   * Reset previously-highlighted virtual arcs to base styling (dirty-set + cache).
   * Uses DomAdapter cache (getArc/getArrows/getLabelGroup) instead of querySelectorAll.
   */
  _resetVirtualArcStyles(
    dom: Dom,
    C: Classes,
    virtualArcUsages: Map<string, ArcData['usages']>,
    dpr: number,
  ) {
    for (const arcId of this._prevVirtualArcIds) {
      const usages = virtualArcUsages.get(arcId);
      if (!usages) continue;
      const count = usages.reduce((sum, g) => sum + g.locations.length, 0);
      const strokeWidth = ArcLogic.calculateStrokeWidth(count);
      const scale = ArcLogic.scaleFromStrokeWidth(strokeWidth);

      // Virtual arc group (from cache)
      const arc = dom.getArc(arcId);
      if (arc) {
        arc.classList.remove(C.relation.highlightedArc);
        ArcStroke.redraw(arc, strokeWidth, dpr);
      }

      // Virtual arrows (from cache)
      dom.getArrows(arcId).forEach((arrow) => {
        arrow.classList.remove(C.relation.highlightedArrow);
        const points = arrow.getAttribute('points');
        if (points === null) return;
        const tip = ArcLogic.parseTipFromPoints(points);
        if (tip) {
          arrow.setAttribute('points', ArcLogic.getArrowPoints(tip, scale));
        }
      });

      // Virtual labels (from cache)
      const labelGroup = dom.getLabelGroup(arcId);
      if (labelGroup) {
        for (const child of labelGroup.children) {
          if (child.classList?.contains(C.labels.arcCount)) {
            child.classList.remove(C.relation.highlightedLabel);
          }
        }
      }
    }
  },
};

export { HighlightRenderer };
