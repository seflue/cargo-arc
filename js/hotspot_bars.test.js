import { describe, expect, test } from 'bun:test';
import { hotspotBars } from './hotspot_bars.js';

// Three ranked hotspots, rank order as STATIC_DATA.hotspots gives it.
// fillPercent is precomputed by Rust (render::hotspots::circle_style's own
// scale), not recomputed here.
function nodes() {
  return {
    hot: {
      name: 'hot.rs',
      file: 'src/hot.rs',
      lines: 100,
      commits: 8,
      rank: 1,
      fillPercent: 100,
    },
    warm: {
      name: 'warm.rs',
      file: 'src/warm.rs',
      lines: 40,
      commits: 4,
      rank: 2,
      fillPercent: 71,
    },
    cool: {
      name: 'cool.rs',
      file: 'src/cool.rs',
      lines: 10,
      commits: 1,
      rank: 3,
      fillPercent: 35,
    },
  };
}

describe('hotspotBars', () => {
  test('keeps the given rank order', () => {
    const bars = hotspotBars(nodes(), ['hot', 'warm', 'cool']);
    expect(bars.map((b) => b.key)).toEqual(['hot', 'warm', 'cool']);
  });

  test('scores by lines times commits', () => {
    const bars = hotspotBars(nodes(), ['hot', 'warm']);
    expect(bars[0].score).toBe(800);
    expect(bars[1].score).toBe(160);
  });

  test('widths are relative to the highest-scoring hotspot', () => {
    const bars = hotspotBars(nodes(), ['hot', 'warm', 'cool']);
    expect(bars[0].widthPercent).toBe(100);
    expect(bars[1].widthPercent).toBe(20);
    expect(bars[2].widthPercent).toBeCloseTo(1.25, 5);
  });

  test('fill is read straight off the node, not recomputed', () => {
    const bars = hotspotBars(nodes(), ['hot', 'warm', 'cool']);
    expect(bars[0].fillPercent).toBe(100);
    expect(bars[1].fillPercent).toBe(71);
    expect(bars[2].fillPercent).toBe(35);
  });

  test('carries the file path and rank through for display', () => {
    const [bar] = hotspotBars(nodes(), ['hot']);
    expect(bar.file).toBe('src/hot.rs');
    expect(bar.rank).toBe(1);
    expect(bar.lines).toBe(100);
    expect(bar.commits).toBe(8);
  });

  test('an empty hotspot list gives an empty bar list', () => {
    expect(hotspotBars(nodes(), [])).toEqual([]);
  });
});
