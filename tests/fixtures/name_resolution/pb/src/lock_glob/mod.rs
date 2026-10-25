#[cfg(feature = "obs")]
mod observing;
mod vanilla;

#[cfg(feature = "obs")]
use observing as chosen;
#[cfg(not(feature = "obs"))]
use vanilla as chosen;

pub use chosen::*;
