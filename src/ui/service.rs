//! The jump service's HTTP-independent logic: the page, the table, the two
//! lines it writes to stdout, the three it reads from stdin, and the three
//! events it pushes to the page.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

use crate::layout::{JumpTable, Location, LocationId};
use crate::render::{Appearance, Mode, Theme, html_page, project_name};

/// Whether the page follows the editor's cursor. Passed through from stdin
/// to the page; the service holds no state of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FollowState {
    On,
    Off,
}

/// One line the editor wrote to stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StdinCommand {
    /// The cursor stands on `line` of `file` (absolute).
    Focus {
        line: usize,
        file: PathBuf,
    },
    Follow(FollowState),
    /// The editor's colour mode; it sends no colours of its own.
    Theme(Mode),
}

/// What the page needs to select the node the editor is in: the node's
/// `STATIC_DATA` key, and the ids of the jump targets at the cursor line,
/// so the sidebar can open their rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FocusEvent {
    pub node: String,
    pub jumps: Vec<LocationId>,
}

/// Parses one stdin line. Anything but the three known forms is `None`:
/// stdin is a protocol, not a shell, and a stray line is dropped without
/// notice.
pub(crate) fn parse_stdin(line: &str) -> Option<StdinCommand> {
    if let Some(rest) = line.strip_prefix("arc focus ") {
        let (line, file) = rest.split_once(' ')?;
        let line = line.parse().ok()?;
        return Some(StdinCommand::Focus {
            line,
            file: PathBuf::from(file),
        });
    }
    if let Some(rest) = line.strip_prefix("arc follow ") {
        return match rest {
            "on" => Some(StdinCommand::Follow(FollowState::On)),
            "off" => Some(StdinCommand::Follow(FollowState::Off)),
            _ => None,
        };
    }
    let mode = Mode::parse(line.strip_prefix("arc theme ")?)?;
    Some(StdinCommand::Theme(mode))
}

/// Serves the diagram page and resolves jump ids against the workspace root
/// that produced them.
pub(crate) struct JumpService {
    svg: String,
    table: JumpTable,
    root: PathBuf,
    /// The theme `--theme` pinned, if any.
    theme: Option<&'static Theme>,
    /// The last mode the editor sent. Held so that a page loaded after the
    /// line, or reloaded, starts in the editor's mode.
    editor_mode: Mutex<Option<Mode>>,
}

impl JumpService {
    /// `svg` is the rendered diagram as `cargo arc -o` would write it.
    pub(crate) fn new(
        svg: String,
        table: JumpTable,
        root: PathBuf,
        theme: Option<&'static Theme>,
    ) -> Self {
        Self {
            svg,
            table,
            root,
            theme,
            editor_mode: Mutex::new(None),
        }
    }

    /// The diagram as the page from [`html_page`]: a webview shrinks a bare
    /// SVG document to its frame, an inline one keeps its size. The root
    /// carries the editor's mode and the pinned theme.
    pub(crate) fn page(&self) -> String {
        let appearance = Appearance {
            theme: self.theme,
            mode: *self.editor_mode(),
        };
        html_page(&self.svg, project_name(&self.root), appearance)
    }

    /// Records the editor's mode; `true` when it differs from the last one.
    pub(crate) fn set_editor_mode(&self, mode: Mode) -> bool {
        self.editor_mode().replace(mode) != Some(mode)
    }

    fn editor_mode(&self) -> std::sync::MutexGuard<'_, Option<Mode>> {
        self.editor_mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Resolves `id` against the table and anchors the result at the
    /// workspace root. A table entry that is already absolute is
    /// unaffected: `Path::join` on an absolute path replaces the base
    /// entirely.
    pub(crate) fn jump(&self, id: usize) -> Option<Location> {
        let location = self.table.resolve(LocationId::from(id))?;
        Some(Location {
            file: self.root.join(&location.file),
            line: location.line,
        })
    }

    /// The node `path` belongs to and the jump targets on its `line`. The
    /// table holds paths as the run had them, absolute or workspace-relative
    /// depending on where they were collected; the editor sends an absolute
    /// one, so both spellings are looked up; a path outside the root has only
    /// the absolute one. `None` when the path belongs to no node.
    pub(crate) fn focus(&self, line: usize, path: &Path) -> Option<FocusEvent> {
        let spellings: Vec<&Path> = std::iter::once(path)
            .chain(path.strip_prefix(&self.root).ok())
            .collect();
        let node = spellings.iter().find_map(|file| self.table.node_at(file))?;
        Some(FocusEvent {
            node: node.to_string(),
            jumps: spellings
                .iter()
                .flat_map(|file| self.table.ids_at(file, line))
                .collect(),
        })
    }

    pub(crate) fn ready_line(port: u16) -> String {
        format!("arc ready {} {port}\n", env!("CARGO_PKG_VERSION"))
    }

    pub(crate) fn jump_line(location: &Location) -> String {
        format!("arc jump {} {}\n", location.line, location.file.display())
    }

