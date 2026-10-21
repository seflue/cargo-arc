pub trait Triple {
    fn triple(self) -> u8;
}

impl Triple for u8 {
    fn triple(self) -> u8 {
        self * 3
    }
}
