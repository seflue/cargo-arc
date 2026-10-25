#[allow(non_camel_case_types)]
pub enum segs {
    is_start,
}

use segs::is_start;

pub fn f() -> segs {
    is_start
}
