mod reader;
mod traits;

use traits::*;

pub fn run(v: u8) -> u8 {
    v.double()
}
