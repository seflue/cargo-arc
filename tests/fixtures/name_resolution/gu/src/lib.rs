pub mod field;
pub mod shadow;
pub mod through;

use pre::prelude::*;
use segs::other;

pub fn f() -> u8 {
    other();
    segs::is_start()
}
