// What the extension knows about the service without VS Code: the command
// line that starts it, the two stdout lines it speaks, the jump target, and
// the stdin lines the editor's events become.

const path = require('node:path');

/**
 * @typedef {object} Config
 * @property {string} binary the cargo-arc binary; a bare name is looked up in PATH
 * @property {string} manifestPath manifest to analyze; empty for the service's default
 * @property {string[]} features cargo features to activate
 * @property {boolean} includeTests include test code in the analysis
 * @property {boolean} externals include external crate dependencies
 */

/**
 * The command line that starts the service: the shared flags sit on `arc`,
 * before the subcommand, the port after it.
 * @param {Config} config
 * @param {number|null} port port to serve on; null lets the OS pick one
 * @returns {string[]}
 */
function argv(config, port) {
  const args = [config.binary, 'arc'];
  if (config.manifestPath) {
    args.push('--manifest-path', config.manifestPath);
  }
  if (config.features.length > 0) {
    args.push('--features', config.features.join(','));
  }
  if (config.includeTests) {
    args.push('--include-tests');
  }
  if (config.externals) {
    args.push('--externals');
  }
  args.push('ui');
  if (port !== null) {
    args.push('--port', String(port));
  }
  return args;
}

/**
 * @typedef {{ kind: 'ready', version: string, port: number }} Ready
 * @typedef {{ kind: 'jump', line: number, file: string }} Jump
 */

/**
 * One line of the service's stdout; null for a line that is neither.
 * @param {string} line
 * @returns {Ready|Jump|null}
 */
function parseLine(line) {
  const ready = line.match(/^arc ready (\S+) (\d+)$/);
  if (ready) {
    return { kind: 'ready', version: ready[1], port: Number(ready[2]) };
  }
  const jump = line.match(/^arc jump (\d+) (.+)$/);
  if (jump) {
    return { kind: 'jump', line: Number(jump[1]), file: jump[2] };
  }
  return null;
}

/**
 * The path to run: `binary` unchanged for a bare name, so `spawn` looks it
 * up in PATH; resolved against the workspace folder when it contains a path
 * separator. A settings value passes through no shell, so a leading `~` and
 * `$VAR` / `${VAR}` are expanded here; an unset variable stays literal.
 * @param {string} binary
 * @param {string} workspaceFolder absolute path
 * @param {NodeJS.ProcessEnv} [env]
 * @returns {string}
 */
function resolveBinary(binary, workspaceFolder, env = process.env) {
  const expanded = binary
    .replace(/^~(?=\/|$)/, () => env.HOME ?? '~')
    .replace(
      /\$(\w+)|\$\{(\w+)\}/g,
      (match, bare, braced) => env[bare ?? braced] ?? match,
    );
  if (expanded.includes('/') || expanded.includes(path.sep)) {
    return path.resolve(workspaceFolder, expanded);
  }
  return expanded;
}

/**
 * The 0-based line to place the cursor on for a jump: the service's 1-based
 * line, clamped to the last line of a file that has grown shorter since it
 * was analyzed.
 * @param {number} line 1-based line from the service
 * @param {number} lineCount number of lines in the document
 * @returns {number}
 */
function jumpLine(line, lineCount) {
  return Math.min(line, lineCount) - 1;
}

/**
 * The fields of VS Code's `TextDocument` and `TextEditor` the stdin lines
 * are built from.
 * @typedef {{ uri: { scheme: string, fsPath: string } }} TextDocument
 * @typedef {{ document: TextDocument, selection: { active: { line: number } } }} TextEditor
 */

/**
 * The absolute path of `document`, or null for a document that is no file
 * on disk, such as an untitled buffer.
 * @param {TextDocument} document
 * @returns {string|null}
 */
function fileOf(document) {
  return document.uri.scheme === 'file' ? document.uri.fsPath : null;
}

/**
 * Turn the editor's events into lines on the service's stdin, each passed
 * to `write`. A document that is no file writes nothing; the service
 * decides whether a file has a node or counts for a recompute.
 * @param {(line: string) => void} write
 * @returns {{ activeEditorChanged: (editor: TextEditor|undefined) => void, windowFocused: (state: { focused: boolean }, editor: TextEditor|undefined) => void, saved: (document: TextDocument) => void }}
 */
function editorEvents(write) {
  /**
   * @param {TextEditor|undefined} editor undefined when no editor is active
   */
  const focus = (editor) => {
    const file = editor && fileOf(editor.document);
    if (file) {
      write(`arc focus ${editor.selection.active.line + 1} ${file}`);
    }
  };
  return {
    activeEditorChanged: focus,
    windowFocused(state, editor) {
      if (state.focused) {
        focus(editor);
      }
    },
    saved(document) {
      const file = fileOf(document);
      if (file) {
        write(`arc saved ${file}`);
      }
    },
  };
}

module.exports = { argv, parseLine, resolveBinary, jumpLine, editorEvents };
