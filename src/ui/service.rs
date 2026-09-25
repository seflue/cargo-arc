//! The jump service's HTTP-independent logic: the page, the table, the
//! lines it writes to stdout, the commands it reads from stdin and the
//! page, and the events it pushes to the page.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};

use anyhow::Result;
use serde::Serialize;

use crate::layout::{JumpTable, Location, LocationId};
use crate::render::{AnalysisSwitches, Appearance, Mode, Theme, html_page, project_name};

/// Whether the page follows the editor's cursor. Passed through from stdin
/// to the page; the service holds no state of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FollowState {
    On,
    Off,
}

/// One of the two analysis inputs that can change while the service runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Switch {
    /// External crates in the analysis (`--externals`).
    Externals,
    /// Test code in the analysis (`--include-tests`).
    Tests,
}

impl Switch {
    /// `switches` with this one set to `on`.
    pub(crate) fn set(self, switches: AnalysisSwitches, on: bool) -> AnalysisSwitches {
        match self {
            Self::Externals => AnalysisSwitches {
                externals: on,
                ..switches
            },
            Self::Tests => AnalysisSwitches {
                tests: on,
                ..switches
            },
        }
    }
}

/// One command line, from the editor on stdin or from the page as the body
/// of `POST /command`; both spell it the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    /// The cursor stands on `line` of `file` (absolute).
    Focus {
        line: usize,
        file: PathBuf,
    },
    Follow(FollowState),
    /// The editor's colour mode; it sends no colours of its own.
    Theme(Mode),
    /// Turn an analysis switch on or off; the service recomputes.
    Switch {
        switch: Switch,
        on: bool,
    },
    /// The editor wrote `file` (absolute) to disk.
    Saved(PathBuf),
    /// Run the analysis again with the served switches.
    Recompute,
    /// Whether a save starts a run.
    OnSave(bool),
}

/// What the page needs to select the node the editor is in: the arc node's
/// `STATIC_DATA` key (used only by the arc page), the file the cursor is in
/// as either page's own `STATIC_DATA` keys it, and the ids of the jump
/// targets at the cursor line so the sidebar can open their rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FocusEvent {
    pub node: String,
    pub file: String,
    pub jumps: Vec<LocationId>,
}

/// Parses one command line. Anything but the known forms is `None`: the
/// line is a protocol, not a shell, and a stray one is dropped without
/// notice.
pub(crate) fn parse_command(line: &str) -> Option<Command> {
    if let Some(rest) = line.strip_prefix("arc focus ") {
        let (line, file) = rest.split_once(' ')?;
        let line = line.parse().ok()?;
        return Some(Command::Focus {
            line,
            file: PathBuf::from(file),
        });
    }
    if let Some(rest) = line.strip_prefix("arc theme ") {
        return Some(Command::Theme(Mode::parse(rest)?));
    }
    if let Some(file) = line.strip_prefix("arc saved ") {
        return (!file.is_empty()).then(|| Command::Saved(PathBuf::from(file)));
    }
    if line == "arc recompute" {
        return Some(Command::Recompute);
    }
    let (word, state) = line.strip_prefix("arc ")?.split_once(' ')?;
    let on = match state {
        "on" => true,
        "off" => false,
        _ => return None,
    };
    match word {
        "follow" => Some(Command::Follow(if on {
            FollowState::On
        } else {
            FollowState::Off
        })),
        "externals" => Some(Command::Switch {
            switch: Switch::Externals,
            on,
        }),
        "tests" => Some(Command::Switch {
            switch: Switch::Tests,
            on,
        }),
        "on-save" => Some(Command::OnSave(on)),
        _ => None,
    }
}

/// One analysis run's output: the diagram as `cargo arc -o` would write it.
pub(crate) struct Diagram {
    pub svg: String,
}

/// Both pages built from one analysis run: the arc diagram and the hotspot
/// map, plus the one jump table the run assigned every id into; either
/// resolves a jump id or a focused file.
pub(crate) struct Pages {
    pub arc: Diagram,
    pub hotspots: Diagram,
    pub table: JumpTable,
}

/// Which page a request is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Arc,
    Hotspots,
}

/// Runs the analysis with the given switches and renders both pages. The
/// service gets it as a closure because the analysis lives in `cli`, which
/// `ui` must not import.
pub(crate) type Recompute<'a> = Box<dyn Fn(AnalysisSwitches) -> Result<Pages> + Send + Sync + 'a>;

