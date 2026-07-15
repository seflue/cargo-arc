import { describe, expect, test } from 'bun:test';
import { deriveHoverKey } from './svg_script.js';

describe('deriveHoverKey', () => {
  test('cluster arc in SCC → scc key', () => {
    expect(deriveHoverKey('arc', '1-2', true, 0)).toBe('scc:0');
  });

  test('cluster off → element key', () => {
    expect(deriveHoverKey('arc', '1-2', false, 0)).toBe('arc:1-2');
  });

  test('arc without sccId → element key', () => {
    expect(deriveHoverKey('arc', '1-2', true, null)).toBe('arc:1-2');
  });

  test('node → element key regardless of cluster mode', () => {
    expect(deriveHoverKey('node', '5', true, 0)).toBe('node:5');
  });
});
