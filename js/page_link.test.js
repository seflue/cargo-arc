import { describe, expect, test } from 'bun:test';
import { buildLink, parseSelect } from './page_link.js';

describe('buildLink', () => {
  test('carries the file as an encoded ?select param', () => {
    expect(buildLink('/hotspots', 'src/a.rs')).toBe(
      '/hotspots?select=src%2Fa.rs',
    );
  });

  test('without a file the base path carries nothing', () => {
    expect(buildLink('/hotspots', null)).toBe('/hotspots');
    expect(buildLink('/hotspots', undefined)).toBe('/hotspots');
  });

  test('encodes characters the query string would otherwise break on', () => {
    expect(buildLink('/', 'src/a b&c.rs')).toBe('/?select=src%2Fa%20b%26c.rs');
  });
});

describe('parseSelect', () => {
  test('reads the select param, decoded', () => {
    expect(parseSelect('?select=src%2Fa.rs')).toBe('src/a.rs');
  });

  test('is null without a select param', () => {
    expect(parseSelect('')).toBeNull();
    expect(parseSelect('?other=1')).toBeNull();
  });

  test('reads select among other params', () => {
    expect(parseSelect('?variant=A&select=src%2Fa.rs')).toBe('src/a.rs');
  });
});
