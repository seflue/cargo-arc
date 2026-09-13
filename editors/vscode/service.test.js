import { describe, expect, test } from 'bun:test';
import { argv, jumpLine, parseLine, resolveBinary } from './service.js';

const defaults = {
  binary: 'cargo-arc',
  manifestPath: '',
  features: [],
  includeTests: false,
  externals: false,
};

describe('argv', () => {
  test('defaults to the bare binary with only the subcommand', () => {
    expect(argv(defaults, null)).toEqual(['cargo-arc', 'arc', 'ui']);
  });

  test('puts the shared flags before ui and the port after it', () => {
    const config = {
      binary: '/opt/cargo-arc',
      manifestPath: 'crates/a/Cargo.toml',
      features: ['x', 'y'],
      includeTests: true,
      externals: true,
    };
    expect(argv(config, 4321)).toEqual([
      '/opt/cargo-arc',
      'arc',
      '--manifest-path',
      'crates/a/Cargo.toml',
      '--features',
      'x,y',
      '--include-tests',
      '--externals',
      'ui',
      '--port',
      '4321',
    ]);
  });
});

describe('parseLine', () => {
  test('reads version and port from the ready line', () => {
    expect(parseLine('arc ready 0.5.0 4321')).toEqual({
      kind: 'ready',
      version: '0.5.0',
      port: 4321,
    });
  });

  test('reads line and path from the jump line, spaces in the path included', () => {
    expect(parseLine('arc jump 7 /tmp/a b/lib.rs')).toEqual({
      kind: 'jump',
      line: 7,
      file: '/tmp/a b/lib.rs',
    });
  });

  test('ignores any other line', () => {
    expect(parseLine('warning: something')).toBeNull();
  });
});

describe('resolveBinary', () => {
  test('leaves a bare name for PATH lookup', () => {
    expect(resolveBinary('cargo-arc', '/repo')).toBe('cargo-arc');
  });

  test('resolves a value with a path separator against the workspace folder', () => {
    expect(resolveBinary('target/release/cargo-arc', '/repo')).toBe(
      '/repo/target/release/cargo-arc',
    );
  });

  test('leaves an absolute path unchanged', () => {
    expect(resolveBinary('/opt/cargo-arc', '/repo')).toBe('/opt/cargo-arc');
  });

  const env = { HOME: '/home/me', BUILD: '/build' };

  test('expands a leading ~ to the home directory', () => {
    expect(resolveBinary('~/bin/cargo-arc', '/repo', env)).toBe(
      '/home/me/bin/cargo-arc',
    );
  });

  test('expands $VAR and braced variables from the environment', () => {
    expect(resolveBinary('$HOME/bin/cargo-arc', '/repo', env)).toBe(
      '/home/me/bin/cargo-arc',
    );
    // biome-ignore lint/suspicious/noTemplateCurlyInString: the literal is the syntax under test
    expect(resolveBinary('${BUILD}/cargo-arc', '/repo', env)).toBe(
      '/build/cargo-arc',
    );
  });

  test('leaves an unset variable literal so the error names it', () => {
    expect(resolveBinary('$NOPE/cargo-arc', '/repo', env)).toBe(
      '/repo/$NOPE/cargo-arc',
    );
  });
});

describe('jumpLine', () => {
  test('converts a 1-based line to 0-based', () => {
    expect(jumpLine(7, 100)).toBe(6);
  });

  test('clamps to the last line when the file has grown shorter', () => {
    expect(jumpLine(500, 10)).toBe(9);
  });
});
