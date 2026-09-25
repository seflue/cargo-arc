import { describe, expect, test } from 'bun:test';
import { CanvasSize } from './canvas_size.js';

describe('svgWidth', () => {
  test('a diagram narrower than the window widens to the window', () => {
    expect(CanvasSize.svgWidth(188, 1000)).toBe(1000);
  });

  test('a diagram wider than the window keeps its own width', () => {
    expect(CanvasSize.svgWidth(1500, 1000)).toBe(1500);
  });
});

describe('svgHeight', () => {
  test('a toolbar wrapped onto extra rows adds their height below the diagram', () => {
    expect(CanvasSize.svgHeight(230, 84)).toBe(314);
  });
});
