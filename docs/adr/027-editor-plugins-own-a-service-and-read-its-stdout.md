# ADR-027: Editor Plugins Own a Service Process and Read Its stdout

- **Status:** Active
- **Decided:** 2026-08-28
- **Related:** ADR-006 (self-contained SVG), ADR-026 (plugins build their CLI)
- Corrected 2026-10-13: stdout is not the only channel. The plugin writes commands to the service's stdin, the page receives server-sent events from the service, and stdout carries four line shapes, not two.

## Context

The diagram should live beside the code in an editor, and a click on a node or a reference should open the file at its line. Three editors were in scope from the start: Neovim, VS Code and RustRover.

Each editor has its own way to be told to open a file at a line: an RPC socket, a URL scheme, a message from a webview. The diagram is one page with one script, shown in a browser tab today.

## Decision

`cargo arc ui` serves the diagram page over HTTP on the loopback interface and resolves jump ids. The page uses only relative URLs, so it is same-origin with its own server wherever it is shown.

A plugin has three jobs: it starts and ends the service process, it shows the page, and it opens the file the service names. All information reaches the plugin through the process's stdout.
It carries the version and port once, one line per jump with file and line, and the state of the analysis.
The plugin sends commands to the service through the process's stdin.

There is no communication between the page and the plugin. A click sends a request to the service, which writes the jump line.
The service pushes changes to the page as server-sent events.

## Rationale

- One frontend for every editor. Wiring an editor's own channel into the page would give the frontend one case per editor; this way the page has one target, the service that serves it, and the editor-specific part is the plugin.
- stdout is the one channel every editor can read from a child process without a library.
- The service announces its port instead of the plugin choosing one, so a start never races a port that is already taken.
- The page carries jump ids, not paths, and the service resolves them against the workspace it ran in. A stale diagram cannot open the wrong file.

## Consequences

- A fourth editor needs a plugin of the same shape and no change here.
- The same page serves a browser tab, a webview and an embedded browser.
- The service lives as long as the plugin keeps the process. There is no daemon; every editor window runs its own.
- A restart is a new process and a new layout; the open page must reload, and its old jump ids are gone.
- The stdout contract is plain text. Any other line written to stdout would break a plugin's parser; new information has to be a new line shape, and every plugin has to learn it.
- The editor reaches the diagram only through the service. A command on stdin that concerns the page reaches it as an event from the service.

## References

- `src/ui/`: the service and its HTTP transport.
- `editors/nvim/`, `editors/vscode/`, `editors/rustrover/`: the three plugins.
