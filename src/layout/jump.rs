//! Jump target ids: assigned by the layout, read by render, resolved by ui.

use crate::model::SourceLocation;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Identifies one entry in a [`JumpTable`]. Serialized as a plain number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct LocationId(usize);

impl From<usize> for LocationId {
    /// Converts a query parameter into the id it names, without making the
    /// field itself public: `resolve` still rejects an id past the end.
    fn from(id: usize) -> Self {
        Self(id)
    }
}

/// Which file a jump target resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TargetKind {
    Lib,
    Bin,
    Manifest,
    Module,
}

/// A file and a line a jump target resolves to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Location {
    pub file: PathBuf,
    pub line: usize,
}

/// One jump target on a layout item: its kind, the path shown in the UI
/// (workspace-relative where the target lies inside the workspace), and the
/// id it resolves through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JumpTarget {
    pub kind: TargetKind,
    pub name: String,
    pub id: LocationId,
}

/// One of an edge's source locations, paired with the id it resolves through.
#[derive(Debug, Clone)]
pub(crate) struct LocatedSource {
    pub location: SourceLocation,
    pub id: LocationId,
}

/// A symbol's definition site in its provider's file, paired with the id it
/// resolves through. The file is the provider's own module target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocatedDefinition {
    pub line: usize,
    pub id: LocationId,
}

/// Assigns `LocationId`s to jump targets as a running counter, and remembers
/// each target as a [`Location`]. Also keeps the reverse direction for the
/// editor: which node a file belongs to, keyed the way `STATIC_DATA` keys its
/// `nodes`.
#[derive(Debug, Default)]
pub(crate) struct JumpTable {
    entries: Vec<Location>,
    node_files: HashMap<PathBuf, String>,
}

impl JumpTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Insert a jump target and return its newly assigned id.
    pub(crate) fn insert(&mut self, file: PathBuf, line: usize) -> LocationId {
        let id = LocationId(self.entries.len());
        self.entries.push(Location { file, line });
        id
    }

    /// Look up a previously inserted target's location.
    pub(crate) fn resolve(&self, id: LocationId) -> Option<&Location> {
        self.entries.get(id.0)
    }

    /// Record that the files behind `ids` belong to the node keyed `node`, so
    /// a file the editor shows resolves to its node. An id past the end
    /// registers nothing.
    pub(crate) fn insert_node_files(
        &mut self,
        node: &str,
        ids: impl IntoIterator<Item = LocationId>,
    ) {
        for id in ids {
            if let Some(location) = self.entries.get(id.0) {
                self.node_files
                    .insert(location.file.clone(), node.to_string());
            }
        }
    }

    /// The key of the node `file` belongs to, as registered.
    pub(crate) fn node_at(&self, file: &Path) -> Option<&str> {
        self.node_files.get(file).map(String::as_str)
    }

    /// Every id whose target is `line` of `file`, in insertion order.
    pub(crate) fn ids_at(&self, file: &Path, line: usize) -> Vec<LocationId> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, location)| location.line == line && location.file == file)
            .map(|(index, _)| LocationId(index))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn table_assigns_sequential_ids_and_resolves_them() {
        let mut table = JumpTable::new();
        let a = table.insert(PathBuf::from("src/lib.rs"), 1);
        let b = table.insert(PathBuf::from("src/main.rs"), 1);

        assert_eq!(a, LocationId(0));
        assert_eq!(b, LocationId(1));
        assert_eq!(
            table.resolve(a),
            Some(&Location {
                file: PathBuf::from("src/lib.rs"),
                line: 1
            })
        );
        assert_eq!(
            table.resolve(b),
            Some(&Location {
                file: PathBuf::from("src/main.rs"),
                line: 1
            })
        );
        assert_eq!(table.resolve(LocationId(2)), None);
    }

    #[test]
    fn location_id_converts_from_the_query_number_a_service_receives() {
        let mut table = JumpTable::new();
        table.insert(PathBuf::from("src/lib.rs"), 1);
        let b = table.insert(PathBuf::from("src/main.rs"), 1);

        assert_eq!(table.resolve(LocationId::from(1)), table.resolve(b));
        assert_eq!(table.resolve(LocationId::from(usize::MAX)), None);
    }

    #[test]
    fn node_at_finds_the_node_whose_targets_a_file_was_registered_under() {
        let mut table = JumpTable::new();
        let lib = table.insert(PathBuf::from("/ws/app/src/lib.rs"), 1);
        let manifest = table.insert(PathBuf::from("/ws/app/Cargo.toml"), 1);
        let module = table.insert(PathBuf::from("src/mod_a.rs"), 1);
        table.insert_node_files("0", [lib, manifest]);
        table.insert_node_files("1", [module, LocationId(9)]);

        assert_eq!(table.node_at(Path::new("/ws/app/src/lib.rs")), Some("0"));
        assert_eq!(table.node_at(Path::new("/ws/app/Cargo.toml")), Some("0"));
        assert_eq!(table.node_at(Path::new("src/mod_a.rs")), Some("1"));
        assert_eq!(table.node_at(Path::new("src/other.rs")), None);
    }

    #[test]
    fn ids_at_lists_every_entry_on_a_line_in_insertion_order() {
        let mut table = JumpTable::new();
        let first = table.insert(PathBuf::from("src/lib.rs"), 3);
        table.insert(PathBuf::from("src/lib.rs"), 4);
        let third = table.insert(PathBuf::from("src/lib.rs"), 3);
        table.insert(PathBuf::from("src/main.rs"), 3);

        assert_eq!(table.ids_at(Path::new("src/lib.rs"), 3), vec![first, third]);
        assert_eq!(
            table.ids_at(Path::new("src/lib.rs"), 9),
            Vec::<LocationId>::new()
        );
        assert_eq!(
            table.ids_at(Path::new("src/nope.rs"), 3),
            Vec::<LocationId>::new()
        );
    }
}
