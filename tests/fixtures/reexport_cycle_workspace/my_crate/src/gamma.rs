// Behavioral dependency on delta: gamma <-> delta form a real logic cycle.
use crate::delta::base;

pub fn compute() -> i32 {
    base() + 1
}

pub fn seed() -> i32 {
    7
}
