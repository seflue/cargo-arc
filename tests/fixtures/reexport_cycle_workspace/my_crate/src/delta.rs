// Behavioral dependency on gamma: closes the real logic cycle.
use crate::gamma::seed;

pub fn base() -> i32 {
    seed() + 2
}
