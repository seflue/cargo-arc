// @module RowState
// @deps
// @config
// row_state.js - Transition table for a tangle sidebar edge row's two bits
// (expanded/collapsed, pinned/unpinned). Pure lookup, no state of its own;
// AppState reads a row's current state from expandedRows + the pin slot,
// looks up the event here, and writes the result back.

/** @typedef {'CU'|'EU'|'CP'|'EP'} RowStateName */

/**
 * @type {Record<RowStateName, Record<
 *   'toggle'|'pinClick'|'pinned'|'unpinned'|'expandAll'|'collapseAll'|'follow',
 *   () => RowStateName
 * >>}
 */
const RowState = {
  CU: {
    toggle: () => 'EU',
    pinClick: () => 'EP',
    pinned: () => 'EP',
    unpinned: () => 'CU',
    expandAll: () => 'EU',
    collapseAll: () => 'CU',
    follow: () => 'EU',
  },
  EU: {
    toggle: () => 'CU',
    pinClick: () => 'EP',
    pinned: () => 'EP',
    unpinned: () => 'EU',
    expandAll: () => 'EU',
    collapseAll: () => 'CU',
    follow: () => 'EU',
  },
  // pinClick on CP goes to CU, not EU: unpinning never changes expansion,
  // the same rule the table already gives EP.
  CP: {
    toggle: () => 'EP',
    pinClick: () => 'CU',
    pinned: () => 'CP',
    unpinned: () => 'CU',
    expandAll: () => 'EP',
    collapseAll: () => 'CP',
    follow: () => 'EP',
  },
  EP: {
    toggle: () => 'CP',
    pinClick: () => 'EU',
    pinned: () => 'EP',
    unpinned: () => 'EU',
    expandAll: () => 'EP',
    collapseAll: () => 'CP',
    follow: () => 'EP',
  },
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { RowState };
}
