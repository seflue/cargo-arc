pub trait Double {
    fn double(self) -> u8;
}

impl Double for u8 {
    fn double(self) -> u8 {
        self * 2
    }
}

pub struct Marker;
