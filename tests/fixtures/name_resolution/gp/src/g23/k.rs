#[allow(unused_imports)]
use crate::d::*;
#[cfg(test)]
pub fn t() -> u8 {
    #[allow(unused_imports)]
    use super::*;
    f()
}
