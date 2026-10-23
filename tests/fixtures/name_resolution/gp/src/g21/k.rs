#[allow(unused_imports)]
use crate::d::*;
#[cfg(unix)]
pub fn g() -> u16 { use super::*; f() }
#[cfg(not(unix))]
pub fn g() -> u8 { use super::*; f() }
