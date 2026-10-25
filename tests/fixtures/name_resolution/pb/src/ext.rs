mod kid;

use core::fmt::Write;

pub(crate) fn core() {}

pub(crate) fn run() {
    core();
    kid::k(&mut String::new());
}
