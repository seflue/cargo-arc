#[cfg(not(unix))]
use crate::cpath3::imp::*;

#[cfg(not(unix))]
pub fn go() {
    job();
    run();
    k::f();
}
