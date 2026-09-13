//! The jump service's HTTP-independent logic: the page, the table, and the
//! two lines it writes to stdout.

use std::path::PathBuf;

use crate::layout::{JumpTable, Location, LocationId};

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
            page: inline_in_xhtml(svg),
            table,
            root,
        }
    }

    /// The diagram as an XHTML document with the SVG inline. An SVG document
    /// shown in a frame (an editor's webview) is sized to the frame and its
    /// content scaled to fit; an inline `<svg>` keeps its pixel size and the
    /// body scrolls.
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

/// Moves the XML declaration of `svg` ahead of the wrapping document: it
/// must stay first, and the rest of the SVG is already well-formed XML. The
/// body is white because the SVG has no background of its own and a webview
/// would otherwise show the editor theme through it.
fn inline_in_xhtml(svg: &str) -> String {
    let (declaration, svg) = match svg.split_once('\n') {
        Some((first, rest)) if first.starts_with("<?xml") => (first, rest),
        _ => ("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", svg),
    };
    format!(
        "{declaration}\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>cargo-arc</title></head><body style=\"margin:0;background:#fff\">\n\
         {svg}\n\
         </body></html>\n"
    )
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
    fn page_inlines_the_svg_in_an_xhtml_document() {
        let svg = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"20\"/>";
        let service = JumpService::new(svg, JumpTable::new(), PathBuf::from("/ws"));
        assert_eq!(
            service.page(),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>cargo-arc</title></head><body style=\"margin:0;background:#fff\">\n\
             <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"20\"/>\n\
             </body></html>\n"
        );
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
