use crate::b::compute;

pub struct AThing;

/// Not re-exported from `b` at first; a test adds `pub use crate::a::AnotherThing;`
/// to `b.rs` to check that a re-exported name added later stays free.
pub struct AnotherThing;

pub fn value() -> i32 {
    compute()
}
