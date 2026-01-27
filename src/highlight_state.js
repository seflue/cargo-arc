// highlight_state.js - State management for highlight/selection
// No DOM dependencies - pure state object with Maps

const HighlightState = {
  /**
   * Create new HighlightState
   * @returns {{pinned: null|{type: string, id: string}, originalValues: Map<string, Object>}}
   */
  create() {
    return {
      pinned: null,              // {type: 'node'|'edge', id: string} or null
      originalValues: new Map()  // arcId -> {strokeWidth, scale, tipX, tipY}
    };
  },

  /**
   * Get current pinned highlight
   * @param {Object} state
   * @returns {null|{type: string, id: string}}
   */
  getPinned(state) {
    return state.pinned;
  },

  /**
   * Set pinned highlight
   * @param {Object} state
   * @param {string} type - 'node' or 'edge'
   * @param {string} id - node or edge id
   */
  setPinned(state, type, id) {
    state.pinned = { type, id };
  },

  /**
   * Clear pinned highlight
   * @param {Object} state
   */
  clearPinned(state) {
    state.pinned = null;
  },

  /**
   * Check if specific element is pinned
   * @param {Object} state
   * @param {string} type
   * @param {string} id
   * @returns {boolean}
   */
  isPinned(state, type, id) {
    return state.pinned !== null &&
           state.pinned.type === type &&
           state.pinned.id === id;
  },

  /**
   * Toggle pinned state for element
   * If same element is pinned, unpins it. Otherwise pins new element.
   * @param {Object} state
   * @param {string} type
   * @param {string} id
   * @returns {boolean} - true if newly pinned, false if unpinned
   */
  togglePinned(state, type, id) {
    if (this.isPinned(state, type, id)) {
      this.clearPinned(state);
      return false;
    }
    this.setPinned(state, type, id);
    return true;
  },

  /**
   * Store original values for an arc (only if not already stored)
   * This prevents the "growing arrow" bug where values accumulate on repeated hover.
   * @param {Object} state
   * @param {string} arcId
   * @param {Object} values - {strokeWidth, scale, tipX, tipY}
   */
  storeOriginal(state, arcId, values) {
    if (!state.originalValues.has(arcId)) {
      state.originalValues.set(arcId, values);
    }
  },

  /**
   * Get stored original values for an arc
   * @param {Object} state
   * @param {string} arcId
   * @returns {Object|undefined}
   */
  getOriginal(state, arcId) {
    return state.originalValues.get(arcId);
  },

  /**
   * Check if original values exist for an arc
   * @param {Object} state
   * @param {string} arcId
   * @returns {boolean}
   */
  hasOriginal(state, arcId) {
    return state.originalValues.has(arcId);
  },

  /**
   * Clear all stored original values
   * @param {Object} state
   */
  clearAllOriginals(state) {
    state.originalValues.clear();
  },

  /**
   * Iterate over all stored original values
   * @param {Object} state
   * @param {function(string, Object): void} callback - (arcId, values) => void
   */
  forEachOriginal(state, callback) {
    state.originalValues.forEach((values, arcId) => {
      callback(arcId, values);
    });
  }
};

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { HighlightState };
}
