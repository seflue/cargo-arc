extern crate self as xs;

pub mod a;
pub mod b;
pub mod c;
pub mod d;
pub mod e;
#[cfg_attr(test, path = "fake_test.rs")]
pub mod fake;
pub mod guser;
pub mod pc;
#[cfg(test)]
pub mod tfake;
pub mod user;
