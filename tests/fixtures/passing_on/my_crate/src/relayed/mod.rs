mod front;
mod reader2;

use front::*;

pub fn run(v: u8) -> u8 {
    v.triple()
}
