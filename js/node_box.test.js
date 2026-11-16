import { describe, expect, test } from 'bun:test';
import { NodeBox } from './node_box.js';

describe('NodeBox', () => {
  test('the level number ends a gap before the collapse symbol', () => {
    // The collapse symbol starts 14 px before the right edge of the box.
    expect(NodeBox.levelEndX(40, 88)).toBe(108);
  });

  test('a collapsed row without a level needs its label and the padding', () => {
    expect(NodeBox.collapsedWidth(100, '')).toBe(120);
  });

  test('a collapsed row with a level also keeps room for the number and the collapse symbol', () => {
    // 100 + 20 padding + 14 collapse symbol + 2 digits of 6 + gap 6
    expect(NodeBox.collapsedWidth(100, '12')).toBe(152);
  });

  test('the level number of a collapsed row starts after its label', () => {
    const x = 40;
    const labelWidth = 100;
    const width = NodeBox.collapsedWidth(labelWidth, '12');
    const labelEnd = x + 10 + labelWidth;
    const levelStart = NodeBox.levelEndX(x, width) - 2 * 6;
    expect(levelStart - labelEnd).toBe(10);
  });

  test('the label text leaves out the level number at its end', () => {
    expect(NodeBox.labelText('outer ↺ (+3)12', '12')).toBe('outer ↺ (+3)');
    expect(NodeBox.labelText('outer', '')).toBe('outer');
  });
});
