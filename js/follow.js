// @module Follow
// @deps
// @config
// follow.js - Receives the service's focus and follow events and applies a
// focus to the page while following is on.

/**
 * @typedef {object} FollowHooks
 * @property {(handler: (eventName: string, data: string) => void) => void} connect -
 *   opens the event stream and forwards each event to the handler; tests
 *   inject a mock so no connection happens.
 * @property {(node: string, jumps: number[]) => void} apply - selects the
 *   node in the page and opens the sidebar rows of the given jump ids.
 * @property {(enabled: boolean) => void} showState - reflects the follow
 *   state on the toolbar button.
 */

/**
 * @param {FollowHooks} hooks
 */
function createFollow({ connect, apply, showState }) {
  let enabled = true;

  function setEnabled(on) {
    enabled = on;
    showState(enabled);
  }

  /** @param {string} data */
  function handleFocus(data) {
    if (!enabled) return;
    let event;
    try {
      event = JSON.parse(data);
    } catch {
      return;
    }
    if (
      !event ||
      typeof event.node !== 'string' ||
      !Array.isArray(event.jumps)
    ) {
      return;
    }
    apply(event.node, event.jumps);
  }

  /** @param {string} data */
  function handleFollow(data) {
    if (data === 'on') setEnabled(true);
    else if (data === 'off') setEnabled(false);
  }

  return {
    /** Opens the stream and shows the initial state. */
    start() {
      showState(enabled);
      connect((eventName, data) => {
        if (eventName === 'focus') handleFocus(data);
        else if (eventName === 'follow') handleFollow(data);
      });
    },
    setEnabled,
    isEnabled() {
      return enabled;
    },
  };
}

/**
 * Connects to the service's event stream, relative to the page so the port
 * is never spelled out, and forwards every event the service sends.
 * `EventSource` reconnects by itself.
 * @param {(eventName: string, data: string) => void} handler
 */
function connectEventSource(handler) {
  const source = new EventSource('events');
  for (const name of [
    'focus',
    'follow',
    'theme',
    'analysis',
    'analysis-error',
  ]) {
    source.addEventListener(name, (event) => {
      handler(name, /** @type {MessageEvent} */ (event).data);
    });
  }
}

// The browser global exposing this module's API.
const Follow = { createFollow, connectEventSource };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { createFollow, connectEventSource, Follow };
}
