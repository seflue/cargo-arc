//! Jump target ids: assigned by the layout, read by render, resolved by ui.

use crate::model::SourceLocation;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

/// One jump target on a layout item: its kind and the id it resolves through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JumpTarget {
    pub kind: TargetKind,
    pub id: LocationId,
}

/// One of an edge's source locations, paired with the id it resolves through.
#[derive(Debug, Clone)]
pub(crate) struct LocatedSource {
    pub location: SourceLocation,
    pub id: LocationId,
}

/// Assigns `LocationId`s to jump targets as a running counter, and remembers
/// each target as a [`Location`].
#[derive(Debug, Default)]
pub(crate) struct JumpTable {
    entries: Vec<Location>,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
}
