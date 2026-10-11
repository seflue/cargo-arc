// jump.ts - Requests a jump target and reports the outcome via a callback.

/** Shown when the service answers 404: the id belongs to an earlier layout. */
const STALE_MESSAGE = 'This diagram is from an earlier run. Reload the page.';

/**
 * @param request - issues the HTTP GET;
 *   tests inject a mock so no network call happens.
 * @param showMessage - displays (or clears, with
 *   an empty string) the jump status message.
 */
function createJump(
  request: (url: string) => Promise<Response>,
  showMessage: (text: string) => void,
) {
  return {
    /**
     * Ask the service to jump to the given id.
     * @returns resolves once the outcome has been reported,
     *   so tests can await it.
     */
    jump(id: number): Promise<void> {
      return request(`jump?id=${id}`).then(
        (response) => {
          if (response.ok) {
            showMessage('');
          } else if (response.status === 404) {
            showMessage(STALE_MESSAGE);
          } else {
            showMessage(`Jump failed (HTTP ${response.status}).`);
          }
        },
        () => {
          showMessage('Jump service unreachable.');
        },
      );
    },
  };
}

const Jump = { createJump, STALE_MESSAGE };

export { createJump, STALE_MESSAGE, Jump };
