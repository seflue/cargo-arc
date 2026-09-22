import { describe, expect, mock, test } from 'bun:test';
import { createFakeElement } from './dom_adapter.js';
import { bootstrapControls, createTheme } from './theme.js';

const THEMES = {
  light: [{ name: 'latte', label: 'Latte' }],
  dark: [
    { name: 'mocha', label: 'Mocha' },
    { name: 'night', label: 'Night' },
  ],
};

/**
 * Builds a theme control with mocks. `stored` seeds the storage, `root` the
 * attributes the page root carried at load, `dark` the system setting.
 */
function setup({ stored = {}, root = {}, dark = false } = {}) {
  const store = new Map(Object.entries(stored));
  const storage = {
    get: (key) => store.get(key) ?? null,
    set: (key, value) => store.set(key, value),
  };
  let systemListener = null;
  let systemDark = dark;
  const applyTheme = mock();
  const showState = mock();
  const control = createTheme({
    themes: THEMES,
    storage,
    systemDark: () => systemDark,
    onSystemChange: (listener) => {
      systemListener = listener;
    },
    root,
    applyTheme,
    showState,
  });
  return {
    control,
    store,
    applyTheme,
    showState,
    lastApplied: () => applyTheme.mock.calls.at(-1)?.[0],
    lastShown: () => showState.mock.calls.at(-1)?.[0],
    setSystemDark: (value) => {
      systemDark = value;
      systemListener?.();
    },
  };
}

describe('createTheme', () => {
  test('without a choice the page follows the system setting', () => {
    const light = setup();
    light.control.start();
    expect(light.lastApplied()).toBe('latte');
    expect(light.lastShown()).toEqual({
      mode: 'system',
      light: 'latte',
      dark: 'mocha',
    });

    const dark = setup({ dark: true });
    dark.control.start();
    expect(dark.lastApplied()).toBe('mocha');
  });

  test('a system change re-applies only while the switch says system', () => {
    const t = setup();
    t.control.start();
    t.setSystemDark(true);
    expect(t.lastApplied()).toBe('mocha');

    t.control.setMode('light');
    t.setSystemDark(false);
    t.setSystemDark(true);
    expect(t.lastApplied()).toBe('latte');
  });

  test('the switch is remembered across loads', () => {
    const first = setup();
    first.control.start();
    first.control.setMode('dark');
    expect(first.lastApplied()).toBe('mocha');

    const second = setup({ stored: Object.fromEntries(first.store) });
    second.control.start();
    expect(second.lastApplied()).toBe('mocha');
    expect(second.lastShown().mode).toBe('dark');
  });

  test('the theme chosen for a mode is remembered and shown', () => {
    const first = setup({ dark: true });
    first.control.start();
    first.control.setThemeFor('dark', 'night');
    expect(first.lastApplied()).toBe('night');

    const second = setup({ stored: Object.fromEntries(first.store) });
    second.control.start();
    expect(second.lastShown()).toEqual({
      mode: 'system',
      light: 'latte',
      dark: 'night',
    });
    second.control.setMode('dark');
    expect(second.lastApplied()).toBe('night');
  });

  test('the editor mode on the root replaces the remembered switch with system', () => {
    const t = setup({
      stored: { 'arc.theme.mode': 'dark' },
      root: { mode: 'light' },
    });
    t.control.start();
    expect(t.lastApplied()).toBe('latte');
    expect(t.lastShown().mode).toBe('system');
    expect(t.store.get('arc.theme.mode')).toBe('system');
  });

  test('system follows the editor once it has sent a mode, else the OS', () => {
    const withEditor = setup({ root: { mode: 'dark' } });
    withEditor.control.start();
    expect(withEditor.lastApplied()).toBe('mocha');
    withEditor.setSystemDark(true);
    withEditor.setSystemDark(false);
    expect(withEditor.lastApplied()).toBe('mocha');

    const withoutEditor = setup({ dark: true });
    withoutEditor.control.start();
    expect(withoutEditor.lastApplied()).toBe('mocha');
  });

  test('an editor event during the session is the last word', () => {
    const t = setup();
    t.control.start();
    t.control.setMode('light');
    t.control.handleEditorMode('dark');
    expect(t.lastApplied()).toBe('mocha');
    expect(t.lastShown().mode).toBe('system');
    expect(t.store.get('arc.theme.mode')).toBe('system');

    t.control.setMode('light');
    expect(t.lastApplied()).toBe('latte');
    t.control.setMode('system');
    expect(t.lastApplied()).toBe('mocha');
  });

  test('a theme pinned on the root decides the mode and beats the remembered theme', () => {
    const t = setup({
      stored: { 'arc.theme.mode': 'light', 'arc.theme.dark': 'night' },
      root: { theme: 'mocha' },
    });
    t.control.start();
    expect(t.lastApplied()).toBe('mocha');
    expect(t.lastShown()).toEqual({
      mode: 'system',
      light: 'latte',
      dark: 'mocha',
    });
    expect(t.store.get('arc.theme.mode')).toBe('light');
  });

  test('choosing a theme for the pinned mode releases the pin', () => {
    const t = setup({ root: { theme: 'mocha' } });
    t.control.start();
    t.control.setThemeFor('dark', 'night');
    expect(t.lastApplied()).toBe('night');
    t.control.setMode('light');
    t.control.setMode('dark');
    expect(t.lastApplied()).toBe('night');
  });

  test('the editor mode outranks a pinned theme of the other mode', () => {
    const t = setup({ root: { theme: 'mocha', mode: 'light' } });
    t.control.start();
    expect(t.lastApplied()).toBe('latte');
    t.control.setMode('dark');
    expect(t.lastApplied()).toBe('mocha');
  });

  test('unknown stored or root values are ignored', () => {
    const t = setup({
      stored: { 'arc.theme.mode': 'sepia', 'arc.theme.dark': 'gone' },
      root: { theme: 'gone', mode: 'sepia' },
    });
    t.control.start();
    expect(t.lastApplied()).toBe('latte');
    expect(t.lastShown()).toEqual({
      mode: 'system',
      light: 'latte',
      dark: 'mocha',
    });
    t.control.setThemeFor('dark', 'gone');
    expect(t.lastShown().dark).toBe('mocha');
  });
});

