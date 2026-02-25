import { describe, expect, test } from 'bun:test';
import { createHighlightDebouncer } from './svg_script.js';

describe('createHighlightDebouncer', () => {
  test('debounced() delays execution by configured delay', async () => {
    let callCount = 0;
    const timing = createHighlightDebouncer(() => callCount++, 30);
    timing.debounced();
    expect(callCount).toBe(0);
    await new Promise((r) => setTimeout(r, 50));
    expect(callCount).toBe(1);
  });

  test('rapid debounced() calls trigger single execution', async () => {
    let callCount = 0;
    const timing = createHighlightDebouncer(() => callCount++, 30);
    timing.debounced();
    timing.debounced();
    timing.debounced();
    expect(callCount).toBe(0);
    await new Promise((r) => setTimeout(r, 50));
    expect(callCount).toBe(1);
  });

  test('immediate() cancels pending debounce and executes synchronously', async () => {
    let callCount = 0;
    const timing = createHighlightDebouncer(() => callCount++, 30);
    timing.debounced();
    timing.immediate();
    expect(callCount).toBe(1);
    await new Promise((r) => setTimeout(r, 50));
    expect(callCount).toBe(1);
  });

  test('immediate() executes without prior debounce', () => {
    let callCount = 0;
    const timing = createHighlightDebouncer(() => callCount++, 30);
    timing.immediate();
    expect(callCount).toBe(1);
  });

  test('debounced() works after immediate()', async () => {
    let callCount = 0;
    const timing = createHighlightDebouncer(() => callCount++, 30);
    timing.immediate();
    expect(callCount).toBe(1);
    timing.debounced();
    expect(callCount).toBe(1);
    await new Promise((r) => setTimeout(r, 50));
    expect(callCount).toBe(2);
  });
});
