// Three jobs: start and own the cargo-arc service, embed its page in a
// webview, and jump into the code when the page asks for it.

const vscode = require('vscode');
const { spawn } = require('node:child_process');
const readline = require('node:readline');
const path = require('node:path');
const { argv, parseLine, resolveBinary, jumpLine } = require('./service.js');

/**
 * The running service: its process, once it has announced itself the
 * version and port from its ready line, the port of the service it
 * replaces, whether its exit was asked for, whether a restart waits for it,
 * and whether the browser is to open once it is ready.
 * @type {{ process: import('node:child_process').ChildProcess, version: string|null, port: number|null, replaces: number|null, stopping: boolean, restart: boolean, spawnFailed: boolean, browser: boolean }|null}
 */
let service = null;

/** @type {import('vscode').WebviewPanel|null} */
let panel = null;

/** @type {import('vscode').OutputChannel|null} */
let output = null;

/**
 * @param {import('vscode').ExtensionContext} context
 */
function activate(context) {
  output = vscode.window.createOutputChannel('cargo-arc');
  context.subscriptions.push(
    output,
    vscode.commands.registerCommand('cargoArc.open', () => open(context)),
    vscode.commands.registerCommand('cargoArc.openInBrowser', () =>
      openInBrowser(),
    ),
    vscode.commands.registerCommand('cargoArc.restart', () => restart(context)),
    vscode.commands.registerCommand('cargoArc.stop', () => endService()),
  );
}

function deactivate() {
  endService();
}

/**
 * The first workspace folder, which the service analyzes; null, with an
 * error shown, when the window has none.
 * @returns {import('vscode').WorkspaceFolder|null}
 */
function workspaceFolder() {
  const folder = vscode.workspace.workspaceFolders?.[0];
  if (!folder) {
    vscode.window.showErrorMessage('cargo-arc: open a workspace folder first');
  }
  return folder ?? null;
}

/**
 * Shows the panel, creating it if needed, and starts the service unless
 * one is running; a panel created beside a running service shows its page
 * right away.
 * @param {import('vscode').ExtensionContext} context
 */
function open(context) {
  const folder = workspaceFolder();
  if (!folder) {
    return;
  }

  if (panel) {
    panel.reveal();
  } else {
    panel = vscode.window.createWebviewPanel(
      'cargoArc',
      'cargo-arc',
      vscode.ViewColumn.Beside,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
      },
    );
    panel.onDidDispose(
      () => {
        panel = null;
        endService();
      },
      null,
      context.subscriptions,
    );
    if (service?.port) {
      showFrame(service.port);
    }
  }

  if (!service) {
    startService(folder, null);
  }
}

/**
 * Opens the page in the system browser, for a second screen. Without a
 * running service, starts one and opens the browser once it is ready.
 */
function openInBrowser() {
  if (service?.port) {
    openExternal(service.port);
    return;
  }
  if (service) {
    vscode.window.showInformationMessage('cargo-arc is still starting');
    return;
  }
  const folder = workspaceFolder();
  if (!folder) {
    return;
  }
  startService(folder, null);
  service.browser = true;
}

/**
 * @param {number} port
 */
function openExternal(port) {
  vscode.env.openExternal(vscode.Uri.parse(`http://127.0.0.1:${port}/`));
}

/**
 * Ends the service and starts a new one on the same port once it has
 * exited; the iframe reloads automatically once the new service announces
 * itself ready, showing the code as it is now.
 * @param {import('vscode').ExtensionContext} context
 */
function restart(context) {
  if (!service) {
    open(context);
    return;
  }
  service.restart = true;
  service.stopping = true;
  service.process.kill();
}

/**
 * Ends the service without starting a new one, even if a restart was
 * pending for it.
 */
function endService() {
  if (service) {
    service.restart = false;
    service.stopping = true;
    service.process.kill();
  }
}

/**
 * Starts the service for the given workspace folder. With `replacesPort`,
 * the port of the service that just exited, the new one takes that port.
 * @param {import('vscode').WorkspaceFolder} folder
 * @param {number|null} replacesPort
 */
