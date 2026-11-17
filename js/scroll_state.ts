// scroll_state.ts - Whether the page is scrolling, and whether fast. Chromium
// fires scrollend after every wheel step, so scrolling ends only after a quiet
// period without scroll events.

const QUIET_MS = 200;
// Faster than this, or a jump farther than FAST_JUMP window heights, is fast.
const FAST_PX_PER_S = 3000;
const FAST_JUMP = 1.5;

interface ScrollStateHooks {
  /** Called once when scrolling starts and once when it ends. */
  setScrolling: (scrolling: boolean) => void;
  /** Called once when scrolling turns fast and once when that scrolling ends. */
  setFast: (fast: boolean) => void;
  schedule: (fn: () => void, ms: number) => unknown;
  cancel: (handle: unknown) => void;
  position: () => { x: number; y: number };
  /** Milliseconds. */
  now: () => number;
  viewportHeight: () => number;
}

function createScrollState({
  setScrolling,
  setFast,
  schedule,
  cancel,
  position,
  now,
  viewportHeight,
}: ScrollStateHooks) {
  let scrolling = false;
  let fast = false;
  let quiet: unknown = null;
  let last = { ...position(), time: now() };
  return {
    onScroll() {
      if (!scrolling) {
        scrolling = true;
        setScrolling(true);
      }
      const here = { ...position(), time: now() };
      const distance = Math.hypot(here.x - last.x, here.y - last.y);
      const elapsed = Math.max(1, here.time - last.time);
      last = here;
      if (
        !fast &&
        (distance > FAST_JUMP * viewportHeight() ||
          (distance / elapsed) * 1000 > FAST_PX_PER_S)
      ) {
        fast = true;
        setFast(true);
      }
      if (quiet !== null) cancel(quiet);
      quiet = schedule(() => {
        quiet = null;
        scrolling = false;
        setScrolling(false);
        if (fast) {
          fast = false;
          setFast(false);
        }
      }, QUIET_MS);
    },
  };
}

const ScrollState = { createScrollState };

export { createScrollState, ScrollState };
