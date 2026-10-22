macro_rules! min {
    () => {
        0
    };
}
use std::cmp::min;

mod kid;

pub fn g() -> i32 {
    min!() + min(1, 2) + kid::f()
}
