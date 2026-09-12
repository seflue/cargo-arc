// @module JumpIcons
// @deps DomAdapter
// @config
// jump_icons.js - Builds, positions, and times out the jump popover shown
// next to a hovered node, and defines the #jump-icon symbol the sidebar
// rows reuse.

/** Chip text per target kind (STATIC_DATA `targets[i].kind`). */
const CHIP_LABELS = { lib: 'lib', bin: 'bin', manifest: 'toml', module: 'mod' };
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

/**
 * @param {{
 *   layer: Element,
 *   defsHost: Element,
 *   onJump: (id: number) => void,
 *   onEnter: (nodeId: string) => void,
 *   onLeave: () => void,
 *   setTimeout: typeof setTimeout,
 *   clearTimeout: typeof clearTimeout,
 *   grace: number,
 * }} deps - `setTimeout`/`clearTimeout` are injected so tests control the
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
}) {
  let group = null;
  let hideTimer = null;

  // Defines the shared <use> target: a box with an arrow pointing out of its
  // top-right corner. The sidebar rows reference it too, so it exists from
  // construction on, not from the first hover.
  function defineSymbol() {
    const defs = DomAdapter.createSvgElement('defs');
    const symbol = DomAdapter.createSvgElement('symbol');
    symbol.setAttribute('id', 'jump-icon');
    symbol.setAttribute('viewBox', '0 0 16 16');
    const path = DomAdapter.createSvgElement('path');
    path.setAttribute(
      'd',
      'M2 5a2 2 0 0 1 2-2h3v2H4v7h7V9h2v3a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5z' +
        'M9 2h5v5h-2V5.4l-5 5-1.4-1.4 5-5H9z',
    );
    symbol.appendChild(path);
    defs.appendChild(symbol);
    defsHost.appendChild(defs);
  }

  defineSymbol();

  function removeGroup() {
    if (group) {
      layer.removeChild(group);
      group = null;
    }
  }

  function cancelHide() {
    clearTimeout(hideTimer);
    hideTimer = null;
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      hideTimer = null;
      removeGroup();
    }, grace);
  }

  function hide() {
    cancelHide();
    removeGroup();
  }

  /** @param {{ kind: string, name: string, jump: number }} target */
  function chipLabel(target) {
    return CHIP_LABELS[target.kind] ?? target.kind;
  }

  /** @param {{ kind: string, name: string, jump: number }} target */
  function chipWidth(target) {
    return chipLabel(target).length * CHAR_WIDTH + 2 * CHIP_PAD;
  }

  /**
   * One clickable chip: rounded box, kind label, target path as tooltip.
   * @param {{ kind: string, name: string, jump: number }} target
   * @param {number} x
   * @param {number} y
   */
  function buildChip(target, x, y) {
    const chip = DomAdapter.createSvgElement('g');
    chip.setAttribute('class', 'jump-chip');
    const width = chipWidth(target);

    const box = DomAdapter.createSvgElement('rect');
    box.setAttribute('class', 'jump-popover jump-chip-bg');
    box.setAttribute('x', x);
    box.setAttribute('y', y);
    box.setAttribute('width', width);
    box.setAttribute('height', CHIP_HEIGHT);
    chip.appendChild(box);

    const label = DomAdapter.createSvgElement('text');
    label.setAttribute('class', 'jump-chip-label');
    label.setAttribute('x', x + width / 2);
    label.setAttribute('y', y + CHIP_HEIGHT / 2);
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
  function show(nodeId, rect, targets) {
    cancelHide();
    removeGroup();

    const x = parseFloat(rect.getAttribute('x'));
    const y = parseFloat(rect.getAttribute('y'));
    const width = parseFloat(rect.getAttribute('width'));
    const height = parseFloat(rect.getAttribute('height'));
    const chipsWidth =
      targets.reduce((sum, t) => sum + chipWidth(t), 0) +
      (targets.length - 1) * CHIP_GAP;
    const bgX = x + width + BRIDGE_WIDTH;
    const bgWidth = chipsWidth + 2 * POPOVER_PAD;

    const g = DomAdapter.createSvgElement('g');
    g.setAttribute('class', 'jump-popover');

    const bridge = DomAdapter.createSvgElement('rect');
    bridge.setAttribute('class', 'jump-popover jump-popover-bridge');
    bridge.setAttribute('x', x + width);
    bridge.setAttribute('y', y);
    bridge.setAttribute('width', BRIDGE_WIDTH);
    bridge.setAttribute('height', height);
    g.appendChild(bridge);

    const bg = DomAdapter.createSvgElement('rect');
    bg.setAttribute('class', 'jump-popover jump-popover-bg');
    bg.setAttribute('x', bgX);
    bg.setAttribute('y', y);
    bg.setAttribute('width', bgWidth);
    bg.setAttribute('height', height);
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

// The browser global exposing this module's API.
const JumpIcons = { createJumpIcons };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { createJumpIcons, JumpIcons };
}
