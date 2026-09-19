//! The `use` in `stats` binds `shared` for that body alone. In `draw` the name
//! still means the module the file-level `use` binds, and that dependency
//! closes a cycle with it.

use crate::shared;

pub struct Marker;

pub fn draw() {
    shared::submit();
}

pub fn stats() -> usize {
    use crate::metrics::shared;

    shared::count()
}
