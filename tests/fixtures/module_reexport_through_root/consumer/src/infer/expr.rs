use crate::Mode;

pub fn pick(mode: Mode) -> u8 {
    match mode {
        Mode::Shared => 0,
        Mode::Owned => 1,
    }
}
