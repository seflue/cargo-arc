import { describe, expect, test } from 'bun:test';
import { ArcLogic } from './arc_logic.js';
import { VirtualEdgeLogic } from './virtual_edge_logic.js';

// Phase 2: aggregateHiddenEdges
describe('VirtualEdgeLogic.aggregateHiddenEdges', () => {
  // Helper: mock getVisibleAncestorFn
  function mockVisibleAncestor(mapping) {
    return (nodeId) => mapping[nodeId] ?? nodeId;
  }

  test('no hidden edges returns empty map', () => {
    const edges = [
      {
        arcId: 'a-b',
        fromId: 'a',
        toId: 'b',
        fromHidden: false,
        toHidden: false,
      },
    ];
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({}),
    );
    expect(result.size).toBe(0);
  });

  test('single hidden edge returns count 1', () => {
    const edges = [
      {
        arcId: 'a-b',
        fromId: 'a',
        toId: 'b',
        fromHidden: true,
        toHidden: false,
        sourceLocations: 'loc1',
        direction: 'downward',
      },
    ];
    // 'a' is hidden, its visible ancestor is 'root'
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({ a: 'root' }),
    );
    expect(result.size).toBe(1);
    expect(result.get('root-b').count).toBe(1);
  });

  test('multiple edges to same ancestor are aggregated', () => {
    const edges = [
      {
        arcId: 'a1-b',
        fromId: 'a1',
        toId: 'b',
        fromHidden: true,
        toHidden: false,
        direction: 'downward',
      },
      {
        arcId: 'a2-b',
        fromId: 'a2',
        toId: 'b',
        fromHidden: true,
        toHidden: false,
        direction: 'downward',
      },
    ];
    // Both a1 and a2 hidden under same parent 'a'
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({ a1: 'a', a2: 'a' }),
    );
    expect(result.size).toBe(1);
    expect(result.get('a-b').count).toBe(2);
  });

  test('from hidden, to visible: uses visible ancestor for from', () => {
    const edges = [
      {
        arcId: 'child-target',
        fromId: 'child',
        toId: 'target',
        fromHidden: true,
        toHidden: false,
        direction: 'downward',
      },
    ];
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({ child: 'parent' }),
    );
    expect(result.has('parent-target')).toBe(true);
  });

  test('from visible, to hidden: uses visible ancestor for to', () => {
    const edges = [
      {
        arcId: 'source-child',
        fromId: 'source',
        toId: 'child',
        fromHidden: false,
        toHidden: true,
        direction: 'downward',
      },
    ];
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({ child: 'parent' }),
    );
    expect(result.has('source-parent')).toBe(true);
  });

  test('both hidden with same visible ancestor (self-loop) is excluded', () => {
    const edges = [
      {
        arcId: 'a-b',
        fromId: 'a',
        toId: 'b',
        fromHidden: true,
        toHidden: true,
        direction: 'downward',
      },
    ];
    // Both map to same visible ancestor 'root'
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({ a: 'root', b: 'root' }),
    );
    expect(result.size).toBe(0);
  });

  test('sourceLocations are collected', () => {
    const edges = [
      {
        arcId: 'a-b',
        fromId: 'a',
        toId: 'b',
        fromHidden: true,
        toHidden: false,
        sourceLocations: 'Symbol1  ← file1:10',
        direction: 'downward',
      },
      {
        arcId: 'c-b',
        fromId: 'c',
        toId: 'b',
        fromHidden: true,
        toHidden: false,
        sourceLocations: 'Symbol2  ← file2:20',
        direction: 'downward',
      },
    ];
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({ a: 'root', c: 'root' }),
    );
    const data = result.get('root-b');
    expect(data.hiddenEdgeData).toContain('Symbol1  ← file1:10');
    expect(data.hiddenEdgeData).toContain('Symbol2  ← file2:20');
  });

  test('a pair with both directions gives one arc per direction', () => {
    const edges = [
      {
        arcId: 'a-x',
        fromId: 'a',
        toId: 'x',
        fromHidden: true,
        toHidden: false,
        direction: 'upward',
      },
      {
        arcId: 'b-x',
        fromId: 'b',
        toId: 'x',
        fromHidden: true,
        toHidden: false,
        direction: 'downward',
      },
    ];
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({ a: 'root', b: 'root' }),
    );
    expect(result.size).toBe(2);
    expect(result.get('root-x')).toMatchObject({
      count: 1,
      direction: 'downward',
      originalArcs: ['b-x'],
    });
    expect(result.get('root-x-up')).toMatchObject({
      count: 1,
      direction: 'upward',
      originalArcs: ['a-x'],
      fromId: 'root',
      toId: 'x',
    });
  });

  test('an edge without a direction counts as downward', () => {
    const edges = [
      {
        arcId: 'a-x',
        fromId: 'a',
        toId: 'x',
        fromHidden: true,
        toHidden: false,
      },
    ];
    const result = VirtualEdgeLogic.aggregateHiddenEdges(
      edges,
      mockVisibleAncestor({ a: 'root' }),
    );
    expect(result.get('root-x').direction).toBe('downward');
  });
});

