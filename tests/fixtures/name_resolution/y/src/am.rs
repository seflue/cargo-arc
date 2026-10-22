#[allow(unused_imports)]
use crate::r::*;

macro_rules! mk {
    ($n:ident) => {
        pub struct $n {
            pub v: u8,
        }
    };
}

mk!(Thing2);
