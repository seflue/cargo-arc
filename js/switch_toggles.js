// @module SwitchToggles
// @deps
// @config
// switch_toggles.js - The two toolbar buttons that switch an analysis input
// (external crates, test code). A click asks the service for the opposite
// state; the button stays busy until the service reports a new page (the
// page then reloads) or an error.

/** @typedef {'externals' | 'tests'} SwitchName */

/**
 * @typedef {object} SwitchToggleHooks
 * @property {(line: string) => Promise<{ status: number }>} post - sends
 *   one command line to the service.
 * @property {() => void} reload - reloads the page once a new one is ready.
 * @property {(text: string) => void} showStatus - shows a line of text in
 *   the toolbar's status span.
 * @property {(name: SwitchName, busy: boolean) => void} showBusy - marks a
 *   button as waiting for the service.
 * @property {(name: SwitchName) => boolean} isOn - the state the button
 *   shows for a switch.
 */

const SWITCHES = /** @type {SwitchName[]} */ (['externals', 'tests']);

/**
 * @param {SwitchToggleHooks} hooks
 */
function createSwitchToggles({ post, reload, showStatus, showBusy, isOn }) {
  /** @param {string} text */
  function fail(text) {
    for (const name of SWITCHES) showBusy(name, false);
    showStatus(text);
  }

  return {
    /** @param {SwitchName} name */
    click(name) {
      showBusy(name, true);
      showStatus('Analysing…');
      post(`arc ${name} ${isOn(name) ? 'off' : 'on'}`).then(
        (response) => {
          if (response.status !== 202) fail('The service refused the command');
        },
        () => fail('The service is not reachable'),
      );
    },
    /**
     * @param {string} eventName
     * @param {string} data
     */
    handleEvent(eventName, data) {
      if (eventName === 'analysis') reload();
      else if (eventName === 'analysis-error') fail(data);
    },
  };
}

/**
 * Posts one command line to the service, relative to the page so the port
 * is never spelled out.
 * @param {string} line
 */
function postCommand(line) {
  return fetch('command', { method: 'POST', body: line });
}

// The browser global exposing this module's API.
const SwitchToggles = { createSwitchToggles, postCommand };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { createSwitchToggles, postCommand, SwitchToggles };
}
