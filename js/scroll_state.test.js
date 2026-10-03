import { describe, expect, mock, test } from 'bun:test';
import { createScrollState } from './scroll_state.js';

/** Builds the state over a manual timer; `fire()` runs the pending callback. */
function setup() {
  const setScrolling = mock();
  let pending = null;
  const schedule = mock((fn) => {
    pending = fn;
    return 1;
  });
  const cancel = mock(() => {
    pending = null;
  });
  const state = createScrollState({ setScrolling, schedule, cancel });
  const fire = () => {
    const fn = pending;
    pending = null;
    fn?.();
  };
  return { state, setScrolling, schedule, fire };
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
