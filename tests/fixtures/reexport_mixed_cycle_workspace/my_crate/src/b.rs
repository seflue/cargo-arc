// Mixed edge b -> a: a pure re-export (AThing) alongside a real, behavioral
// import (value). Only `value` should count as a symbol crossing the edge.
pub use crate::a::AThing;
use crate::a::value;

pub fn compute() -> i32 {
    value() + 1
}