// A <select> stub with a real addEventListener/_fire, so a test can drive
// the change bootstrapControls wires (pattern: createFakeInteractiveElement
// in hotspot_script.test.js).
function createFakeSelect() {
  const el = createFakeElement('select');
  const listeners = new Map();
  el.addEventListener = (evt, fn) => {
    if (!listeners.has(evt)) listeners.set(evt, []);
    listeners.get(evt).push(fn);
  };
  el._fire = (evt) => {
    for (const fn of listeners.get(evt) || []) fn();
  };
  return el;
}

describe('bootstrapControls', () => {
  test('populates both selects from STATIC_DATA.theme, reflects the mode, and forwards a change to setThemeFor', () => {
    const elements = {
      'theme-mode': createFakeSelect(),
      'theme-light': createFakeSelect(),
      'theme-dark': createFakeSelect(),
    };
    const store = new Map();
    global.DomAdapter = { getElementById: (id) => elements[id] ?? null };
    global.document = {
      documentElement: { dataset: {} },
      createElement: (tag) => createFakeElement(tag),
    };
    global.window = {
      matchMedia: () => ({ matches: false, addEventListener() {} }),
      localStorage: {
        getItem: (key) => store.get(key) ?? null,
        setItem: (key, value) => store.set(key, value),
      },
    };
    global.STATIC_DATA = {
      theme: THEMES,
    };

    bootstrapControls();

    expect(elements['theme-light'].children.map((o) => o.value)).toEqual([
      'latte',
    ]);
    expect(elements['theme-dark'].children.map((o) => o.value)).toEqual([
      'mocha',
      'night',
    ]);
    expect(elements['theme-mode'].value).toBe('system');
    expect(document.documentElement.dataset.theme).toBe('latte');

    elements['theme-dark'].value = 'night';
    elements['theme-dark']._fire('change');

    expect(store.get('arc.theme.dark')).toBe('night');
  });
});
