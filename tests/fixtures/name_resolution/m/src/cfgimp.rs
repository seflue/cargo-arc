#[cfg(unix)]
#[path = "cfgimp_unix.rs"]
pub mod imp;
#[cfg(not(unix))]
#[path = "cfgimp_other.rs"]
pub mod imp;
