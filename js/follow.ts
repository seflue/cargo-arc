// follow.ts - Receives the service's focus and follow events and applies a
// focus to the page while following is on.

interface FollowHooks {
  /**
   * Opens the event stream and forwards each event to the handler; tests
   * inject a mock so no connection happens.
   */
  connect: (handler: (eventName: string, data: string) => void) => void;
  /**
   * Selects the node in the page and opens the sidebar rows of the given
   * jump ids; `file` is the same node's workspace-relative identity on the
   * other page, when the event carries one.
   */
  apply: (node: string, jumps: number[], file?: string) => void;
  /** Reflects the follow state on the toolbar button. */
  showState: (enabled: boolean) => void;
}

function createFollow({ connect, apply, showState }: FollowHooks) {
  let enabled = true;

  function setEnabled(on) {
    enabled = on;
    showState(enabled);
  }

  function handleFocus(data: string) {
    if (!enabled) return;
    let event: any;
    try {
      event = JSON.parse(data);
    } catch {
      return;
    }
    if (
      !event ||
      typeof event.node !== 'string' ||
      !Array.isArray(event.jumps)
    ) {
      return;
    }
    apply(
      event.node,
      event.jumps,
      typeof event.file === 'string' ? event.file : undefined,
    );
  }

  function handleFollow(data: string) {
    if (data === 'on') setEnabled(true);
    else if (data === 'off') setEnabled(false);
  }

  return {
    /** Opens the stream and shows the initial state. */
    start() {
      showState(enabled);
      connect((eventName, data) => {
        if (eventName === 'focus') handleFocus(data);
        else if (eventName === 'follow') handleFollow(data);
      });
    },
    setEnabled,
    isEnabled() {
      return enabled;
    },
  };
}

/**
 * Connects to the service's event stream, relative to the page so the port
 * is never spelled out, and forwards every event the service sends.
 * `EventSource` reconnects by itself.
 */
function connectEventSource(
  handler: (eventName: string, data: string) => void,
) {
  const source = new EventSource('events');
  for (const name of [
    'focus',
    'follow',
    'theme',
    'analysis',
    'analysis-error',
    'on-save',
  ]) {
    source.addEventListener(name, (event) => {
      handler(name, (event as MessageEvent).data);
    });
  }
}

const Follow = { createFollow, connectEventSource };

export { createFollow, connectEventSource, Follow };