/// What one run produced and the switches it ran with; replaced as a whole.
struct Current {
    pages: Pages,
    switches: AnalysisSwitches,
}

/// Serves the diagram page, resolves jump ids against the workspace root
/// that produced them, and recomputes both when a switch changes.
pub(crate) struct JumpService<'a> {
    /// Read by every request, written only by the swap after a run.
    current: RwLock<Current>,
    root: PathBuf,
    /// The theme `--theme` pinned, if any.
    theme: Option<&'static Theme>,
    /// The last mode the editor sent. Held so that a page loaded after the
    /// line, or reloaded, starts in the editor's mode.
    editor_mode: Mutex<Option<Mode>>,
    /// Whether a save starts a run; on until a command turns it off.
    on_save: AtomicBool,
    recompute: Recompute<'a>,
}

impl<'a> JumpService<'a> {
    pub(crate) fn new(
        pages: Pages,
        switches: AnalysisSwitches,
        root: PathBuf,
        theme: Option<&'static Theme>,
        recompute: Recompute<'a>,
    ) -> Self {
        Self {
            current: RwLock::new(Current { pages, switches }),
            root,
            theme,
            editor_mode: Mutex::new(None),
            on_save: AtomicBool::new(true),
            recompute,
        }
    }

    fn current(&self) -> std::sync::RwLockReadGuard<'_, Current> {
        self.current.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// `page` as the page from [`html_page`]: a webview shrinks a bare SVG
    /// document to its frame, an inline one keeps its size. The root carries
    /// the editor's mode and the pinned theme, so the page is rendered per
    /// request rather than once per run.
    pub(crate) fn page(&self, page: Page) -> String {
        let appearance = Appearance {
            theme: self.theme,
            mode: *self.editor_mode(),
        };
        let current = self.current();
        let svg = match page {
            Page::Arc => &current.pages.arc.svg,
            Page::Hotspots => &current.pages.hotspots.svg,
        };
        html_page(svg, project_name(&self.root), appearance)
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

    pub(crate) fn on_save(&self) -> bool {
        self.on_save.load(Ordering::Relaxed)
    }

    pub(crate) fn set_on_save(&self, on: bool) {
        self.on_save.store(on, Ordering::Relaxed);
    }

    /// Whether the analysis reads `path` (absolute): a Rust source file or a
    /// manifest under the workspace root.
    pub(crate) fn reads(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
            && (path.extension().is_some_and(|ext| ext == "rs")
                || path.file_name().is_some_and(|name| name == "Cargo.toml"))
    }

    /// The switches the served page was computed with.
    pub(crate) fn switches(&self) -> AnalysisSwitches {
        self.current().switches
    }

    /// Runs the analysis for `switches` and swaps both pages in. The lock
    /// is taken for the swap only, not for the run; on an error nothing
    /// changes.
    pub(crate) fn recompute(&self, switches: AnalysisSwitches) -> Result<()> {
        let pages = (self.recompute)(switches)?;
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        *current = Current { pages, switches };
        Ok(())
    }

    /// Resolves `id` against the shared table and anchors the result at the
    /// workspace root. A table entry that is already absolute is
    /// unaffected: `Path::join` on an absolute path replaces the base
    /// entirely.
    pub(crate) fn jump(&self, id: usize) -> Option<Location> {
        let current = self.current();
        let location = current.pages.table.resolve(LocationId::from(id))?;
        Some(Location {
            file: self.root.join(&location.file),
            line: location.line,
        })
    }

    /// The arc node `path` belongs to and the jump targets on its `line`.
    /// The table holds paths as the run had them, absolute or
    /// workspace-relative depending on where they were collected; the
    /// editor sends an absolute one, so both spellings are looked up; a path
    /// outside the root has only the absolute one. `None` when the path
    /// belongs to no node. `file` carries the path workspace-relative, or
    /// absolute when it lies outside the root, the same convention either
    /// page's `STATIC_DATA` uses, so each page resolves it against its own
    /// data independently of `node`, which only the arc page reads.
    pub(crate) fn focus(&self, line: usize, path: &Path) -> Option<FocusEvent> {
        let spellings: Vec<&Path> = std::iter::once(path)
            .chain(path.strip_prefix(&self.root).ok())
            .collect();
        let current = self.current();
        let node = spellings
            .iter()
            .find_map(|file| current.pages.table.node_at(file))?;
        Some(FocusEvent {
            node: node.to_string(),
            file: workspace_relative(path, &self.root),
            jumps: spellings
                .iter()
                .flat_map(|file| current.pages.table.ids_at(file, line))
                .collect(),
        })
    }

    pub(crate) fn ready_line(port: u16) -> String {
        format!("arc ready {} {port}\n", env!("CARGO_PKG_VERSION"))
    }

    pub(crate) fn jump_line(location: &Location) -> String {
        format!("arc jump {} {}\n", location.line, location.file.display())
    }

    /// The switches a finished run used, for the plugin.
    pub(crate) fn analysis_line(switches: AnalysisSwitches) -> String {
        format!("arc analysis {}\n", switches_data(switches))
    }

    /// A failed run, for the plugin.
    pub(crate) fn analysis_error_line(err: &anyhow::Error) -> String {
        format!("arc analysis-error {}\n", error_text(err))
    }

    /// The `analysis` event as one complete SSE block: a new page is ready.
    pub(crate) fn analysis_event(switches: AnalysisSwitches) -> String {
        format!("event: analysis\ndata: {}\n\n", switches_data(switches))
    }

    /// The `analysis-error` event as one complete SSE block.
    pub(crate) fn analysis_error_event(err: &anyhow::Error) -> String {
        format!("event: analysis-error\ndata: {}\n\n", error_text(err))
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

    /// The `on-save` event as one complete SSE block.
    pub(crate) fn on_save_event(on: bool) -> String {
        format!(
            "event: on-save\ndata: {}\n\n",
            if on { "on" } else { "off" }
        )
    }

    /// The `theme` event as one complete SSE block.
    pub(crate) fn theme_event(mode: Mode) -> String {
        format!("event: theme\ndata: {}\n\n", mode.as_str())
    }
}

/// `externals=on tests=off`: the same spelling on stdout and in the event.
fn switches_data(switches: AnalysisSwitches) -> String {
    let word = |on: bool| if on { "on" } else { "off" };
    format!(
        "externals={} tests={}",
        word(switches.externals),
        word(switches.tests)
    )
}

/// The error with its context chain on one line, since both channels are
/// line-based.
fn error_text(err: &anyhow::Error) -> String {
    format!("{err:#}").replace('\n', " ")
}

/// `path` relative to `root` where it lies inside, the absolute path
/// otherwise - the same convention `NodeData.file` uses on the arc page, so
/// a focus event's `file` matches what either page's `STATIC_DATA` carries.
fn workspace_relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A service whose pages never change: the recomputation fails. Only the
    /// arc page gets `svg`, the hotspots page an empty one.
    fn fixed(svg: &str, table: JumpTable, root: &str) -> JumpService<'static> {
        pinned(svg, table, root, None)
    }

