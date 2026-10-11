// switch_toggles.ts - The two toolbar buttons that switch an analysis input
// (external packages, test code). A click asks the service for the opposite
// state; the button stays busy until the service reports a new page (the
// page then reloads) or an error.

type SwitchName = 'externals' | 'tests';

interface SwitchToggleHooks {
  /** Sends one command line to the service. */
  post: (line: string) => Promise<{ status: number }>;
  /** Reloads the page once a new one is ready. */
  reload: () => void;
  /** Shows a line of text in the toolbar's status span. */
  showStatus: (text: string) => void;
  /** Marks a button as waiting for the service. */
  showBusy: (name: SwitchName, busy: boolean) => void;
  /** The state the button shows for a switch. */
  isOn: (name: SwitchName) => boolean;
}

const SWITCHES = ['externals', 'tests'] as SwitchName[];

function createSwitchToggles({
  post,
  reload,
  showStatus,
  showBusy,
  isOn,
}: SwitchToggleHooks) {
  function fail(text: string) {
    for (const name of SWITCHES) showBusy(name, false);
    showStatus(text);
  }

  return {
    click(name: SwitchName) {
      showBusy(name, true);
      showStatus('Analysing…');
      post(`arc ${name} ${isOn(name) ? 'off' : 'on'}`).then(
        (response) => {
          if (response.status !== 202) fail('The service refused the command');
        },
        () => fail('The service is not reachable'),
      );
    },
    handleEvent(eventName: string, data: string) {
      if (eventName === 'analysis') reload();
      else if (eventName === 'analysis-error') fail(data);
    },
  };
}

/**
 * Posts one command line to the service, relative to the page so the port
 * is never spelled out.
 */
function postCommand(line: string) {
  return fetch('command', { method: 'POST', body: line });
}

const SwitchToggles = { createSwitchToggles, postCommand };

export { createSwitchToggles, postCommand, SwitchToggles };
