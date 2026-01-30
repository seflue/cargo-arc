import { test, expect, describe } from "bun:test";
import { ArcLogic } from "./arc_logic.js";

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

  describe("getSvgCoords", () => {
    test("transforms client coords to SVG coords without viewBox", () => {
      const svgRect = { left: 100, top: 50, width: 800, height: 600 };
      const result = ArcLogic.getSvgCoords(150, 100, svgRect, null);
      expect(result.x).toBe(50);  // 150 - 100
      expect(result.y).toBe(50);  // 100 - 50
    });

    test("transforms client coords with viewBox scaling", () => {
      const svgRect = { left: 0, top: 0, width: 400, height: 300 };
      const viewBox = { x: 0, y: 0, width: 800, height: 600 };
      const result = ArcLogic.getSvgCoords(200, 150, svgRect, viewBox);
      // x = 200 * (800/400) + 0 = 400
      // y = 150 * (600/300) + 0 = 300
      expect(result.x).toBe(400);
      expect(result.y).toBe(300);
    });

    test("handles viewBox with offset", () => {
      const svgRect = { left: 0, top: 0, width: 400, height: 300 };
      const viewBox = { x: 100, y: 50, width: 400, height: 300 };
      const result = ArcLogic.getSvgCoords(200, 150, svgRect, viewBox);
      // x = 200 * (400/400) + 100 = 300
      // y = 150 * (300/300) + 50 = 200
      expect(result.x).toBe(300);
      expect(result.y).toBe(200);
    });

    test("handles scroll offset via svgRect", () => {
      // When SVG is scrolled, getBoundingClientRect returns negative left/top
      const svgRect = { left: -200, top: -100, width: 800, height: 600 };
      const result = ArcLogic.getSvgCoords(100, 150, svgRect, null);
      // x = 100 - (-200) = 300
      // y = 150 - (-100) = 250
      expect(result.x).toBe(300);
      expect(result.y).toBe(250);
    });

    test("handles viewBox with zero width gracefully", () => {
      const svgRect = { left: 0, top: 0, width: 400, height: 300 };
      const viewBox = { x: 0, y: 0, width: 0, height: 0 };
      const result = ArcLogic.getSvgCoords(200, 150, svgRect, viewBox);
      // Should skip viewBox transform when width is 0
      expect(result.x).toBe(200);
      expect(result.y).toBe(150);
    });
  });

  describe("estimatePathLength", () => {
    test("returns 100 for empty/null path", () => {
      expect(ArcLogic.estimatePathLength("")).toBe(100);
      expect(ArcLogic.estimatePathLength(null)).toBe(100);
      expect(ArcLogic.estimatePathLength(undefined)).toBe(100);
    });

    test("returns 100 for invalid path with insufficient coordinates", () => {
      expect(ArcLogic.estimatePathLength("M 0,0")).toBe(100);
      expect(ArcLogic.estimatePathLength("invalid")).toBe(100);
    });

    test("estimates length from valid S-curve path using bezier approximation", () => {
      // Path: M fromX,fromY Q ctrlX,fromY ctrlX,midY Q ctrlX,toY toX,toY
      // Uses quadratic bezier approximation for accurate S-curve length
      const path = "M 100,50 Q 300,50 300,100 Q 300,150 100,150";
      // Horizontal extent 200, vertical extent 100 → bezier length ≈ 456
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(456, 0);
    });

    test("handles large vertical distance", () => {
      // Horizontal extent 300, vertical extent 500 → bezier length ≈ 941
      const path = "M 100,0 Q 400,0 400,250 Q 400,500 100,500";
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(941, 0);
    });

    test("handles negative coordinates", () => {
      // Same geometry as first test, just shifted → ≈ 456
      const path = "M 100,-50 Q 300,-50 300,0 Q 300,50 100,50";
      expect(ArcLogic.estimatePathLength(path)).toBeCloseTo(456, 0);
    });
  });

  describe("sortAndGroupLocations", () => {
    test("returns empty string for empty input", () => {
      expect(ArcLogic.sortAndGroupLocations([])).toBe("");
    });

    test("sorts symbols alphabetically", () => {
      const input = ["Zebra  ← src/z.rs:1", "Alpha  ← src/a.rs:1"];
      const result = ArcLogic.sortAndGroupLocations(input);
      expect(result).toBe("Alpha  ← src/a.rs:1|Zebra  ← src/z.rs:1");
    });

    test("sorts locations within same symbol", () => {
      const input = ["Foo  ← src/z.rs:1", "Foo  ← src/a.rs:2"];
      const result = ArcLogic.sortAndGroupLocations(input);
      // Foo = 3 chars, padding = maxLen(3) + 2 = 5 spaces for continuation
      expect(result).toBe("Foo  ← src/a.rs:2|     ← src/z.rs:1");
    });

    test("handles pipe-separated entries in single string", () => {
      const input = ["Beta  ← src/b.rs:1|Alpha  ← src/a.rs:2"];
      const result = ArcLogic.sortAndGroupLocations(input);
      expect(result).toBe("Alpha  ← src/a.rs:2|Beta   ← src/b.rs:1");
    });

    test("groups multiple locations per symbol with continuation lines", () => {
      const input = [
        "ModuleInfo  ← src/cli.rs:7",
        "ModuleInfo  ← src/render.rs:12"
      ];
      const result = ArcLogic.sortAndGroupLocations(input);
      expect(result).toBe("ModuleInfo  ← src/cli.rs:7|            ← src/render.rs:12");
    });

    test("places bare locations (no symbol) at the beginning", () => {
      const input = ["src/bare.rs:5", "Symbol  ← src/sym.rs:1"];
      const result = ArcLogic.sortAndGroupLocations(input);
      expect(result).toBe("src/bare.rs:5|Symbol  ← src/sym.rs:1");
    });

    test("handles complex mixed input", () => {
      const input = [
        "analyze_module  ← src/cli.rs:7|ModuleInfo  ← src/cli.rs:7",
        "ModuleInfo  ← src/render.rs:12"
      ];
      const result = ArcLogic.sortAndGroupLocations(input);
      // Symbols sorted alphabetically: ModuleInfo before analyze_module
      // Locations within ModuleInfo sorted: src/cli.rs:7 before src/render.rs:12
      expect(result).toBe(
        "ModuleInfo      ← src/cli.rs:7|                ← src/render.rs:12|analyze_module  ← src/cli.rs:7"
      );
    });
  });
});