    /// [`fixed`] with a theme pinned as by `--theme`.
    fn pinned(
        svg: &str,
        table: JumpTable,
        root: &str,
        theme: Option<&'static Theme>,
    ) -> JumpService<'static> {
        JumpService::new(
            Pages {
                arc: Diagram {
                    svg: svg.to_string(),
                },
                hotspots: Diagram { svg: String::new() },
                table,
            },
            AnalysisSwitches::default(),
            PathBuf::from(root),
            theme,
            Box::new(|_| anyhow::bail!("this service does not recompute")),
        )
    }

    /// Two crates' manifests (`my_crate` at id 0, `other` at id 1) and one
    /// workspace-relative source location (id 2), the way `build_layout`
    /// would have assigned them.
    fn service_over_root(root: &str) -> JumpService<'static> {
        let mut table = JumpTable::new();
        table.insert(PathBuf::from("/ws/my_crate/Cargo.toml"), 1);
        table.insert(PathBuf::from("/ws/other/Cargo.toml"), 1);
        table.insert(PathBuf::from("src/lib.rs"), 3);
        fixed("", table, root)
    }

    /// A recomputation that renders the switches into both pages' SVGs and
    /// registers one location (id 0) whose line is the number of runs so far.
    fn counting_recompute() -> (Recompute<'static>, std::sync::Arc<AtomicUsize>) {
        let runs = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = runs.clone();
        let recompute = Box::new(move |switches: AnalysisSwitches| {
            let run = counter.fetch_add(1, Ordering::SeqCst) + 1;
            let mut table = JumpTable::new();
            table.insert(PathBuf::from("src/lib.rs"), run);
            Ok(Pages {
                arc: Diagram {
                    svg: format!("<svg>{switches:?}</svg>"),
                },
                hotspots: Diagram {
                    svg: format!("<svg>hotspots {switches:?}</svg>"),
                },
                table,
            })
        });
        (recompute, runs)
    }

    #[test]
    fn recompute_swaps_both_pages_the_table_and_switches() {
        let (recompute, runs) = counting_recompute();
        let service = JumpService::new(
            Pages {
                arc: Diagram {
                    svg: "<svg>old</svg>".to_string(),
                },
                hotspots: Diagram {
                    svg: "<svg>old hotspots</svg>".to_string(),
                },
                table: JumpTable::new(),
            },
            AnalysisSwitches::default(),
            PathBuf::from("/ws"),
            None,
            recompute,
        );
        assert_eq!(service.jump(0), None);

        let wanted = AnalysisSwitches {
            externals: true,
            tests: false,
        };
        service.recompute(wanted).unwrap();

        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(service.switches(), wanted);
        assert_eq!(
            service.page(Page::Arc),
            html_page(
                &format!("<svg>{wanted:?}</svg>"),
                Some("ws"),
                Appearance::default()
            )
        );
        assert_eq!(
            service.page(Page::Hotspots),
            html_page(
                &format!("<svg>hotspots {wanted:?}</svg>"),
                Some("ws"),
                Appearance::default()
            )
        );
        assert_eq!(
            service.jump(0),
            Some(Location {
                file: PathBuf::from("/ws/src/lib.rs"),
                line: 1,
            })
        );
    }

    #[test]
    fn a_failed_recompute_keeps_the_page_and_the_switches() {
        let service = fixed("<svg>old</svg>", JumpTable::new(), "/ws");
        let wanted = AnalysisSwitches {
            externals: false,
            tests: true,
        };
        let err = service.recompute(wanted).unwrap_err();
        assert_eq!(err.to_string(), "this service does not recompute");
        assert_eq!(service.switches(), AnalysisSwitches::default());
        assert_eq!(
            service.page(Page::Arc),
            html_page("<svg>old</svg>", Some("ws"), Appearance::default())
        );
    }

    #[test]
    fn switch_set_changes_only_its_own_flag() {
        let both = AnalysisSwitches {
            externals: true,
            tests: true,
        };
        assert_eq!(
            Switch::Externals.set(both, false),
            AnalysisSwitches {
                externals: false,
                tests: true,
            }
        );
        assert_eq!(
            Switch::Tests.set(AnalysisSwitches::default(), true),
            AnalysisSwitches {
                externals: false,
                tests: true,
            }
        );
    }

    #[test]
    fn analysis_line_and_event_spell_both_switches() {
        let switches = AnalysisSwitches {
            externals: true,
            tests: false,
        };
        assert_eq!(
            JumpService::analysis_line(switches),
            "arc analysis externals=on tests=off\n"
        );
        assert_eq!(
            JumpService::analysis_event(switches),
            "event: analysis\ndata: externals=on tests=off\n\n"
        );
    }

    /// The error arrives with its context chain and may span lines; both
    /// channels carry one line.
    #[test]
    fn analysis_error_line_and_event_flatten_the_error_to_one_line() {
        let err = anyhow::anyhow!("no such\nmanifest").context("failed to load the workspace");
        assert_eq!(
            JumpService::analysis_error_line(&err),
            "arc analysis-error failed to load the workspace: no such manifest\n"
        );
        assert_eq!(
            JumpService::analysis_error_event(&err),
            "event: analysis-error\ndata: failed to load the workspace: no such manifest\n\n"
        );
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

    /// A hotspot map leaf's own jump target (file, line 1), registered in
    /// `node_files` under its workspace-relative path the way the hotspots
    /// page's own tree registers its leaves into the shared table: the
    /// service resolves it exactly like an arc target.
    #[test]
    fn jump_resolves_a_hotspot_leaf_registered_in_the_shared_table() {
        let mut table = JumpTable::new();
        let leaf = table.insert(PathBuf::from("/ws/app/src/hot.rs"), 1);
        table.insert_node_files("app/src/hot.rs", [leaf]);
        let service = fixed("", table, "/ws");
        assert_eq!(
            service.jump(0),
            Some(Location {
                file: PathBuf::from("/ws/app/src/hot.rs"),
                line: 1,
            })
        );
    }

    #[test]
    fn page_is_the_html_page_of_the_svg() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
        let service = fixed(svg, JumpTable::new(), "/ws");
        assert_eq!(
            service.page(Page::Arc),
            html_page(svg, Some("ws"), Appearance::default())
        );
    }

    /// `page` selects between the two pages one analysis run produced.
    #[test]
    fn page_selects_between_the_arc_diagram_and_the_hotspot_map() {
        let service = JumpService::new(
            Pages {
                arc: Diagram {
                    svg: "<svg>arc</svg>".to_string(),
                },
                hotspots: Diagram {
                    svg: "<svg>hotspots</svg>".to_string(),
                },
                table: JumpTable::new(),
            },
            AnalysisSwitches::default(),
            PathBuf::from("/ws"),
            None,
            Box::new(|_| anyhow::bail!("this service does not recompute")),
        );
        assert_eq!(
            service.page(Page::Arc),
            html_page("<svg>arc</svg>", Some("ws"), Appearance::default())
        );
        assert_eq!(
            service.page(Page::Hotspots),
            html_page("<svg>hotspots</svg>", Some("ws"), Appearance::default())
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
    fn focus_service() -> JumpService<'static> {
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
        fixed("", table, "/ws")
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
                file: "my_crate/Cargo.toml".to_string(),
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
                file: "src/mod_a.rs".to_string(),
                jumps: vec![LocationId::from(2), LocationId::from(3)],
            })
        );
    }

    /// A workspace member outside the root (`members = ["../member"]`) is
    /// registered absolute and cannot be spelled relative to the root; its
    /// `file` falls back to the absolute path, the same as `NodeData.file`
    /// would for such a member.
    #[test]
    fn focus_resolves_an_absolute_path_outside_the_root() {
        let service = focus_service();
        assert_eq!(
            service.focus(1, Path::new("/elsewhere/member/Cargo.toml")),
            Some(FocusEvent {
                node: "2".to_string(),
                file: "/elsewhere/member/Cargo.toml".to_string(),
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
                file: "src/mod_a.rs".to_string(),
                jumps: vec![],
            })
        );
    }

    #[test]
    fn parse_command_reads_focus_lines_with_the_path_last() {
        assert_eq!(
            parse_command("arc focus 12 /tmp/a b/lib.rs"),
            Some(Command::Focus {
                line: 12,
                file: PathBuf::from("/tmp/a b/lib.rs"),
            })
        );
    }

    #[test]
    fn parse_command_reads_follow_lines() {
        assert_eq!(
            parse_command("arc follow on"),
            Some(Command::Follow(FollowState::On))
        );
        assert_eq!(
            parse_command("arc follow off"),
            Some(Command::Follow(FollowState::Off))
        );
    }

    #[test]
    fn parse_command_reads_theme_lines() {
        assert_eq!(
            parse_command("arc theme light"),
            Some(Command::Theme(Mode::Light))
        );
        assert_eq!(
            parse_command("arc theme dark"),
            Some(Command::Theme(Mode::Dark))
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
        assert!(!service.page(Page::Arc).contains("data-mode"));
        service.set_editor_mode(Mode::Dark);
        assert!(
            service
                .page(Page::Arc)
                .contains("<html xmlns=\"http://www.w3.org/1999/xhtml\" data-mode=\"dark\">"),
            "{}",
            service.page(Page::Arc)
        );
    }

    /// A theme pinned by --theme keeps its name on the root only while the
    /// editor's mode agrees with it: the editor decides the mode.
    #[test]
    fn editor_mode_drops_a_pinned_theme_of_the_other_mode() {
        let mut table = JumpTable::new();
        table.insert(PathBuf::from("src/lib.rs"), 3);
        let service = pinned("", table, "/ws", Theme::named("mocha"));
        assert!(service.page(Page::Arc).contains("data-theme=\"mocha\""));
        service.set_editor_mode(Mode::Dark);
        assert!(service.page(Page::Arc).contains("data-theme=\"mocha\""));
        service.set_editor_mode(Mode::Light);
        assert!(
            !service.page(Page::Arc).contains("data-theme"),
            "{}",
            service.page(Page::Arc)
        );
    }

    #[test]
    fn parse_command_reads_the_analysis_switch_lines() {
        assert_eq!(
            parse_command("arc externals on"),
            Some(Command::Switch {
                switch: Switch::Externals,
                on: true,
            })
        );
        assert_eq!(
            parse_command("arc externals off"),
            Some(Command::Switch {
                switch: Switch::Externals,
                on: false,
            })
        );
        assert_eq!(
            parse_command("arc tests on"),
            Some(Command::Switch {
                switch: Switch::Tests,
                on: true,
            })
        );
        assert_eq!(
            parse_command("arc tests off"),
            Some(Command::Switch {
                switch: Switch::Tests,
                on: false,
            })
        );
    }

    #[test]
    fn reads_a_save_of_a_source_file_or_manifest_under_the_root() {
        let service = service_over_root("/ws");
        assert!(service.reads(Path::new("/ws/src/lib.rs")));
        assert!(service.reads(Path::new("/ws/a/src/deep/mod.rs")));
        assert!(service.reads(Path::new("/ws/Cargo.toml")));
        assert!(service.reads(Path::new("/ws/a/Cargo.toml")));
        assert!(!service.reads(Path::new("/ws/README.md")));
        assert!(!service.reads(Path::new("/ws/arc-rules.toml")));
        assert!(!service.reads(Path::new("/ws/Cargo.lock")));
        assert!(!service.reads(Path::new("/elsewhere/src/lib.rs")));
    }

    #[test]
    fn on_save_starts_on_and_its_event_spells_the_state() {
        let service = service_over_root("/ws");
        assert!(service.on_save());
        service.set_on_save(false);
        assert!(!service.on_save());
        assert_eq!(
            JumpService::on_save_event(false),
            "event: on-save\ndata: off\n\n"
        );
        assert_eq!(
            JumpService::on_save_event(true),
            "event: on-save\ndata: on\n\n"
        );
    }

    #[test]
    fn parse_command_reads_saved_lines_with_the_whole_rest_as_path() {
        assert_eq!(
            parse_command("arc saved /tmp/a b/lib.rs"),
            Some(Command::Saved(PathBuf::from("/tmp/a b/lib.rs")))
        );
    }

    #[test]
    fn parse_command_reads_the_recompute_and_on_save_lines() {
        assert_eq!(parse_command("arc recompute"), Some(Command::Recompute));
        assert_eq!(parse_command("arc on-save on"), Some(Command::OnSave(true)));
        assert_eq!(
            parse_command("arc on-save off"),
            Some(Command::OnSave(false))
        );
    }

    /// The line is a protocol: spelling is exact, nothing is trimmed.
    #[test]
    fn parse_command_ignores_malformed_and_foreign_lines() {
        assert_eq!(parse_command("arc focus x /p"), None);
        assert_eq!(parse_command("arc focus 3"), None);
        assert_eq!(parse_command("arc follow maybe"), None);
        assert_eq!(parse_command("arc theme blue"), None);
        assert_eq!(parse_command("arc externals ON"), None);
        assert_eq!(parse_command("arc Tests on"), None);
        assert_eq!(parse_command("arc externals"), None);
        assert_eq!(parse_command("arc externals on "), None);
        assert_eq!(parse_command("arc jump 3 /p"), None);
        assert_eq!(parse_command("arc saved "), None);
        assert_eq!(parse_command("arc recompute now"), None);
        assert_eq!(parse_command("arc on-save"), None);
        assert_eq!(parse_command("hello"), None);
        assert_eq!(parse_command(""), None);
    }

    #[test]
    fn focus_event_formats_as_an_sse_block() {
        let event = FocusEvent {
            node: "7".to_string(),
            file: "src/lib.rs".to_string(),
            jumps: vec![LocationId::from(2), LocationId::from(3)],
        };
        assert_eq!(
            JumpService::focus_event(&event),
            "event: focus\ndata: {\"node\":\"7\",\"file\":\"src/lib.rs\",\"jumps\":[2,3]}\n\n"
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
