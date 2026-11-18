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

describe('sidebarWidth', () => {
  test('room larger than the content gives the content width', () => {
    expect(CanvasSize.sidebarWidth(400, 700, 280)).toBe(400);
  });

  test('room smaller than the content gives the room', () => {
    expect(CanvasSize.sidebarWidth(600, 350, 280)).toBe(350);
  });

  test('room below the minimum gives the minimum', () => {
    expect(CanvasSize.sidebarWidth(600, 100, 280)).toBe(280);
    expect(CanvasSize.sidebarWidth(100, 100, 280)).toBe(280);
  });
});

describe('sidebarRoom', () => {
  test('is the space between the label edge plus gap and the right margin', () => {
    expect(CanvasSize.sidebarRoom(1000, 500, 24, 16)).toBe(460);
  });
});

describe('minSvgWidth', () => {
  test('keeps the labels left of a minimum-width sidebar when scrolled fully right', () => {
    expect(CanvasSize.minSvgWidth(500, 24, 280, 16)).toBe(820);
  });
});

describe('svgHeight', () => {
  test('a toolbar wrapped onto extra rows adds their height below the diagram', () => {
    expect(CanvasSize.svgHeight(230, 84)).toBe(314);
  });
});