// Phase 3: prepareVirtualEdgeData
describe('VirtualEdgeLogic.prepareVirtualEdgeData', () => {
  // Mock ArcLogic
  const mockArcLogic = {
    calculateArcPath(fromX, fromY, toX, toY, maxRight, _rowHeight) {
      return {
        path: `M ${fromX},${fromY} Q ${maxRight},${(fromY + toY) / 2} ${toX},${toY}`,
        toX,
        toY,
        ctrlX: maxRight + 35,
        midY: (fromY + toY) / 2,
      };
    },
    calculateArcPathFromPositions(fromPos, toPos, yOffset, extent, rowHeight) {
      const fromX = fromPos.x + fromPos.width;
      const fromY = fromPos.y + fromPos.height / 2 + yOffset;
      const toX = toPos.x + toPos.width;
      const toY = toPos.y + toPos.height / 2 - yOffset;
      return this.calculateArcPath(
        fromX,
        fromY,
        toX,
        toY,
        extent.right,
        rowHeight,
      );
    },
    countLocations(locs) {
      return locs ? locs.split('|').length : 0;
    },
    calculateStrokeWidth(count) {
      return 0.5 + count * 0.1;
    },
  };

  test('arc path is calculated via ArcLogic', () => {
    const virtualEdges = new Map([
      ['a-b', { count: 1, hiddenEdgeData: [], direction: 'downward' }],
    ]);
    const nodePositions = new Map([
      ['a', { x: 10, y: 100, width: 80, height: 20 }],
      ['b', { x: 10, y: 200, width: 80, height: 20 }],
    ]);

    const result = VirtualEdgeLogic.prepareVirtualEdgeData(
      virtualEdges,
      nodePositions,
      { left: 10, right: 300 },
      mockArcLogic,
      25,
    );
    const data = result.get('a-b');
    expect(data.arc).toBeDefined();
    expect(data.arc.path).toContain('M ');
  });

  test('strokeWidth is calculated from locations', () => {
    const virtualEdges = new Map([
      [
        'a-b',
        {
          count: 2,
          hiddenEdgeData: ['loc1|loc2', 'loc3'],
          direction: 'downward',
        },
      ],
    ]);
    const nodePositions = new Map([
      ['a', { x: 10, y: 100, width: 80, height: 20 }],
      ['b', { x: 10, y: 200, width: 80, height: 20 }],
    ]);

    const result = VirtualEdgeLogic.prepareVirtualEdgeData(
      virtualEdges,
      nodePositions,
      { left: 10, right: 300 },
      mockArcLogic,
      25,
    );
    const data = result.get('a-b');
    expect(data.strokeWidth).toBeGreaterThan(0);
  });

  test('complete render data is returned', () => {
    const virtualEdges = new Map([
      [
        'a-b',
        {
          count: 3,
          hiddenEdgeData: ['loc1'],
          direction: 'upward',
        },
      ],
    ]);
    const nodePositions = new Map([
      ['a', { x: 10, y: 100, width: 80, height: 20 }],
      ['b', { x: 10, y: 200, width: 80, height: 20 }],
    ]);

    const result = VirtualEdgeLogic.prepareVirtualEdgeData(
      virtualEdges,
      nodePositions,
      { left: 10, right: 300 },
      mockArcLogic,
      25,
    );
    const data = result.get('a-b');

    expect(data.fromId).toBe('a');
    expect(data.toId).toBe('b');
    expect(data.count).toBe(3);
    expect(data.hiddenEdgeData).toEqual(['loc1']);
    expect(data.arc).toBeDefined();
    expect(data.strokeWidth).toBeDefined();
    expect(data.direction).toBe('upward');
  });

  test('an upward arc is drawn on the left of the rows', () => {
    const virtualEdges = new Map([
      [
        'a-b-up',
        {
          count: 1,
          hiddenEdgeData: [],
          direction: 'upward',
          fromId: 'a',
          toId: 'b',
        },
      ],
    ]);
    const nodePositions = new Map([
      ['b', { x: 200, y: 100, width: 80, height: 20 }],
      ['a', { x: 220, y: 160, width: 80, height: 20 }],
    ]);

    const result = VirtualEdgeLogic.prepareVirtualEdgeData(
      virtualEdges,
      nodePositions,
      { left: 200, right: 300 },
      ArcLogic,
      30,
    );

    // From y 173 to y 107 are 2 rows: reach 50 left of the leftmost row.
    expect(result.get('a-b-up').arc.path).toBe(
      'M 220,173 Q 150,173 150,140 Q 150,107 200,107',
    );
  });
});
