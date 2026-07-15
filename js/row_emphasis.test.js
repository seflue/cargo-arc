import { describe, expect, test } from 'bun:test';
import { deriveRowEmphasis } from './svg_script.js';

describe('deriveRowEmphasis', () => {
  test('hovered arc emphasized, others dimmed', () => {
    const r = deriveRowEmphasis('A-B', ['A-B', 'C-A', 'D-A']);
    expect(r.emphasize).toEqual(['A-B']);
    expect(r.dim.sort()).toEqual(['C-A', 'D-A']);
  });
  test('hovered arc not in cut set → nothing emphasized', () => {
    const r = deriveRowEmphasis('X-Y', ['A-B', 'C-A']);
    expect(r.emphasize).toEqual([]);
    expect(r.dim.sort()).toEqual(['A-B', 'C-A']);
  });
});
