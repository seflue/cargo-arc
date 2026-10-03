// scroll_state.js - Whether the page is scrolling. Chromium fires scrollend
// after every wheel step, so scrolling ends only after a quiet period without
// scroll events.

const QUIET_MS = 200;

/**
 * @typedef {object} ScrollStateHooks
 * @property {(scrolling: boolean) => void} setScrolling - called once when
 *   scrolling starts and once when it ends.
 * @property {(fn: () => void, ms: number) => unknown} schedule
 * @property {(handle: unknown) => void} cancel
 */

/**
 * @param {ScrollStateHooks} hooks
 */
function createScrollState({ setScrolling, schedule, cancel }) {
  let scrolling = false;
  /** @type {unknown} */
  let quiet = null;
  return {
    onScroll() {
      if (!scrolling) {
        scrolling = true;
        setScrolling(true);
      }
      if (quiet !== null) cancel(quiet);
      quiet = schedule(() => {
        quiet = null;
        scrolling = false;
        setScrolling(false);
      }, QUIET_MS);
    },
  };
}

const ScrollState = { createScrollState };

export { createScrollState, ScrollState };
