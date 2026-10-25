use super::*;

pub(crate) fn x() -> bool {
    inner::y()
}

mod inner {
    use super::*;

    pub(super) fn y() -> bool {
        segs::is_start()
    }
}
