// on_save_toggle.ts - The toolbar button that turns recomputing on save on
// or off. The service holds the state: a click asks it for the opposite, and
// the button shows what the service's on-save event reports, which every
// page hears, the clicking one included.

interface OnSaveToggleHooks {
  /** Sends one command line to the service. */
  post: (line: string) => Promise<{ status: number }>;
  /** Reflects the state on the button. */
  showState: (on: boolean) => void;
  /** Shows a line of text in the toolbar's status span. */
  showStatus: (text: string) => void;
  /** The state the button shows. */
  isOn: () => boolean;
}

function createOnSaveToggle({
  post,
  showState,
  showStatus,
  isOn,
}: OnSaveToggleHooks) {
  return {
    click() {
      post(`arc on-save ${isOn() ? 'off' : 'on'}`).then(
        (response) => {
          if (response.status !== 202)
            showStatus('The service refused the command');
        },
        () => showStatus('The service is not reachable'),
      );
    },
    handleEvent(eventName: string, data: string) {
      if (eventName !== 'on-save') return;
      if (data === 'on') showState(true);
      else if (data === 'off') showState(false);
    },
  };
}

const OnSaveToggle = { createOnSaveToggle };

export { createOnSaveToggle, OnSaveToggle };
