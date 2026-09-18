import { describe, expect, mock, test } from 'bun:test';
import { createSwitchToggles } from './switch_toggles.js';

/** Builds the toggles over mocks; `on` is the current state per switch. */
function setup({ on = { externals: false, tests: false }, status = 202 } = {}) {
  const post = mock(() => Promise.resolve({ status }));
  const reload = mock();
  const showStatus = mock();
  const showBusy = mock();
  const toggles = createSwitchToggles({
    post,
    reload,
    showStatus,
    showBusy,
    isOn: (name) => on[name],
  });
  return { toggles, post, reload, showStatus, showBusy };
}

/** Lets the promise chain behind a click settle. */
const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('createSwitchToggles', () => {
  test('a click sends the opposite command and marks the switch busy', async () => {
    const { toggles, post, showBusy, showStatus } = setup();
    toggles.click('externals');
    expect(post).toHaveBeenCalledWith('arc externals on');
    expect(showBusy).toHaveBeenCalledWith('externals', true);
    expect(showStatus).toHaveBeenCalledWith('Analysing…');
    await settled();
    expect(showBusy).toHaveBeenCalledTimes(1);
  });

  test('a click on a switch that is on sends off', () => {
    const { toggles, post } = setup({ on: { externals: true, tests: true } });
    toggles.click('tests');
    expect(post).toHaveBeenCalledWith('arc tests off');
  });

  test('a refused command frees the switch and says so', async () => {
    const { toggles, showBusy, showStatus, reload } = setup({ status: 400 });
    toggles.click('tests');
    await settled();
    expect(showBusy).toHaveBeenCalledWith('tests', false);
    expect(showStatus).toHaveBeenLastCalledWith(
      'The service refused the command',
    );
    expect(reload).not.toHaveBeenCalled();
  });

  test('a failed request frees the switch and says so', async () => {
    const post = mock(() => Promise.reject(new Error('gone')));
    const showBusy = mock();
    const showStatus = mock();
    const toggles = createSwitchToggles({
      post,
      reload: mock(),
      showStatus,
      showBusy,
      isOn: () => false,
    });
    toggles.click('externals');
    await settled();
    expect(showBusy).toHaveBeenCalledWith('externals', false);
    expect(showStatus).toHaveBeenLastCalledWith('The service is not reachable');
  });

  test('an analysis event reloads the page', () => {
    const { toggles, reload } = setup();
    toggles.handleEvent('analysis', 'externals=on tests=off');
    expect(reload).toHaveBeenCalledTimes(1);
  });

  test('an analysis-error event frees both switches and shows the text', () => {
    const { toggles, showBusy, showStatus, reload } = setup();
    toggles.click('externals');
    toggles.handleEvent('analysis-error', 'analysis failed: no manifest');
    expect(showBusy).toHaveBeenCalledWith('externals', false);
    expect(showBusy).toHaveBeenCalledWith('tests', false);
    expect(showStatus).toHaveBeenLastCalledWith('analysis failed: no manifest');
    expect(reload).not.toHaveBeenCalled();
  });

  test('other events are ignored', () => {
    const { toggles, reload, showBusy, showStatus } = setup();
    toggles.handleEvent('focus', '{"node":"1","jumps":[]}');
    expect(reload).not.toHaveBeenCalled();
    expect(showBusy).not.toHaveBeenCalled();
    expect(showStatus).not.toHaveBeenCalled();
  });
});
