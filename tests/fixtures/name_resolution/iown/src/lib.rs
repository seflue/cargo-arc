mod seg_impl;

use seg_impl as segs;

pub fn run() -> bool {
    segs::seg_only() && inner::y()
}

mod inner {
    use super::*;

    mod segs {
        pub fn own_only() -> bool {
            true
        }
    }

    pub(super) fn y() -> bool {
        segs::own_only()
    }
}
