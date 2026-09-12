import { describe, expect, mock, test } from 'bun:test';
import { createJump, STALE_MESSAGE } from './jump.js';

describe('createJump', () => {
  test('requests the relative jump URL for the given id, once', async () => {
    const request = mock(() => Promise.resolve({ ok: true, status: 200 }));
    const jumper = createJump(request, () => {});

    await jumper.jump(7);

    expect(request).toHaveBeenCalledTimes(1);
    expect(request).toHaveBeenCalledWith('jump?id=7');
  });

  test('clears the message on a successful response', async () => {
    const request = mock(() => Promise.resolve({ ok: true, status: 200 }));
    const showMessage = mock();
    const jumper = createJump(request, showMessage);

    await jumper.jump(1);

    expect(showMessage).toHaveBeenCalledWith('');
  });

  test('shows the stale-diagram message on a 404', async () => {
    const request = mock(() => Promise.resolve({ ok: false, status: 404 }));
    const showMessage = mock();
    const jumper = createJump(request, showMessage);

    await jumper.jump(1);

    expect(showMessage).toHaveBeenCalledWith(STALE_MESSAGE);
  });

  test('shows an unreachable message when the request throws', async () => {
    const request = mock(() => Promise.reject(new Error('network down')));
    const showMessage = mock();
    const jumper = createJump(request, showMessage);

    await jumper.jump(1);

    expect(showMessage).toHaveBeenCalledWith('Jump service unreachable.');
  });

  test('shows the status code for another failing response', async () => {
    const request = mock(() => Promise.resolve({ ok: false, status: 500 }));
    const showMessage = mock();
    const jumper = createJump(request, showMessage);

    await jumper.jump(1);

    expect(showMessage).toHaveBeenCalledWith('Jump failed (HTTP 500).');
  });

  test('does not mistake an error from showMessage for a network failure', async () => {
    const request = mock(() => Promise.resolve({ ok: true, status: 200 }));
    const showMessage = mock(() => {
      throw new Error('render failed');
    });
    const jumper = createJump(request, showMessage);

    await expect(jumper.jump(1)).rejects.toThrow('render failed');
  });
});
