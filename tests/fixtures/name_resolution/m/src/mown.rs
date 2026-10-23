macro_rules! mm {
    () => {
        0
    };
}
use mm;

mod kid;

pub fn g() -> i32 {
    kid::f()
}
