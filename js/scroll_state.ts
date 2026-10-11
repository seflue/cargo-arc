// scroll_state.ts - Whether the page is scrolling. Chromium fires scrollend
// after every wheel step, so scrolling ends only after a quiet period without
// scroll events.

const QUIET_MS = 200;

interface ScrollStateHooks {
  /** Called once when scrolling starts and once when it ends. */
  setScrolling: (scrolling: boolean) => void;
  schedule: (fn: () => void, ms: number) => unknown;
  cancel: (handle: unknown) => void;
}

function createScrollState({
  setScrolling,
  schedule,
  cancel,
}: ScrollStateHooks) {
  let scrolling = false;
  let quiet: unknown = null;
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
