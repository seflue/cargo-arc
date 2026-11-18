import { describe, expect, test } from 'bun:test';
import { createHoverKeyTracker, deriveHoverKey } from './svg_script.js';

describe('createHoverKeyTracker', () => {
  test('same key twice → second is a no-op', () => {
    const t = createHoverKeyTracker();
    expect(t.enter('arc:1-2')).toBe(true);
    expect(t.enter('arc:1-2')).toBe(false);
  });

  test('different key → new identity, updates state', () => {
    const t = createHoverKeyTracker();
    t.enter('arc:3-4');
    expect(t.enter('arc:1-2')).toBe(true);
    expect(t.enter('arc:1-2')).toBe(false);
  });

  test('reset clears the identity', () => {
    const t = createHoverKeyTracker();
    t.enter('arc:1-2');
    t.reset();
    expect(t.enter('arc:1-2')).toBe(true);
  });

  test('moving between two arcs of one tangle is a new hover each time', () => {
    const t = createHoverKeyTracker();
    expect(t.enter(deriveHoverKey('arc', '1-2'))).toBe(true);
    expect(t.enter(deriveHoverKey('arc', '3-4'))).toBe(true);
  });
});
