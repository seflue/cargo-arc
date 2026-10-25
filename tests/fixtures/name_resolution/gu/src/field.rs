use pre::prelude::*;

pub struct Holder {
    pub segs: u8,
}

pub fn read(holder: &Holder) -> u8 {
    assert!(holder.segs == 0);
    holder.segs
}
