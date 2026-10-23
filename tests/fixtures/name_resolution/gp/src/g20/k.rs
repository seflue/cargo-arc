#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::d::*;
#[cfg(not(unix))]
pub fn g() -> u8 { f() }
