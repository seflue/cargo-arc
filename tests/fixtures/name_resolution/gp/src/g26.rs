#[cfg(not(test))]
use crate::d::f;
#[cfg(test)]
use crate::c::f;
pub fn here() {
    let _ = f();
}
pub mod k;
