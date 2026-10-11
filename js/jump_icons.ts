import { DomAdapter } from './dom_adapter.js';
import { JumpSymbol } from './jump_symbol.js';

// jump_icons.ts - Builds, positions, and times out the jump popover shown
// next to a hovered node. The `#jump-icon` symbol it draws the chips'
// popover next to is defined in `jump_symbol.ts`, shared with the sidebar
// rows and the hotspot map's own single icon.

/** Chip text per target kind (STATIC_DATA `targets[i].kind`). */
const CHIP_LABELS: Record<string, string> = {
  lib: 'lib',
  bin: 'bin',
  manifest: 'toml',
  module: 'mod',
};
/** Transparent strip between the node's edge and the popover, in SVG units. */
const BRIDGE_WIDTH = 4;
/** Padding inside the popover background, in SVG units. */
const POPOVER_PAD = 4;
/** Chip box height, in SVG units. */
const CHIP_HEIGHT = 16;
/** Horizontal padding inside a chip, in SVG units. */
const CHIP_PAD = 5;
/** Space between chips, in SVG units. */
const CHIP_GAP = 4;
/** Advance width of one chip character at the 10px monospace label size. */
const CHAR_WIDTH = 6;

type JumpTarget = { kind: string; name: string; jump: number };

/**
 * @param deps - `setTimeout`/`clearTimeout` are injected so tests control the
 *   hide timer without waiting on it.
 */
function createJumpIcons({
  layer,
  defsHost,
  onJump,
  onEnter,
  onLeave,
  setTimeout,
  clearTimeout,
  grace,
}: {
  layer: Element;
  defsHost: Element;
  onJump: (id: number) => void;
  onEnter: (nodeId: string) => void;
  onLeave: () => void;
  setTimeout: typeof globalThis.setTimeout;
  clearTimeout: typeof globalThis.clearTimeout;
  grace: number;
}) {
  let group: SVGElement | null = null;
  let hideTimer: number | undefined;

  JumpSymbol.defineJumpSymbol(defsHost);

  function removeGroup() {
    if (group) {
      layer.removeChild(group);
      group = null;
    }
  }

  function cancelHide() {
    clearTimeout(hideTimer);
    hideTimer = undefined;
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      hideTimer = undefined;
      removeGroup();
    }, grace);
  }

  function hide() {
    cancelHide();
    removeGroup();
  }

  function chipLabel(target: JumpTarget) {
    return CHIP_LABELS[target.kind] ?? target.kind;
  }

  function chipWidth(target: JumpTarget) {
    return chipLabel(target).length * CHAR_WIDTH + 2 * CHIP_PAD;
  }

  /**
   * One clickable chip: rounded box, kind label, target path as tooltip.
   */
  function buildChip(target: JumpTarget, x: number, y: number) {
    const chip = DomAdapter.createSvgElement('g');
    chip.setAttribute('class', 'jump-chip');
    const width = chipWidth(target);

    const box = DomAdapter.createSvgElement('rect');
    box.setAttribute('class', 'jump-popover jump-chip-bg');
    box.setAttribute('x', String(x));
    box.setAttribute('y', String(y));
    box.setAttribute('width', String(width));
    box.setAttribute('height', String(CHIP_HEIGHT));
    chip.appendChild(box);

    const label = DomAdapter.createSvgElement('text');
    label.setAttribute('class', 'jump-chip-label');
    label.setAttribute('x', String(x + width / 2));
    label.setAttribute('y', String(y + CHIP_HEIGHT / 2));
    label.textContent = chipLabel(target);
    chip.appendChild(label);

    const title = DomAdapter.createSvgElement('title');
    title.textContent = target.name;
    chip.appendChild(title);

    chip.addEventListener('click', (e) => {
      e.stopPropagation();
      onJump(target.jump);
    });
    return chip;
  }

  // Builds the popover to the right of the node rect: a transparent bridge
  // strip from the rect's edge, then a background with one chip per target.
  // The layer sits above every arc and hit-area layer, so the pointer reaches
  // the popover and its mouseenter cancels the hide the node's mouseleave
  // scheduled.
  function show(nodeId: string, rect: Element, targets: JumpTarget[]) {
    cancelHide();
    removeGroup();

    const x = parseFloat(rect.getAttribute('x') ?? '');
    const y = parseFloat(rect.getAttribute('y') ?? '');
    const width = parseFloat(rect.getAttribute('width') ?? '');
    const height = parseFloat(rect.getAttribute('height') ?? '');
    const chipsWidth =
      targets.reduce((sum, t) => sum + chipWidth(t), 0) +
      (targets.length - 1) * CHIP_GAP;
    const bgX = x + width + BRIDGE_WIDTH;
    const bgWidth = chipsWidth + 2 * POPOVER_PAD;

    const g = DomAdapter.createSvgElement('g');
    g.setAttribute('class', 'jump-popover');

    const bridge = DomAdapter.createSvgElement('rect');
    bridge.setAttribute('class', 'jump-popover jump-popover-bridge');
    bridge.setAttribute('x', String(x + width));
    bridge.setAttribute('y', String(y));
    bridge.setAttribute('width', String(BRIDGE_WIDTH));
    bridge.setAttribute('height', String(height));
    g.appendChild(bridge);

    const bg = DomAdapter.createSvgElement('rect');
    bg.setAttribute('class', 'jump-popover jump-popover-bg');
    bg.setAttribute('x', String(bgX));
    bg.setAttribute('y', String(y));
    bg.setAttribute('width', String(bgWidth));
    bg.setAttribute('height', String(height));
    g.appendChild(bg);

    const chipY = y + (height - CHIP_HEIGHT) / 2;
    let chipX = bgX + POPOVER_PAD;
    for (const target of targets) {
      g.appendChild(buildChip(target, chipX, chipY));
      chipX += chipWidth(target) + CHIP_GAP;
    }

    g.addEventListener('mouseenter', () => {
      cancelHide();
      onEnter(nodeId);
    });
    g.addEventListener('mouseleave', () => {
      scheduleHide();
      onLeave();
    });

    layer.appendChild(g);
    group = g;
  }

  return { show, scheduleHide, cancelHide, hide };
}

const JumpIcons = { createJumpIcons };

export { createJumpIcons, JumpIcons };
