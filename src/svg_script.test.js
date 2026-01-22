import { test, expect, describe } from "bun:test";
import { ArcLogic } from "./svg_script.js";

describe("ArcLogic", () => {
  describe("getArcOffset", () => {
    test("calculates correct offset for 1 hop", () => {
      expect(ArcLogic.getArcOffset(1)).toBe(35); // 20 + 1*15
    });

    test("calculates correct offset for 3 hops", () => {
      expect(ArcLogic.getArcOffset(3)).toBe(65); // 20 + 3*15
    });

    test("calculates correct offset for 0 hops", () => {
      expect(ArcLogic.getArcOffset(0)).toBe(20); // 20 + 0*15
    });

    test("calculates correct offset for 10 hops", () => {
      expect(ArcLogic.getArcOffset(10)).toBe(170); // 20 + 10*15
    });
  });

  describe("calculateArcPath", () => {
    test("returns valid SVG path with M and Q commands", () => {
      const result = ArcLogic.calculateArcPath(100, 50, 100, 150, 200, 24);
      expect(result.path).toContain("M ");
      expect(result.path).toContain("Q ");
    });

    test("returns correct toX and toY coordinates", () => {
      const result = ArcLogic.calculateArcPath(100, 50, 150, 200, 200, 24);
      expect(result.toX).toBe(150);
      expect(result.toY).toBe(200);
    });

    test("calculates midY as average of fromY and toY", () => {
      const result = ArcLogic.calculateArcPath(100, 50, 100, 150, 200, 24);
      expect(result.midY).toBe(100); // (50 + 150) / 2
    });

    test("calculates ctrlX based on maxRight and arc offset", () => {
      // 150 distance, rowHeight 24 => hops = max(1, round(150/24)) = 6
      // arcOffset = 20 + 6*15 = 110
      // ctrlX = 200 + 110 = 310
      const result = ArcLogic.calculateArcPath(100, 0, 100, 150, 200, 24);
      expect(result.ctrlX).toBe(310);
    });

    test("minimum hops is 1", () => {
      // Even with small distance, hops should be at least 1
      const result = ArcLogic.calculateArcPath(100, 50, 100, 55, 200, 24);
      // hops = max(1, round(5/24)) = max(1, 0) = 1
      // arcOffset = 20 + 1*15 = 35
      // ctrlX = 200 + 35 = 235
      expect(result.ctrlX).toBe(235);
    });

    test("path format is correct quadratic bezier", () => {
      const result = ArcLogic.calculateArcPath(100, 50, 150, 200, 200, 24);
      // Path should be: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
      const pathRegex = /^M \d+,\d+ Q \d+,\d+ \d+,\d+ Q \d+,\d+ \d+,\d+$/;
      expect(result.path).toMatch(pathRegex);
    });
  });
});
