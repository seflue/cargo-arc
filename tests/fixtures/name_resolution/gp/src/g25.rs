#[cfg(test)]
use crate::c::f;
#[cfg(not(test))]
use crate::d::f;
#[allow(unused_imports)]
use crate::e::*;
pub fn here() {
    let _ = f();
}
pub mod k;
