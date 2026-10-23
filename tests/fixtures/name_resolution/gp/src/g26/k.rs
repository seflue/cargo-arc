#[allow(unused_imports)]
use crate::e::*;
#[cfg(not(test))]
pub fn g() -> u8 {
    use super::*;
    f()
}
