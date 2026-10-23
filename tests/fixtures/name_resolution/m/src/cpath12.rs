#[cfg_attr(test, path = "cpath12_test.rs", cfg_attr(unix, path = "cpath12_second.rs"))]
pub mod imp;
