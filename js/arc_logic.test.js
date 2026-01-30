import { test, expect, describe } from "bun:test";
import { ArcLogic } from "./arc_logic.js";

describe("ArcLogic (Arrow functions)", () => {
  describe("getArrowPoints", () => {
    test("generates correct points at scale 1.0", () => {
      const points = ArcLogic.getArrowPoints({ x: 100, y: 50 }, 1.0);
      // Arrow at (100, 50), len=8, hw=4
      // Format: "tip.x+len,tip.y-hw tip.x,tip.y tip.x+len,tip.y+hw"
      expect(points).toBe("108,46 100,50 108,54");
    });

    test("scales arrow dimensions correctly", () => {
      const points = ArcLogic.getArrowPoints({ x: 100, y: 50 }, 2.0);
      // Arrow at (100, 50), len=16, hw=8
      expect(points).toBe("116,42 100,50 116,58");
    });

    test("handles small scale factors", () => {
      const points = ArcLogic.getArrowPoints({ x: 100, y: 50 }, 0.5);
      // Arrow at (100, 50), len=4, hw=2
      expect(points).toBe("104,48 100,50 104,52");
    });
  });

  describe("parseTipFromPoints", () => {
    test("extracts tip coordinates from valid points string", () => {
      const tip = ArcLogic.parseTipFromPoints("108,46 100,50 108,54");
      expect(tip).toEqual({ x: 100, y: 50 });
    });

    test("returns null for single point (parts.length === 1)", () => {
      const tip = ArcLogic.parseTipFromPoints("108,46");
      expect(tip).toBeNull();
    });

    test("returns null for empty string", () => {
      const tip = ArcLogic.parseTipFromPoints("");
      expect(tip).toBeNull();
    });

    test("returns null for malformed coordinate pair", () => {
      const tip = ArcLogic.parseTipFromPoints("108,46 invalid 108,54");
      expect(tip).toBeNull();
    });
  });

  describe("scaleFromStrokeWidth", () => {
    test("calculates correct scale for base stroke width", () => {
      expect(ArcLogic.scaleFromStrokeWidth(1.5)).toBe(1.0);
    });

    test("calculates correct scale for larger stroke width", () => {
      expect(ArcLogic.scaleFromStrokeWidth(3.0)).toBe(2.0);
    });
  });

  describe("constants", () => {
    test("exports ARROW_LENGTH", () => {
      expect(ArcLogic.ARROW_LENGTH).toBe(8);
    });

    test("exports ARROW_HALF_WIDTH", () => {
      expect(ArcLogic.ARROW_HALF_WIDTH).toBe(4);
    });
  });
});
