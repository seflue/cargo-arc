import { describe, expect, mock, test } from 'bun:test';
import { createFollow } from './follow.js';

/** Builds a follow with mocks and returns them with the captured handler. */
function setup() {
  let handler = null;
  const connect = mock((h) => {
    handler = h;
  });
  const apply = mock();
  const showState = mock();
  const follow = createFollow({ connect, apply, showState });
  return {
    follow,
    apply,
    showState,
    connect,
    send: (name, data) => handler(name, data),
  };
}

describe('createFollow', () => {
  test('connects once on start and shows the initial state', () => {
    const { follow, connect, showState } = setup();
    follow.start();
    expect(connect).toHaveBeenCalledTimes(1);
    expect(showState).toHaveBeenCalledWith(true);
    expect(follow.isEnabled()).toBe(true);
  });

  test('a focus event applies the node and its jumps while enabled', () => {
    const { follow, apply, send } = setup();
    follow.start();
    send('focus', '{"node":"a::b","jumps":[3]}');
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith('a::b', [3], undefined);
  });

  test('a focus event with a file passes it through to apply', () => {
    const { follow, apply, send } = setup();
    follow.start();
    send('focus', '{"node":"a::b","jumps":[3],"file":"src/a.rs"}');
    expect(apply).toHaveBeenCalledWith('a::b', [3], 'src/a.rs');
  });

  test('a non-string file is treated as absent', () => {
    const { follow, apply, send } = setup();
    follow.start();
    send('focus', '{"node":"a::b","jumps":[3],"file":7}');
    expect(apply).toHaveBeenCalledWith('a::b', [3], undefined);
  });

  test('after follow off a focus event applies nothing', () => {
    const { follow, apply, showState, send } = setup();
    follow.start();
    send('follow', 'off');
    expect(showState).toHaveBeenLastCalledWith(false);
    expect(follow.isEnabled()).toBe(false);
    send('focus', '{"node":"a::b","jumps":[3]}');
    expect(apply).not.toHaveBeenCalled();
  });

  test('follow on shows the state and makes focus effective again', () => {
    const { follow, apply, showState, send } = setup();
    follow.start();
    send('follow', 'off');
    send('follow', 'on');
    expect(showState).toHaveBeenLastCalledWith(true);
    send('focus', '{"node":"7","jumps":[]}');
    expect(apply).toHaveBeenCalledWith('7', [], undefined);
  });

  test('setEnabled switches locally and shows the state', () => {
    const { follow, apply, showState, send } = setup();
    follow.start();
    follow.setEnabled(false);
    expect(showState).toHaveBeenLastCalledWith(false);
    send('focus', '{"node":"7","jumps":[]}');
    expect(apply).not.toHaveBeenCalled();
    follow.setEnabled(true);
    send('focus', '{"node":"7","jumps":[]}');
    expect(apply).toHaveBeenCalledTimes(1);
  });

  test('invalid focus data applies nothing and does not throw', () => {
    const { follow, apply, send } = setup();
    follow.start();
    expect(() => send('focus', 'not json')).not.toThrow();
    expect(() => send('focus', '{"jumps":[1]}')).not.toThrow();
    expect(() => send('focus', '{"node":"7"}')).not.toThrow();
    expect(apply).not.toHaveBeenCalled();
  });

  test('an unknown event name or follow value changes nothing', () => {
    const { follow, apply, showState, send } = setup();
    follow.start();
    send('other', 'x');
    send('follow', 'maybe');
    expect(apply).not.toHaveBeenCalled();
    expect(showState).toHaveBeenCalledTimes(1);
    expect(follow.isEnabled()).toBe(true);
  });
});
