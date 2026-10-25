pub struct S;

pub mod inl {
    use super::S;

    pub struct T;

    pub fn g(_: S) {}
}

use self::inl::T;

pub fn h(_: T) {}
