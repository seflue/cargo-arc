mod segs;

pub fn run() -> bool {
    segs::is_start() && inner::y() == 0
}

mod inner {
    pub(super) fn y() -> u8 {
        segs::is_start()
    }
}
