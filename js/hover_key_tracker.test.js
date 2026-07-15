import { describe, expect, test } from 'bun:test';
import { createHoverKeyTracker, deriveHoverKey } from './svg_script.js';

describe('createHoverKeyTracker', () => {
  test('same key twice → second is a no-op', () => {
    const t = createHoverKeyTracker();
    expect(t.enter('scc:0')).toBe(true);
    expect(t.enter('scc:0')).toBe(false);
  });

  test('different key → new identity, updates state', () => {
    const t = createHoverKeyTracker();
    t.enter('scc:0');
    expect(t.enter('arc:1-2')).toBe(true);
    expect(t.enter('arc:1-2')).toBe(false);
  });

  test('reset clears the identity', () => {
    const t = createHoverKeyTracker();
    t.enter('scc:0');
    t.reset();
    expect(t.enter('scc:0')).toBe(true);
  });

  test('sccId collision: two arc ids in one SCC collapse to one no-op', () => {
    const t = createHoverKeyTracker();
    // Two different arcs, same sccId, cluster mode on → identical key → no-op.
    const k1 = deriveHoverKey('arc', '1-2', true, 0);
    const k2 = deriveHoverKey('arc', '3-4', true, 0);
    expect(k1).toBe(k2);
    expect(t.enter(k1)).toBe(true);
    expect(t.enter(k2)).toBe(false);
  });
});
