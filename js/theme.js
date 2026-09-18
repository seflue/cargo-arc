// @module Theme
// @deps
// @config
// theme.js - Chooses the theme the page shows: a mode (light, dark or the
// system's) and a theme per mode, from the editor, the root attributes a
// pinned theme left, the remembered choices and the system setting.

/**
 * @typedef {'light' | 'dark'} Mode
 * @typedef {Mode | 'system'} ModeChoice
 * @typedef {{ name: string, label: string }} ThemeName
 * @typedef {{ light: ThemeName[], dark: ThemeName[] }} Themes
 *
 * @typedef {object} ThemeHooks
 * @property {Themes} themes - the shipped themes per mode, the default first.
 * @property {{ get: (key: string) => string | null, set: (key: string, value: string) => void }} storage -
 *   remembers the switch and the theme per mode across loads.
 * @property {() => boolean} systemDark - whether the system prefers dark.
 * @property {(listener: () => void) => void} onSystemChange - calls the
 *   listener when the system setting changes.
 * @property {{ theme?: string, mode?: string }} root - what the page root
 *   carried at load: a theme pinned by name (`--theme`) and the mode the
 *   editor sent (`arc theme`).
 * @property {(name: string) => void} applyTheme - puts the theme on the root.
 * @property {(state: { mode: ModeChoice, light: string, dark: string }) => void} showState -
 *   reflects the switch and the theme per mode on the toolbar.
 */

const MODE_KEY = 'arc.theme.mode';
const MODES = /** @type {Mode[]} */ (['light', 'dark']);

/**
 * @param {ThemeHooks} hooks
 */
function createTheme({
  themes,
  storage,
  systemDark,
  onSystemChange,
  root,
  applyTheme,
  showState,
}) {
  /** The switch: an override, or system, which leaves the rest of the chain to decide. */
  /** @type {ModeChoice} */
  let choice = 'system';
  /** The last mode the editor sent; what system follows in a webview. */
  /** @type {Mode | null} */
  let editorMode = null;
  /** @type {Record<Mode, string>} */
  const chosen = { light: themes.light[0].name, dark: themes.dark[0].name };
  /** A theme pinned on the root, until the page chooses one for its mode. */
  /** @type {string | null} */
  let pinned = null;

  /** @param {string | null | undefined} name */
  function modeOf(name) {
    return MODES.find((mode) => themes[mode].some((t) => t.name === name));
  }

  /** @param {Mode} mode */
  function themeFor(mode) {
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

  /** @param {ModeChoice} mode */
  function setMode(mode) {
    choice = mode;
    storage.set(MODE_KEY, mode);
    apply();
  }

  /** @param {Mode} mode */
  function followEditor(mode) {
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
    /**
     * @param {Mode} mode
     * @param {string} name
     */
    setThemeFor(mode, name) {
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
    /** @param {string} mode - the mode the editor sent during the session. */
    handleEditorMode(mode) {
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
    /** @param {string} key */
    get(key) {
      try {
        return window.localStorage.getItem(key);
      } catch {
        return null;
      }
    },
    /**
     * @param {string} key
     * @param {string} value
     */
    set(key, value) {
      try {
        window.localStorage.setItem(key, value);
      } catch {
        // Nothing to remember into; the choice holds for this load.
      }
    },
  };
}

// The browser global exposing this module's API.
const Theme = { createTheme, localStorageAdapter };

// CommonJS export for tests (Node/Bun)
if (typeof module !== 'undefined') {
  module.exports = { createTheme, localStorageAdapter, Theme };
}
