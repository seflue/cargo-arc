// @module OnSaveToggle
// @deps
// @config
// on_save_toggle.js - The toolbar button that turns recomputing on save on
// or off. The service holds the state: a click asks it for the opposite, and
// the button shows what the service's on-save event reports, which every
// page hears, the clicking one included.

/**
 * @typedef {object} OnSaveToggleHooks
 * @property {(line: string) => Promise<{ status: number }>} post - sends
 *   one command line to the service.
 * @property {(on: boolean) => void} showState - reflects the state on the
 *   button.
 * @property {(text: string) => void} showStatus - shows a line of text in
 *   the toolbar's status span.
 * @property {() => boolean} isOn - the state the button shows.
 */

/**
 * @param {OnSaveToggleHooks} hooks
 */
function createOnSaveToggle({ post, showState, showStatus, isOn }) {
  return {
    click() {
      post(`arc on-save ${isOn() ? 'off' : 'on'}`).then(
        (response) => {
          if (response.status !== 202)
            showStatus('The service refused the command');
        },
        () => showStatus('The service is not reachable'),
      );
    },
    /**
     * @param {string} eventName
     * @param {string} data
     */
    handleEvent(eventName, data) {
      if (eventName !== 'on-save') return;
      if (data === 'on') showState(true);
      else if (data === 'off') showState(false);
    },
  };
}

// The browser global exposing this module's API.
const OnSaveToggle = { createOnSaveToggle };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { createOnSaveToggle, OnSaveToggle };
}
