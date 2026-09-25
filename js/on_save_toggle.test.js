import { describe, expect, mock, test } from 'bun:test';
import { createOnSaveToggle } from './on_save_toggle.js';

/** Builds the toggle over mocks; `on` is the state the button shows. */
function setup({ on = true, status = 202 } = {}) {
  const post = mock(() => Promise.resolve({ status }));
  const showState = mock();
  const showStatus = mock();
  const toggle = createOnSaveToggle({
    post,
    showState,
    showStatus,
    isOn: () => on,
  });
  return { toggle, post, showState, showStatus };
}

/** Lets the promise chain behind a click settle. */
const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('createOnSaveToggle', () => {
  test('a click sends the opposite state and leaves the button to the event', async () => {
    const { toggle, post, showState } = setup({ on: true });
    toggle.click();
    expect(post).toHaveBeenCalledWith('arc on-save off');
    await settled();
    expect(showState).not.toHaveBeenCalled();
  });

  test('a click on a button that is off sends on', () => {
    const { toggle, post } = setup({ on: false });
    toggle.click();
    expect(post).toHaveBeenCalledWith('arc on-save on');
  });

  test('an on-save event sets the button', () => {
    const { toggle, showState } = setup();
    toggle.handleEvent('on-save', 'off');
    toggle.handleEvent('on-save', 'on');
    expect(showState.mock.calls).toEqual([[false], [true]]);
  });

  test('a refused command says so', async () => {
    const { toggle, showStatus } = setup({ status: 400 });
    toggle.click();
    await settled();
    expect(showStatus).toHaveBeenLastCalledWith(
      'The service refused the command',
    );
  });

  test('a failed request says so', async () => {
    const showStatus = mock();
    const toggle = createOnSaveToggle({
      post: mock(() => Promise.reject(new Error('gone'))),
      showState: mock(),
      showStatus,
      isOn: () => true,
    });
    toggle.click();
    await settled();
    expect(showStatus).toHaveBeenLastCalledWith('The service is not reachable');
  });

  test('other events and unknown states are ignored', () => {
    const { toggle, showState } = setup();
    toggle.handleEvent('analysis', 'externals=on tests=off');
    toggle.handleEvent('on-save', 'maybe');
    expect(showState).not.toHaveBeenCalled();
  });
});
