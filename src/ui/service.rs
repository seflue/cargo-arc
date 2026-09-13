//! The jump service's HTTP-independent logic: the page, the table, and the
//! two lines it writes to stdout.

use std::path::PathBuf;

use crate::layout::{JumpTable, Location, LocationId};
use crate::render::html_page;

/// Serves the diagram page and resolves jump ids against the workspace root
/// that produced them.
pub(crate) struct JumpService {
    page: String,
    table: JumpTable,
    root: PathBuf,
}

impl JumpService {
    /// `svg` is the rendered diagram as `cargo arc -o` would write it.
    pub(crate) fn new(svg: &str, table: JumpTable, root: PathBuf) -> Self {
        Self {
            page: html_page(svg),
            table,
            root,
        }
    }

    /// The diagram as the page from [`html_page`]: a webview shrinks a bare
    /// SVG document to its frame, an inline one keeps its size.
    pub(crate) fn page(&self) -> &str {
        &self.page
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

    pub(crate) fn ready_line(port: u16) -> String {
        format!("arc ready {} {port}\n", env!("CARGO_PKG_VERSION"))
    }

    pub(crate) fn jump_line(location: &Location) -> String {
        format!("arc jump {} {}\n", location.line, location.file.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two crates' manifests (`my_crate` at id 0, `other` at id 1) and one
    /// workspace-relative source location (id 2), the way `build_layout`
    /// would have assigned them.
    fn service_over_root(root: &str) -> JumpService {
        let mut table = JumpTable::new();
        table.insert(PathBuf::from("/ws/my_crate/Cargo.toml"), 1);
        table.insert(PathBuf::from("/ws/other/Cargo.toml"), 1);
        table.insert(PathBuf::from("src/lib.rs"), 3);
        JumpService::new("", table, PathBuf::from(root))
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
        let service = JumpService::new(svg, JumpTable::new(), PathBuf::from("/ws"));
        assert_eq!(service.page(), html_page(svg));
    }

    #[test]
    fn ready_line_names_the_crate_version_and_port() {
        assert_eq!(
            JumpService::ready_line(4321),
            format!("arc ready {} 4321\n", env!("CARGO_PKG_VERSION"))
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
