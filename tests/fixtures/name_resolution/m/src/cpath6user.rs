#[cfg(unix)]
use crate::cpath6::imp::*;
use crate::cpath6other::*;

#[cfg(unix)]
pub fn go() {
    run();
    keep();
}
