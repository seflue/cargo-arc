import { describe, expect, test } from 'bun:test';
import { RowState } from './row_state.js';

// One test per row of the transition table: state name in, state name out,
// for every event a tangle sidebar edge row can receive.
describe('RowState', () => {
  test('toggle flips the expand bit, keeps the pin bit', () => {
    expect(RowState.CU.toggle()).toBe('EU');
    expect(RowState.EU.toggle()).toBe('CU');
    expect(RowState.CP.toggle()).toBe('EP');
    expect(RowState.EP.toggle()).toBe('CP');
  });

  test('pinClick always expands on pin, unpinning never re-collapses', () => {
    expect(RowState.CU.pinClick()).toBe('EP');
    expect(RowState.EU.pinClick()).toBe('EP');
    expect(RowState.CP.pinClick()).toBe('CU');
    expect(RowState.EP.pinClick()).toBe('EU');
  });

  test('pinned (pin arrived from the graph) expands, no-ops if already pinned', () => {
    expect(RowState.CU.pinned()).toBe('EP');
    expect(RowState.EU.pinned()).toBe('EP');
    expect(RowState.CP.pinned()).toBe('CP');
    expect(RowState.EP.pinned()).toBe('EP');
  });

  test('unpinned (pin left, from the graph or another row) never collapses', () => {
    expect(RowState.CU.unpinned()).toBe('CU');
    expect(RowState.EU.unpinned()).toBe('EU');
    expect(RowState.CP.unpinned()).toBe('CU');
    expect(RowState.EP.unpinned()).toBe('EU');
  });

  test('expandAll always ends expanded, keeps the pin bit', () => {
    expect(RowState.CU.expandAll()).toBe('EU');
    expect(RowState.EU.expandAll()).toBe('EU');
    expect(RowState.CP.expandAll()).toBe('EP');
    expect(RowState.EP.expandAll()).toBe('EP');
  });

  test('collapseAll always ends collapsed, keeps the pin bit', () => {
    expect(RowState.CU.collapseAll()).toBe('CU');
    expect(RowState.EU.collapseAll()).toBe('CU');
    expect(RowState.CP.collapseAll()).toBe('CP');
    expect(RowState.EP.collapseAll()).toBe('CP');
  });

  test('follow (row holds the editor jump target) always ends expanded', () => {
    expect(RowState.CU.follow()).toBe('EU');
    expect(RowState.EU.follow()).toBe('EU');
    expect(RowState.CP.follow()).toBe('EP');
    expect(RowState.EP.follow()).toBe('EP');
  });
});
