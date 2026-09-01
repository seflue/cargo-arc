//! The `use` in `stats` binds `shared` for that body alone. In `draw` the bare
//! name still means the module beside this file, and that dependency closes a
//! cycle with it.

pub struct Marker;

pub fn draw() {
    shared::submit();
}

pub fn stats() -> usize {
    use crate::metrics::shared;

    shared::count()
}