function startService(folder, replacesPort) {
  if (panel) {
    panel.webview.html = '<!doctype html><body>starting cargo-arc…</body>';
  }

  const settings = vscode.workspace.getConfiguration('cargoArc');
  const config = {
    binary: resolveBinary(settings.get('binary'), folder.uri.fsPath),
    manifestPath: settings.get('manifestPath'),
    features: settings.get('features'),
    includeTests: settings.get('includeTests'),
    externals: settings.get('externals'),
  };
  const [command, ...args] = argv(config, replacesPort);

  const current = {
    replaces: replacesPort,
    stopping: false,
    restart: false,
    version: null,
    port: null,
    spawnFailed: false,
    browser: false,
  };
  const child = spawn(command, args, { cwd: folder.uri.fsPath });
  current.process = child;
  service = current;

  child.on('error', (err) => {
    current.spawnFailed = true;
    if (service === current) {
      service = null;
    }
    vscode.window.showErrorMessage(
      `cargo-arc: ${config.binary}: ${err.message}`,
    );
  });

  child.on('exit', (code, signal) => {
    if (service === current) {
      service = null;
    }
    if (current.port === null && !current.stopping && !current.spawnFailed) {
      vscode.window.showErrorMessage(
        `cargo-arc exited with ${code ?? signal} before announcing a port`,
      );
      output.show();
    }
    if (current.restart) {
      startService(folder, current.port);
    }
  });

  readline.createInterface({ input: child.stderr }).on('line', (line) => {
    output.appendLine(line);
  });
  readline.createInterface({ input: child.stdout }).on('line', (line) => {
    handleLine(line, current);
  });
}

/**
 * Handles one line of the service's stdout.
 * @param {string} line
 * @param {NonNullable<typeof service>} current
 */
function handleLine(line, current) {
  const parsed = parseLine(line);
  if (!parsed) {
    return;
  }
  if (parsed.kind === 'ready') {
    current.version = parsed.version;
    current.port = parsed.port;
    showFrame(current.port);
    if (current.browser) {
      openExternal(current.port);
    }
    return;
  }
  jumpTo(parsed.file, parsed.line).catch((err) => {
    vscode.window.showErrorMessage(
      `cargo-arc: cannot open ${parsed.file}: ${err.message}`,
    );
  });
}

/**
 * Fills the panel with an iframe on the service's port. Called on first
 * start and again after a restart, since a webview has no reload button of
 * its own.
 * @param {number} port
 */
async function showFrame(port) {
  const external = await vscode.env.asExternalUri(
    vscode.Uri.parse(`http://127.0.0.1:${port}/`),
  );
  if (!panel) {
    return;
  }
  panel.webview.html = `<!doctype html>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy"
      content="default-src 'none'; frame-src http: https:; style-src 'unsafe-inline';">
<style>
  html, body, iframe { height: 100%; width: 100%; margin: 0; border: 0; }
</style>
<iframe src="${external}" sandbox="allow-scripts allow-same-origin allow-forms"></iframe>`;
}

/**
 * Puts the cursor on `line` of `file`: in the editor that already shows the
 * file, otherwise in a newly opened one. A line past the end of the file is
 * clamped to the last line. Focus stays in the diagram, so clicking through
 * the nodes does not pull it into the editor window each time.
 * @param {string} file absolute path
 * @param {number} line 1-based
 */
async function jumpTo(file, line) {
  // fsPath separators can differ from the service's own rendering of the
  // same absolute path.
  const target = path.normalize(file);
  let editor = vscode.window.visibleTextEditors.find(
    (e) => path.normalize(e.document.uri.fsPath) === target,
  );

  if (editor) {
    editor = await vscode.window.showTextDocument(editor.document, {
      viewColumn: editor.viewColumn,
      preserveFocus: true,
    });
  } else {
    const document = await vscode.workspace.openTextDocument(
      vscode.Uri.file(file),
    );
    const column =
      vscode.window.visibleTextEditors[0]?.viewColumn ?? vscode.ViewColumn.One;
    editor = await vscode.window.showTextDocument(document, {
      viewColumn: column,
      preserveFocus: true,
    });
  }

  const position = new vscode.Position(
    jumpLine(line, editor.document.lineCount),
    0,
  );
  editor.selection = new vscode.Selection(position, position);
  editor.revealRange(
    new vscode.Range(position, position),
    vscode.TextEditorRevealType.InCenterIfOutsideViewport,
  );
}

module.exports = { activate, deactivate };
