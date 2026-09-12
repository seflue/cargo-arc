// @module Jump
// @deps
// @config
// jump.js - Requests a jump target and reports the outcome via a callback.

/** Shown when the service answers 404: the id belongs to an earlier layout. */
const STALE_MESSAGE = 'This diagram is from an earlier run. Reload the page.';

/**
 * @param {(url: string) => Promise<Response>} request - issues the HTTP GET;
 *   tests inject a mock so no network call happens.
 * @param {(text: string) => void} showMessage - displays (or clears, with
 *   an empty string) the jump status message.
 */
function createJump(request, showMessage) {
  return {
    /**
     * Ask the service to jump to the given id.
     * @param {number} id
     * @returns {Promise<void>} resolves once the outcome has been reported,
     *   so tests can await it.
     */
    jump(id) {
      return request(`jump?id=${id}`).then(
        (response) => {
          if (response.ok) {
            showMessage('');
          } else if (response.status === 404) {
            showMessage(STALE_MESSAGE);
          } else {
            showMessage(`Jump failed (HTTP ${response.status}).`);
          }
        },
        () => {
          showMessage('Jump service unreachable.');
        },
      );
    },
  };
}

// The browser global exposing this module's API.
const Jump = { createJump, STALE_MESSAGE };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { createJump, STALE_MESSAGE, Jump };
}
