import { describe, expect, mock, test } from 'bun:test';
import { createScrollState } from './scroll_state.js';

/**
 * Builds the state over a manual timer, clock and scroll position; `fire()`
 * runs the pending callback, `scrollTo(y, ms)` moves the page to `y` at
 * `ms` and fires a scroll event. The window is 800 px tall.
 */
function setup() {
  const setScrolling = mock();
  const setFast = mock();
  let pending = null;
  const schedule = mock((fn) => {
    pending = fn;
    return 1;
  });
  const cancel = mock(() => {
    pending = null;
  });
  let y = 0;
  let time = 0;
  const state = createScrollState({
    setScrolling,
    setFast,
    schedule,
    cancel,
    position: () => ({ x: 0, y }),
    now: () => time,
    viewportHeight: () => 800,
  });
  const fire = () => {
    const fn = pending;
    pending = null;
    fn?.();
  };
  const scrollTo = (toY, ms) => {
    y = toY;
    time = ms;
    state.onScroll();
  };
  return { state, setScrolling, setFast, schedule, fire, scrollTo };
}

describe('createScrollState', () => {
  test('the first scroll event marks scrolling once', () => {
    const { state, setScrolling } = setup();
    state.onScroll();
    state.onScroll();
    state.onScroll();
    expect(setScrolling.mock.calls).toEqual([[true]]);
  });

  test('a quiet period after the last scroll event ends scrolling', () => {
    const { state, setScrolling, fire } = setup();
    state.onScroll();
    fire();
    expect(setScrolling.mock.calls).toEqual([[true], [false]]);
  });

  test('each scroll event restarts the quiet period', () => {
    const { state, schedule, setScrolling } = setup();
    state.onScroll();
    state.onScroll();
    expect(schedule).toHaveBeenCalledTimes(2);
    expect(schedule.mock.calls[1][1]).toBe(200);
    expect(setScrolling).not.toHaveBeenCalledWith(false);
  });

  test('scrolling again after the quiet period marks it again', () => {
    const { state, setScrolling, fire } = setup();
    state.onScroll();
    fire();
    state.onScroll();
    expect(setScrolling.mock.calls).toEqual([[true], [false], [true]]);
  });
});

describe('fast scrolling', () => {
  test('slow scrolling never switches to fast', () => {
    const { setFast, scrollTo, fire } = setup();
    // 100 px per 50 ms: 2,000 px/s.
    for (let i = 1; i <= 20; i++) scrollTo(i * 100, i * 50);
    fire();
    expect(setFast).not.toHaveBeenCalled();
  });

  test('scrolling faster than 3,000 px/s switches to fast once, until the quiet period ends', () => {
    const { setFast, setScrolling, scrollTo, fire } = setup();
    scrollTo(100, 50);
    // 200 px in 50 ms: 4,000 px/s.
    scrollTo(300, 100);
    scrollTo(500, 150);
    // Slow again: stays fast.
    scrollTo(510, 200);
    expect(setFast.mock.calls).toEqual([[true]]);

    fire();
    expect(setFast.mock.calls).toEqual([[true], [false]]);
    expect(setScrolling.mock.calls).toEqual([[true], [false]]);
  });

  test('a jump over more than one and a half window heights switches to fast', () => {
    const { setFast, scrollTo } = setup();
    // 1,201 px after a long pause: slow, but far.
    scrollTo(1201, 10000);
    expect(setFast.mock.calls).toEqual([[true]]);
  });

  test('a jump of one and a half window heights stays slow', () => {
    const { setFast, scrollTo } = setup();
    scrollTo(1200, 10000);
    expect(setFast).not.toHaveBeenCalled();
  });

  test('two events at the same moment count as one millisecond apart', () => {
    const { setFast, scrollTo } = setup();
    scrollTo(2, 0);
    // 4 px in under a millisecond: 4,000 px/s.
    scrollTo(6, 0);
    expect(setFast.mock.calls).toEqual([[true]]);
  });
});
