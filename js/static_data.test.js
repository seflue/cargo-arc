import { describe, expect, test } from 'bun:test';
import { ArcLogic } from './arc_logic.js';

globalThis.ArcLogic = ArcLogic;

globalThis.ArcLogic = ArcLogic;

// Mock STATIC_DATA for tests (structured object format from Phase 1)
const TEST_STATIC_DATA = {
  nodes: {
    crate: {
      type: 'crate',
      parent: null,
      x: 0,
      y: 0,
      width: 100,
      height: 24,
      hasChildren: true,
    },
    mod_a: {
      type: 'module',
      parent: 'crate',
      x: 20,
      y: 50,
      width: 100,
      height: 20,
      hasChildren: true,
    },
    fn_1: {
      type: 'function',
      parent: 'mod_a',
      x: 40,
      y: 60,
      width: 100,
      height: 20,
      hasChildren: false,
    },
    fn_2: {
      type: 'function',
      parent: 'mod_a',
      x: 40,
      y: 80,
      width: 100,
      height: 20,
      hasChildren: false,
    },
  },
  arcs: {
    'fn_1-fn_2': {
      from: 'fn_1',
      to: 'fn_2',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'call_fn2',
          modulePath: null,
          locations: [{ file: 'mod_a.rs', line: 10 }],
        },
      ],
    },
    'mod_a-crate': {
      from: 'mod_a',
      to: 'crate',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'use_crate',
          modulePath: null,
          locations: [
            { file: 'lib.rs', line: 5 },
            { file: 'lib.rs', line: 10 },
            { file: 'lib.rs', line: 15 },
          ],
        },
      ],
    },
    'fn_1-crate': {
      from: 'fn_1',
      to: 'crate',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'use_root',
          modulePath: null,
          locations: [
            { file: 'mod_a.rs', line: 20 },
            { file: 'mod_a.rs', line: 25 },
          ],
        },
      ],
    },
    'crate-fn_1': {
      from: 'crate',
      to: 'fn_1',
      context: { kind: 'production', subKind: null, features: [] },
      usages: [
        {
          symbol: 'call_fn1',
          modulePath: null,
          locations: [
            { file: 'lib.rs', line: 30 },
            { file: 'lib.rs', line: 35 },
            { file: 'lib.rs', line: 40 },
            { file: 'lib.rs', line: 45 },
            { file: 'lib.rs', line: 50 },
          ],
        },
      ],
    },
  },
};

// Inject STATIC_DATA globally for StaticData module
globalThis.STATIC_DATA = TEST_STATIC_DATA;

// Import after STATIC_DATA is set
const { StaticData } = await import('./static_data.js');

