macro_rules! make {
    () => {
        mod segs {
            pub fn is_start() -> bool {
                true
            }
        }
    };
}

make!();

pub fn x() -> bool {
    segs::is_start()
}

pub fn y() {
    alias::other()
}
