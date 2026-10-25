mod seg_impl;

pub fn run() -> u8 {
    #[allow(unused_imports)]
    use seg_impl as segs;
    mod m {
        use super::*;

        pub fn y() -> u8 {
            segs::other();
            segs::is_start()
        }
    }
    m::y()
}
