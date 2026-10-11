import { DomAdapter } from './dom_adapter.js';

// theme.ts - Chooses the theme the page shows: a mode (light, dark or the
// system's) and a theme per mode, from the editor, the root attributes a
// pinned theme left, the remembered choices and the system setting.

type Mode = 'light' | 'dark';
type ModeChoice = Mode | 'system';
type ThemeName = { name: string; label: string };
type Themes = { light: ThemeName[]; dark: ThemeName[] };

interface ThemeHooks {
  /** The shipped themes per mode, the default first. */
  themes: Themes;
  /** Remembers the switch and the theme per mode across loads. */
  storage: {
    get: (key: string) => string | null;
    set: (key: string, value: string) => void;
  };
  /** Whether the system prefers dark. */
  systemDark: () => boolean;
  /** Calls the listener when the system setting changes. */
  onSystemChange: (listener: () => void) => void;
  /**
   * What the page root carried at load: a theme pinned by name (`--theme`)
   * and the mode the editor sent (`arc theme`).
   */
  root: { theme?: string; mode?: string };
  /** Puts the theme on the root. */
  applyTheme: (name: string) => void;
  /** Reflects the switch and the theme per mode on the toolbar. */
  showState: (state: { mode: ModeChoice; light: string; dark: string }) => void;
}

const MODE_KEY = 'arc.theme.mode';
const MODES = ['light', 'dark'] as Mode[];

function createTheme({
  themes,
  storage,
  systemDark,
  onSystemChange,
  root,
  applyTheme,
  showState,
}: ThemeHooks) {
  /** The switch: an override, or system, which leaves the rest of the chain to decide. */
  let choice: ModeChoice = 'system';
  /** The last mode the editor sent; what system follows in a webview. */
  let editorMode: Mode | null = null;
  const chosen: Record<Mode, string> = {
    light: themes.light[0].name,
    dark: themes.dark[0].name,
  };
  /** A theme pinned on the root, until the page chooses one for its mode. */
  let pinned: string | null = null;

  function modeOf(name: string | null | undefined) {
    return MODES.find((mode) => themes[mode].some((t) => t.name === name));
  }

  function themeFor(mode: Mode) {
    return pinned && modeOf(pinned) === mode ? pinned : chosen[mode];
  }

  /** Editor, then the pin, then the OS: the chain behind "system". */
  function chainMode() {
    return editorMode ?? modeOf(pinned) ?? (systemDark() ? 'dark' : 'light');
  }

  function apply() {
    applyTheme(themeFor(choice === 'system' ? chainMode() : choice));
    showState({
      mode: choice,
      light: themeFor('light'),
      dark: themeFor('dark'),
    });
  }

  function setMode(mode: ModeChoice) {
    choice = mode;
    storage.set(MODE_KEY, mode);
    apply();
  }

  function followEditor(mode: Mode) {
    editorMode = mode;
    if (modeOf(pinned) !== mode) pinned = null;
    // The editor's mode replaces the remembered switch: the page follows
    // the editor until the next choice made in the page.
    setMode('system');
  }

  return {
    /** Resolves the precedence at load and applies the result. */
    start() {
      for (const mode of MODES) {
        const remembered = storage.get(`arc.theme.${mode}`);
        if (remembered && modeOf(remembered) === mode)
          chosen[mode] = remembered;
      }
      pinned = root.theme && modeOf(root.theme) ? root.theme : null;
      const remembered = storage.get(MODE_KEY);
      if (
        remembered === 'light' ||
        remembered === 'dark' ||
        remembered === 'system'
      ) {
        choice = remembered;
      }
      // A pinned theme outranks the remembered switch, but only for this
      // load: the remembered value stays.
      if (pinned) choice = 'system';
      onSystemChange(apply);
      if (root.mode === 'light' || root.mode === 'dark') {
        followEditor(root.mode);
      } else {
        apply();
      }
    },
    setMode,
    setThemeFor(mode: Mode, name: string) {
      if (modeOf(name) !== mode) return;
      chosen[mode] = name;
      storage.set(`arc.theme.${mode}`, name);
      if (modeOf(pinned) === mode) {
        // Releasing the pin keeps the page in the mode it decided.
        pinned = null;
        if (choice === 'system') choice = mode;
      }
      apply();
    },
    /** @param mode - the mode the editor sent during the session. */
    handleEditorMode(mode: string) {
      if (mode === 'light' || mode === 'dark') followEditor(mode);
    },
  };
}

/**
 * `localStorage` behind the storage hooks; a browser that refuses it (a
 * file: page in some settings) leaves the page with defaults, not errors.
 */
function localStorageAdapter() {
  return {
    get(key: string) {
      try {
        return window.localStorage.getItem(key);
      } catch {
        return null;
      }
    },
    set(key: string, value: string) {
      try {
        window.localStorage.setItem(key, value);
      } catch {
        // Nothing to remember into; the choice holds for this load.
      }
    },
  };
}

/**
 * Wires a page's appearance controls to a theme control: the mode switch
 * and the light/dark theme selects (`theme-mode`, `theme-light`,
 * `theme-dark`), populated from `STATIC_DATA.theme` and kept in sync with
 * the root the stylesheet reads (the `svg` element in a file, `html` on a
 * served page). Every page that offers the appearance block wires it this
 * same way.
 */
function bootstrapControls(): ReturnType<typeof createTheme> {
  const modeSelect = DomAdapter.getElementById(
    'theme-mode',
  ) as HTMLSelectElement | null;
  const themeSelects = {
    light: DomAdapter.getElementById('theme-light') as HTMLSelectElement | null,
    dark: DomAdapter.getElementById('theme-dark') as HTMLSelectElement | null,
  };
  const darkScheme = window.matchMedia('(prefers-color-scheme: dark)');
  const themeControl = createTheme({
    themes: STATIC_DATA.theme,
    storage: localStorageAdapter(),
    systemDark: () => darkScheme.matches,
    onSystemChange: (listener) =>
      darkScheme.addEventListener('change', listener),
    root: {
      theme: document.documentElement.dataset.theme,
      mode: document.documentElement.dataset.mode,
    },
    applyTheme: (name) => {
      document.documentElement.dataset.theme = name;
    },
    showState: ({ mode, light, dark }) => {
      if (modeSelect) modeSelect.value = mode;
      if (themeSelects.light) themeSelects.light.value = light;
      if (themeSelects.dark) themeSelects.dark.value = dark;
    },
  });
  modeSelect?.addEventListener('change', () => {
    themeControl.setMode(modeSelect.value as 'light' | 'dark' | 'system');
  });
  for (const mode of ['light', 'dark'] as const) {
    const select = themeSelects[mode];
    if (!select) continue;
    for (const { name, label } of STATIC_DATA.theme[mode]) {
      const option = document.createElement('option');
      option.value = name;
      option.textContent = label;
      select.appendChild(option);
    }
    select.addEventListener('change', () => {
      themeControl.setThemeFor(mode, select.value);
    });
  }
  themeControl.start();
  return themeControl;
}

const Theme = { createTheme, localStorageAdapter, bootstrapControls };

export { createTheme, localStorageAdapter, bootstrapControls, Theme };
