mod seg_impl;

use seg_impl as segs;

pub fn run() -> bool {
    inner::y();
    segs::is_start()
}

mod inner {
    pub(super) fn y() {
        segs::other()
    }
}
