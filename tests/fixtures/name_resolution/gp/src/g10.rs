#[cfg(unix)]
use crate::c::f;
#[cfg(not(unix))]
use crate::d::f;
pub mod k;