describe('StaticData', () => {
  describe('getArcStrokeWidth', () => {
    test('returns stroke width for arc with 1 usage', () => {
      // fn_1-fn_2 has 1 usage
      const width = StaticData.getArcStrokeWidth('fn_1-fn_2');

      // 1 usage -> calculateStrokeWidth(1) = MIN (0.5)
      const expected = ArcLogic.calculateStrokeWidth(1);
      expect(width).toBe(expected);
    });

    test('returns stroke width for arc with multiple usages', () => {
      // mod_a-crate has 3 usages
      const width = StaticData.getArcStrokeWidth('mod_a-crate');

      // 3 usages -> calculateStrokeWidth(3)
      const expected = ArcLogic.calculateStrokeWidth(3);
      expect(width).toBe(expected);
    });

    test('returns minimum stroke width for non-existent arc', () => {
      const width = StaticData.getArcStrokeWidth('nonexistent-arc');

      // 0 usages -> MIN (0.5)
      expect(width).toBe(0.5);
    });

    test('returns correct width for high usage count', () => {
      // Temporarily modify STATIC_DATA for this test
      const _originalArcs = { ...TEST_STATIC_DATA.arcs };
      TEST_STATIC_DATA.arcs['heavy-arc'] = {
        from: 'fn_1',
        to: 'crate',
        usages: [
          {
            symbol: 'heavy',
            modulePath: null,
            locations: Array(50).fill({ file: 'file.rs', line: 1 }),
          },
        ],
      };

      const width = StaticData.getArcStrokeWidth('heavy-arc');
      const expected = ArcLogic.calculateStrokeWidth(50);
      expect(width).toBe(expected);

      // Restore original
      delete TEST_STATIC_DATA.arcs['heavy-arc'];
    });
  });

  describe('existing functions', () => {
    test('getArcWeight returns usage count', () => {
      expect(StaticData.getArcWeight('fn_1-fn_2')).toBe(1);
      expect(StaticData.getArcWeight('mod_a-crate')).toBe(3);
    });

    test('getAllArcIds returns all arc IDs', () => {
      const ids = StaticData.getAllArcIds();
      expect(ids).toContain('fn_1-fn_2');
      expect(ids).toContain('mod_a-crate');
    });
  });

  describe('getOriginalPosition', () => {
    test('returns position with x, y, width, height', () => {
      const pos = StaticData.getOriginalPosition('crate');
      expect(pos).toEqual({ x: 0, y: 0, width: 100, height: 24 });
    });

    test('returns null for non-existent node', () => {
      expect(StaticData.getOriginalPosition('nonexistent')).toBeNull();
    });

    test('returns correct dimensions for module', () => {
      const pos = StaticData.getOriginalPosition('mod_a');
      expect(pos.width).toBe(100);
      expect(pos.height).toBe(20);
    });
  });

  describe('getArcUsages', () => {
    test('returns structured usages array', () => {
      const usages = StaticData.getArcUsages('fn_1-fn_2');
      expect(usages).toHaveLength(1);
      expect(usages[0].symbol).toBe('call_fn2');
      expect(usages[0].locations).toHaveLength(1);
      expect(usages[0].locations[0].file).toBe('mod_a.rs');
    });

    test('returns empty array for non-existent arc', () => {
      expect(StaticData.getArcUsages('nonexistent')).toEqual([]);
    });
  });

  describe('context field', () => {
    test('arc context field is accessible via getArc', () => {
      // Add a test arc to STATIC_DATA
      TEST_STATIC_DATA.arcs['test-arc'] = {
        from: 'fn_1',
        to: 'fn_2',
        context: { kind: 'test', subKind: 'unit', features: [] },
        usages: [
          {
            symbol: 'test_fn',
            modulePath: null,
            locations: [{ file: 'test.rs', line: 1 }],
          },
        ],
      };

      const arc = StaticData.getArc('test-arc');
      expect(arc.context.kind).toBe('test');

      // Production arc should have context.kind "production"
      const prodArc = StaticData.getArc('fn_1-fn_2');
      expect(prodArc.context.kind).toBe('production');

      // Cleanup
      delete TEST_STATIC_DATA.arcs['test-arc'];
    });
  });

  describe('getNodeRelations', () => {
    test('groups arcs by direction with correct fields', () => {
      // fn_1 has: outgoing fn_1-fn_2 (weight 1), fn_1-crate (weight 2)
      //           incoming crate-fn_1 (weight 5)
      const result = StaticData.getNodeRelations('fn_1');

      expect(result.outgoing).toHaveLength(2);
      expect(result.incoming).toHaveLength(1);

      // Check incoming entry
      expect(result.incoming[0].targetId).toBe('crate');
      expect(result.incoming[0].weight).toBe(5);
      expect(result.incoming[0].arcId).toBe('crate-fn_1');
      expect(result.incoming[0].usages).toHaveLength(1);
      expect(result.incoming[0].usages[0].symbol).toBe('call_fn1');
    });

    test('returns empty arrays for node without arcs', () => {
      const result = StaticData.getNodeRelations('fn_2');
      // fn_2 only has incoming fn_1-fn_2
      expect(result.outgoing).toEqual([]);
      expect(result.incoming).toHaveLength(1);

      // A node with no arcs at all
      const result2 = StaticData.getNodeRelations('nonexistent');
      expect(result2.outgoing).toEqual([]);
      expect(result2.incoming).toEqual([]);
    });

    test('sorts each direction by tree order (ascending y position)', () => {
      // fn_1 outgoing targets: crate (y:0), fn_2 (y:80)
      const result = StaticData.getNodeRelations('fn_1');

      expect(result.outgoing[0].arcId).toBe('fn_1-crate');
      expect(result.outgoing[0].targetId).toBe('crate');
      expect(result.outgoing[1].arcId).toBe('fn_1-fn_2');
      expect(result.outgoing[1].targetId).toBe('fn_2');
    });
  });

  describe('isExternalNode', () => {
    test('returns false for non-external node types', () => {
      expect(StaticData.isExternalNode('crate')).toBe(false);
      expect(StaticData.isExternalNode('mod_a')).toBe(false);
      expect(StaticData.isExternalNode('fn_1')).toBe(false);
    });

    test('returns false for non-existent node', () => {
      expect(StaticData.isExternalNode('nonexistent')).toBe(false);
    });

    test('returns true for external-section type', () => {
      TEST_STATIC_DATA.nodes.ext_section = {
        type: 'external-section',
        name: 'External Dependencies',
        parent: null,
        x: 0,
        y: 0,
        width: 200,
        height: 24,
        hasChildren: true,
      };

      expect(StaticData.isExternalNode('ext_section')).toBe(true);

      delete TEST_STATIC_DATA.nodes.ext_section;
    });

    test('returns true for external crate type', () => {
      TEST_STATIC_DATA.nodes.ext_serde = {
        type: 'external',
        name: 'serde',
        parent: 'ext_section',
        x: 10,
        y: 30,
        width: 100,
        height: 20,
        hasChildren: false,
        version: '1.0.0',
      };

      expect(StaticData.isExternalNode('ext_serde')).toBe(true);

      delete TEST_STATIC_DATA.nodes.ext_serde;
    });

    test('returns true for external-transitive crate type', () => {
      TEST_STATIC_DATA.nodes.ext_tokio = {
        type: 'external-transitive',
        name: 'tokio',
        parent: 'ext_section',
        x: 10,
        y: 50,
        width: 100,
        height: 20,
        hasChildren: false,
        version: '1.0.0',
      };

      expect(StaticData.isExternalNode('ext_tokio')).toBe(true);

      delete TEST_STATIC_DATA.nodes.ext_tokio;
    });
  });

  describe('isExternalArc', () => {
    test('returns false for arc between internal nodes', () => {
      expect(StaticData.isExternalArc('fn_1-fn_2')).toBe(false);
      expect(StaticData.isExternalArc('mod_a-crate')).toBe(false);
    });

    test('returns false for non-existent arc', () => {
      expect(StaticData.isExternalArc('nonexistent')).toBe(false);
    });

    test('returns true when from-node is external', () => {
      TEST_STATIC_DATA.nodes.ext_serde = {
        type: 'external',
        name: 'serde',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
      };
      TEST_STATIC_DATA.arcs['ext_serde-fn_1'] = {
        from: 'ext_serde',
        to: 'fn_1',
        usages: [
          {
            symbol: 'Serialize',
            modulePath: null,
            locations: [{ file: 'mod_a.rs', line: 1 }],
          },
        ],
      };

      expect(StaticData.isExternalArc('ext_serde-fn_1')).toBe(true);

      delete TEST_STATIC_DATA.nodes.ext_serde;
      delete TEST_STATIC_DATA.arcs['ext_serde-fn_1'];
    });

    test('returns true when to-node is external', () => {
      TEST_STATIC_DATA.nodes.ext_tokio = {
        type: 'external',
        name: 'tokio',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
      };
      TEST_STATIC_DATA.arcs['fn_1-ext_tokio'] = {
        from: 'fn_1',
        to: 'ext_tokio',
        usages: [
          {
            symbol: 'spawn',
            modulePath: null,
            locations: [{ file: 'mod_a.rs', line: 5 }],
          },
        ],
      };

      expect(StaticData.isExternalArc('fn_1-ext_tokio')).toBe(true);

      delete TEST_STATIC_DATA.nodes.ext_tokio;
      delete TEST_STATIC_DATA.arcs['fn_1-ext_tokio'];
    });
  });

  describe('isTransitiveNode', () => {
    test('returns false for non-external node types', () => {
      expect(StaticData.isTransitiveNode('crate')).toBe(false);
      expect(StaticData.isTransitiveNode('mod_a')).toBe(false);
    });

    test('returns false for non-existent node', () => {
      expect(StaticData.isTransitiveNode('nonexistent')).toBe(false);
    });

    test('returns false for direct external crate', () => {
      TEST_STATIC_DATA.nodes.ext_direct = {
        type: 'external',
        name: 'serde',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
      };

      expect(StaticData.isTransitiveNode('ext_direct')).toBe(false);

      delete TEST_STATIC_DATA.nodes.ext_direct;
    });

    test('returns true for external-transitive type', () => {
      TEST_STATIC_DATA.nodes.ext_trans = {
        type: 'external-transitive',
        name: 'syn',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
      };

      expect(StaticData.isTransitiveNode('ext_trans')).toBe(true);

      delete TEST_STATIC_DATA.nodes.ext_trans;
    });
  });

  describe('isTransitiveArc', () => {
    test('returns false for arc between internal nodes', () => {
      expect(StaticData.isTransitiveArc('fn_1-fn_2')).toBe(false);
    });

    test('returns false for non-existent arc', () => {
      expect(StaticData.isTransitiveArc('nonexistent')).toBe(false);
    });

    test('returns true when to-node is transitive', () => {
      TEST_STATIC_DATA.nodes.ext_trans = {
        type: 'external-transitive',
        name: 'syn',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
      };
      TEST_STATIC_DATA.arcs['fn_1-ext_trans'] = {
        from: 'fn_1',
        to: 'ext_trans',
        usages: [
          {
            symbol: 'parse',
            modulePath: null,
            locations: [{ file: 'mod_a.rs', line: 1 }],
          },
        ],
      };

      expect(StaticData.isTransitiveArc('fn_1-ext_trans')).toBe(true);

      delete TEST_STATIC_DATA.nodes.ext_trans;
      delete TEST_STATIC_DATA.arcs['fn_1-ext_trans'];
    });

    test('returns false for arc to direct external', () => {
      TEST_STATIC_DATA.nodes.ext_direct = {
        type: 'external',
        name: 'serde',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
      };
      TEST_STATIC_DATA.arcs['fn_1-ext_direct'] = {
        from: 'fn_1',
        to: 'ext_direct',
        usages: [
          {
            symbol: 'Serialize',
            modulePath: null,
            locations: [{ file: 'mod_a.rs', line: 1 }],
          },
        ],
      };

      expect(StaticData.isTransitiveArc('fn_1-ext_direct')).toBe(false);

      delete TEST_STATIC_DATA.nodes.ext_direct;
      delete TEST_STATIC_DATA.arcs['fn_1-ext_direct'];
    });
  });

  describe('getExternalGroups', () => {
    test('returns empty map when no external nodes', () => {
      const groups = StaticData.getExternalGroups();
      expect(groups.size).toBe(0);
    });

    test('groups external crates by name with multiple versions', () => {
      TEST_STATIC_DATA.nodes.ext_serde_1 = {
        type: 'external',
        name: 'serde',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
        version: '1.0.0',
      };
      TEST_STATIC_DATA.nodes.ext_serde_2 = {
        type: 'external',
        name: 'serde',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
        version: '2.0.0',
      };
      TEST_STATIC_DATA.nodes.ext_tokio = {
        type: 'external',
        name: 'tokio',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 20,
        hasChildren: false,
        version: '1.0.0',
      };

      const groups = StaticData.getExternalGroups();
      // Only serde has multiple versions
      expect(groups.size).toBe(1);
      expect(groups.has('serde')).toBe(true);
      expect(groups.get('serde')).toHaveLength(2);

      delete TEST_STATIC_DATA.nodes.ext_serde_1;
      delete TEST_STATIC_DATA.nodes.ext_serde_2;
      delete TEST_STATIC_DATA.nodes.ext_tokio;
    });

    test('ignores non-external nodes', () => {
      // Existing nodes are crate/module/function types, not external
      const groups = StaticData.getExternalGroups();
      expect(groups.size).toBe(0);
    });
  });

  describe('getExpandLevel', () => {
    test('returns null when expandLevel not set', () => {
      // TEST_STATIC_DATA has no expandLevel field
      expect(StaticData.getExpandLevel()).toBeNull();
    });

    test('returns null when expandLevel is undefined', () => {
      TEST_STATIC_DATA.expandLevel = undefined;
      expect(StaticData.getExpandLevel()).toBeNull();
      delete TEST_STATIC_DATA.expandLevel;
    });

    test('returns numeric value when expandLevel is set', () => {
      TEST_STATIC_DATA.expandLevel = 2;
      expect(StaticData.getExpandLevel()).toBe(2);
      delete TEST_STATIC_DATA.expandLevel;
    });

    test('returns 0 when expandLevel is 0', () => {
      TEST_STATIC_DATA.expandLevel = 0;
      expect(StaticData.getExpandLevel()).toBe(0);
      delete TEST_STATIC_DATA.expandLevel;
    });
  });

  describe('getNodeNesting', () => {
    test('returns nesting for node with nesting field', () => {
      TEST_STATIC_DATA.nodes.nested_mod = {
        type: 'module',
        parent: 'crate',
        x: 40,
        y: 100,
        width: 100,
        height: 20,
        hasChildren: false,
        nesting: 3,
      };

      expect(StaticData.getNodeNesting('nested_mod')).toBe(3);

      delete TEST_STATIC_DATA.nodes.nested_mod;
    });

    test('returns undefined for node without nesting field', () => {
      // crate node in TEST_STATIC_DATA has no nesting field
      expect(StaticData.getNodeNesting('crate')).toBeUndefined();
    });

    test('returns undefined for non-existent node', () => {
      expect(StaticData.getNodeNesting('nonexistent')).toBeUndefined();
    });

    test('returns 0 for node with nesting 0', () => {
      TEST_STATIC_DATA.nodes.root_crate = {
        type: 'crate',
        parent: null,
        x: 0,
        y: 0,
        width: 100,
        height: 24,
        hasChildren: true,
        nesting: 0,
      };

      expect(StaticData.getNodeNesting('root_crate')).toBe(0);

      delete TEST_STATIC_DATA.nodes.root_crate;
    });
  });

  describe('expand-level init logic (collapsed set population)', () => {
    // Replicate the init-block logic from svg_script.js:
    // for each node, if hasChildren && nesting >= expandLevel → collapse it
    function populateCollapsed(expandLvl) {
      const collapsed = new Set();
      if (expandLvl === null) return collapsed;
      for (const nodeId of StaticData.getAllNodeIds()) {
        if (
          StaticData.hasChildren(nodeId) &&
          StaticData.getNodeNesting(nodeId) >= expandLvl
        ) {
          collapsed.add(nodeId);
        }
      }
      return collapsed;
    }

    test('expandLevel null → nothing collapsed', () => {
      expect(populateCollapsed(null).size).toBe(0);
    });

    test('expandLevel collapses parent nodes at or above threshold', () => {
      // Setup: crate (nesting 0, hasChildren), mod_a (nesting 1, hasChildren),
      //        sub_mod (nesting 2, hasChildren), leaf (nesting 3, no children)
      TEST_STATIC_DATA.nodes.crate.nesting = 0;
      TEST_STATIC_DATA.nodes.mod_a.nesting = 1;
      TEST_STATIC_DATA.nodes.sub_mod = {
        type: 'module',
        parent: 'mod_a',
        x: 40,
        y: 90,
        width: 100,
        height: 20,
        hasChildren: true,
        nesting: 2,
      };
      TEST_STATIC_DATA.nodes.leaf = {
        type: 'module',
        parent: 'sub_mod',
        x: 60,
        y: 100,
        width: 100,
        height: 20,
        hasChildren: false,
        nesting: 3,
      };

      // expandLevel=1 → collapse nodes with hasChildren && nesting >= 1
      const collapsed = populateCollapsed(1);
      expect(collapsed.has('crate')).toBe(false); // nesting 0 < 1
      expect(collapsed.has('mod_a')).toBe(true); // nesting 1 >= 1, hasChildren
      expect(collapsed.has('sub_mod')).toBe(true); // nesting 2 >= 1, hasChildren
      expect(collapsed.has('leaf')).toBe(false); // no children

      delete TEST_STATIC_DATA.nodes.sub_mod;
      delete TEST_STATIC_DATA.nodes.leaf;
      delete TEST_STATIC_DATA.nodes.crate.nesting;
      delete TEST_STATIC_DATA.nodes.mod_a.nesting;
    });

    test('leaf nodes are never collapsed even at expandLevel 0', () => {
      TEST_STATIC_DATA.nodes.crate.nesting = 0;
      TEST_STATIC_DATA.nodes.fn_1.nesting = 2;
      TEST_STATIC_DATA.nodes.fn_2.nesting = 2;

      const collapsed = populateCollapsed(0);
      // crate has children → collapsed
      expect(collapsed.has('crate')).toBe(true);
      // fn_1, fn_2 have no children → not collapsed
      expect(collapsed.has('fn_1')).toBe(false);
      expect(collapsed.has('fn_2')).toBe(false);

      delete TEST_STATIC_DATA.nodes.crate.nesting;
      delete TEST_STATIC_DATA.nodes.fn_1.nesting;
      delete TEST_STATIC_DATA.nodes.fn_2.nesting;
    });

    test('STATIC_DATA provides arc data for edges between collapsed children', () => {
      // When mod_a is collapsed at expandLevel=1, fn_1 and fn_2 are hidden.
      // STATIC_DATA.arcs must still provide their arc data (single data source).
      TEST_STATIC_DATA.nodes.crate.nesting = 0;
      TEST_STATIC_DATA.nodes.mod_a.nesting = 1;
      TEST_STATIC_DATA.nodes.fn_1.nesting = 2;
      TEST_STATIC_DATA.nodes.fn_2.nesting = 2;

      const collapsed = populateCollapsed(1);
      expect(collapsed.has('mod_a')).toBe(true);

      // Arc between hidden children must be discoverable via StaticData
      const allArcIds = StaticData.getAllArcIds();
      expect(allArcIds).toContain('fn_1-fn_2');

      const arc = StaticData.getArc('fn_1-fn_2');
      expect(arc.from).toBe('fn_1');
      expect(arc.to).toBe('fn_2');
      expect(StaticData.getArcUsages('fn_1-fn_2')).toEqual(arc.usages);

      delete TEST_STATIC_DATA.nodes.crate.nesting;
      delete TEST_STATIC_DATA.nodes.mod_a.nesting;
      delete TEST_STATIC_DATA.nodes.fn_1.nesting;
      delete TEST_STATIC_DATA.nodes.fn_2.nesting;
    });
  });

  describe('qualifiedParts', () => {
    test('splits crate from same-leaf module paths', () => {
      const saved = globalThis.STATIC_DATA;
      globalThis.STATIC_DATA = {
        nodes: {
          krate: { type: 'crate', name: 'my-crate', parent: null },
          topStore: { type: 'module', name: 'store', parent: 'krate' },
          core: { type: 'module', name: 'core', parent: 'krate' },
          coreStore: { type: 'module', name: 'store', parent: 'core' },
          orphan: { type: 'module', name: 'lonely', parent: null },
        },
        arcs: {},
      };
      try {
        expect(StaticData.qualifiedParts('topStore')).toEqual({
          crate: 'my-crate',
          path: 'store',
        });
        expect(StaticData.qualifiedParts('coreStore')).toEqual({
          crate: 'my-crate',
          path: 'core::store',
        });
        expect(StaticData.qualifiedParts('orphan')).toEqual({
          crate: null,
          path: 'lonely',
        });
        expect(StaticData.qualifiedParts('missing')).toEqual({
          crate: null,
          path: 'missing',
        });
      } finally {
        globalThis.STATIC_DATA = saved;
      }
    });
  });
});
