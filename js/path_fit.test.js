import { describe, expect, test } from 'bun:test';
import { PathFit } from './path_fit.js';

// A span stand-in: `fitPath` reads `dataset.full` and writes `textContent`
// and `style.flexShrink`, nothing else.
function span(full) {
  return { dataset: { full }, textContent: '', style: { flexShrink: '' } };
}

// Stand-in for the DOM overflow test: a span overflows when its text is
// longer than `limit` characters.
const longerThan = (limit) => (s) => s.textContent.length > limit;

describe('elidePath', () => {
  test('drops segments from the end of the prefix, keeps the last part', () => {
    expect(PathFit.elidePath('a::b::c::', '::', 1)).toBe('a::b::…::');
    expect(PathFit.elidePath('a::b::c::', '::', 2)).toBe('a::…::');
    expect(PathFit.elidePath('a::b::c::', '::', 3)).toBe('…::');
  });

  test('keeps the file name of a file path', () => {
    expect(PathFit.elidePath('src/render/static.rs', '/', 1)).toBe(
      'src/…/static.rs',
    );
    expect(PathFit.elidePath('src/render/static.rs', '/', 2)).toBe(
      '…/static.rs',
    );
  });

  test('dropCount past the prefix stops at the last part', () => {
    expect(PathFit.elidePath('src/render/static.rs', '/', 9)).toBe(
      '…/static.rs',
    );
  });

  test('zero drops returns the path unchanged', () => {
    expect(PathFit.elidePath('a::b::', '::', 0)).toBe('a::b::');
    expect(PathFit.elidePath('lib.rs', '/', 0)).toBe('lib.rs');
  });
});

describe('fitPath', () => {
  test('drops the middle segments until the path fits', () => {
    const s = span('src/render/layout/build.rs');
    PathFit.fitPath(s, '/', longerThan(14));
    expect(s.textContent).toBe('src/…/build.rs');
  });

  test('restores the full path when there is room again', () => {
    const s = span('src/render/layout/build.rs');
    PathFit.fitPath(s, '/', longerThan(14));
    PathFit.resetPath(s);
    expect(s.textContent).toBe('src/render/layout/build.rs');
  });
});