    /// The `focus` event as one complete SSE block.
    pub(crate) fn focus_event(event: &FocusEvent) -> String {
        let data = serde_json::to_string(event).expect("a focus event has no unserializable field");
        format!("event: focus\ndata: {data}\n\n")
    }

    /// The `follow` event as one complete SSE block.
    pub(crate) fn follow_event(state: FollowState) -> String {
        let data = match state {
            FollowState::On => "on",
            FollowState::Off => "off",
        };
        format!("event: follow\ndata: {data}\n\n")
    }

    /// The `theme` event as one complete SSE block.
    pub(crate) fn theme_event(mode: Mode) -> String {
        format!("event: theme\ndata: {}\n\n", mode.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Two crates' manifests (`my_crate` at id 0, `other` at id 1) and one
    /// workspace-relative source location (id 2), the way `build_layout`
    /// would have assigned them.
    fn service_over_root(root: &str) -> JumpService {
        let mut table = JumpTable::new();
        table.insert(PathBuf::from("/ws/my_crate/Cargo.toml"), 1);
        table.insert(PathBuf::from("/ws/other/Cargo.toml"), 1);
        table.insert(PathBuf::from("src/lib.rs"), 3);
        JumpService::new(String::new(), table, PathBuf::from(root), None)
    }

    #[test]
    fn jump_joins_a_relative_table_entry_with_the_root() {
        let service = service_over_root("/ws");
        assert_eq!(
            service.jump(2),
            Some(Location {
                file: PathBuf::from("/ws/src/lib.rs"),
                line: 3,
            })
        );
    }

    #[test]
    fn jump_leaves_an_absolute_table_entry_unchanged() {
        let service = service_over_root("/ws");
        assert_eq!(
            service.jump(0),
            Some(Location {
                file: PathBuf::from("/ws/my_crate/Cargo.toml"),
                line: 1,
            })
        );
    }

    #[test]
    fn jump_returns_none_for_an_unknown_id() {
        let service = service_over_root("/ws");
        assert_eq!(service.jump(99), None);
    }

    #[test]
    fn page_is_the_html_page_of_the_svg() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
        let service = JumpService::new(
            svg.to_string(),
            JumpTable::new(),
            PathBuf::from("/ws"),
            None,
        );
        assert_eq!(
            service.page(),
            html_page(svg, Some("ws"), Appearance::default())
        );
    }

    #[test]
    fn ready_line_names_the_crate_version_and_port() {
        assert_eq!(
            JumpService::ready_line(4321),
            format!("arc ready {} 4321\n", env!("CARGO_PKG_VERSION"))
        );
    }

    /// One crate whose manifest is registered absolute (node "0"), one
    /// module whose file is registered workspace-relative (node "1"), with
    /// two edge locations on `src/mod_a.rs`: ids 2 and 3 on line 5, id 4 on
    /// line 9; and one member outside the root, registered absolute (node
    /// "2", id 6).
    fn focus_service() -> JumpService {
        let mut table = JumpTable::new();
        let manifest = table.insert(PathBuf::from("/ws/my_crate/Cargo.toml"), 1);
        table.insert_node_files("0", [manifest]);
        let module = table.insert(PathBuf::from("src/mod_a.rs"), 1);
        table.insert_node_files("1", [module]);
        table.insert(PathBuf::from("src/mod_a.rs"), 5);
        table.insert(PathBuf::from("src/mod_a.rs"), 5);
        table.insert(PathBuf::from("src/mod_a.rs"), 9);
        table.insert(PathBuf::from("my_crate/Cargo.toml"), 1);
        let member = table.insert(PathBuf::from("/elsewhere/member/Cargo.toml"), 1);
        table.insert_node_files("2", [member]);
        JumpService::new(String::new(), table, PathBuf::from("/ws"), None)
    }

    /// The node is registered under one spelling of the file and a location
    /// under the other; the focus carries both.
    #[test]
    fn focus_collects_jumps_under_both_spellings_of_the_file() {
        let service = focus_service();
        assert_eq!(
            service.focus(1, Path::new("/ws/my_crate/Cargo.toml")),
            Some(FocusEvent {
                node: "0".to_string(),
                jumps: vec![LocationId::from(0), LocationId::from(5)],
            })
        );
    }

    #[test]
    fn focus_resolves_an_absolute_path_against_a_relative_table_entry() {
        let service = focus_service();
        assert_eq!(
            service.focus(5, Path::new("/ws/src/mod_a.rs")),
            Some(FocusEvent {
                node: "1".to_string(),
                jumps: vec![LocationId::from(2), LocationId::from(3)],
            })
        );
    }

    /// A workspace member outside the root (`members = ["../member"]`) is
    /// registered absolute and cannot be spelled relative to the root.
    #[test]
    fn focus_resolves_an_absolute_path_outside_the_root() {
        let service = focus_service();
        assert_eq!(
            service.focus(1, Path::new("/elsewhere/member/Cargo.toml")),
            Some(FocusEvent {
                node: "2".to_string(),
                jumps: vec![LocationId::from(6)],
            })
        );
    }

    #[test]
    fn focus_returns_none_for_a_path_without_a_node() {
        let service = focus_service();
        assert_eq!(service.focus(1, Path::new("/elsewhere/src/mod_a.rs")), None);
        assert_eq!(service.focus(1, Path::new("/ws/src/unknown.rs")), None);
    }

    #[test]
    fn focus_on_a_line_without_locations_carries_no_jumps() {
        let service = focus_service();
        assert_eq!(
            service.focus(42, Path::new("/ws/src/mod_a.rs")),
            Some(FocusEvent {
                node: "1".to_string(),
                jumps: vec![],
            })
        );
    }

    #[test]
    fn parse_stdin_reads_focus_lines_with_the_path_last() {
        assert_eq!(
            parse_stdin("arc focus 12 /tmp/a b/lib.rs"),
            Some(StdinCommand::Focus {
                line: 12,
                file: PathBuf::from("/tmp/a b/lib.rs"),
            })
        );
    }

    #[test]
    fn parse_stdin_reads_follow_lines() {
        assert_eq!(
            parse_stdin("arc follow on"),
            Some(StdinCommand::Follow(FollowState::On))
        );
        assert_eq!(
            parse_stdin("arc follow off"),
            Some(StdinCommand::Follow(FollowState::Off))
        );
    }

    #[test]
    fn parse_stdin_reads_theme_lines() {
        assert_eq!(
            parse_stdin("arc theme light"),
            Some(StdinCommand::Theme(Mode::Light))
        );
        assert_eq!(
            parse_stdin("arc theme dark"),
            Some(StdinCommand::Theme(Mode::Dark))
        );
    }

    #[test]
    fn theme_event_formats_as_an_sse_block() {
        assert_eq!(
            JumpService::theme_event(Mode::Dark),
            "event: theme\ndata: dark\n\n"
        );
    }

    /// The editor's mode goes on the page root as data-mode, where the
    /// stylesheet picks the mode's default theme before any script runs.
    #[test]
    fn page_carries_the_editor_mode_once_set() {
        let service = service_over_root("/ws");
        assert!(!service.page().contains("data-mode"));
        service.set_editor_mode(Mode::Dark);
        assert!(
            service
                .page()
                .contains("<html xmlns=\"http://www.w3.org/1999/xhtml\" data-mode=\"dark\">"),
            "{}",
            service.page()
        );
    }

    /// A theme pinned by --theme keeps its name on the root only while the
    /// editor's mode agrees with it: the editor decides the mode.
    #[test]
    fn editor_mode_drops_a_pinned_theme_of_the_other_mode() {
        let mut table = JumpTable::new();
        table.insert(PathBuf::from("src/lib.rs"), 3);
        let service = JumpService::new(
            String::new(),
            table,
            PathBuf::from("/ws"),
            Theme::named("mocha"),
        );
        assert!(service.page().contains("data-theme=\"mocha\""));
        service.set_editor_mode(Mode::Dark);
        assert!(service.page().contains("data-theme=\"mocha\""));
        service.set_editor_mode(Mode::Light);
        assert!(!service.page().contains("data-theme"), "{}", service.page());
    }

    #[test]
    fn parse_stdin_ignores_malformed_and_foreign_lines() {
        assert_eq!(parse_stdin("arc focus x /p"), None);
        assert_eq!(parse_stdin("arc focus 3"), None);
        assert_eq!(parse_stdin("arc follow maybe"), None);
        assert_eq!(parse_stdin("arc theme blue"), None);
        assert_eq!(parse_stdin("arc jump 3 /p"), None);
        assert_eq!(parse_stdin("hello"), None);
        assert_eq!(parse_stdin(""), None);
    }

    #[test]
    fn focus_event_formats_as_an_sse_block() {
        let event = FocusEvent {
            node: "7".to_string(),
            jumps: vec![LocationId::from(2), LocationId::from(3)],
        };
        assert_eq!(
            JumpService::focus_event(&event),
            "event: focus\ndata: {\"node\":\"7\",\"jumps\":[2,3]}\n\n"
        );
    }

    #[test]
    fn follow_event_formats_as_an_sse_block() {
        assert_eq!(
            JumpService::follow_event(FollowState::On),
            "event: follow\ndata: on\n\n"
        );
        assert_eq!(
            JumpService::follow_event(FollowState::Off),
            "event: follow\ndata: off\n\n"
        );
    }

    #[test]
    fn jump_line_puts_the_path_after_the_line_number() {
        let location = Location {
            file: PathBuf::from("/tmp/a b/lib.rs"),
            line: 7,
        };
        assert_eq!(
            JumpService::jump_line(&location),
            "arc jump 7 /tmp/a b/lib.rs\n"
        );
    }
}
