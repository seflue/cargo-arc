mod ranked;
#[cfg(feature = "obs")]
mod observing;
mod vanilla;

#[cfg(validate_locks)]
use ranked as chosen;
#[cfg(feature = "obs")]
use observing as chosen;
#[cfg(not(any(validate_locks, feature = "obs")))]
use vanilla as chosen;

pub use chosen::Mutex;
