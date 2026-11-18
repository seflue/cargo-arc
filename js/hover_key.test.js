import { describe, expect, test } from 'bun:test';
import { deriveHoverKey } from './svg_script.js';

describe('deriveHoverKey', () => {
  test('arc → element key', () => {
    expect(deriveHoverKey('arc', '1-2')).toBe('arc:1-2');
  });

  test('two arcs of one tangle → distinct keys', () => {
    expect(deriveHoverKey('arc', '1-2')).not.toBe(deriveHoverKey('arc', '3-4'));
  });

  test('node → element key', () => {
    expect(deriveHoverKey('node', '5')).toBe('node:5');
  });
});
