#[cfg(not(test))]
use crate::d::f;
#[cfg(test)]
use crate::c::f;
#[allow(unused_imports)]
use crate::e::*;
pub fn here() {
    let _ = f();
}
pub mod k;
