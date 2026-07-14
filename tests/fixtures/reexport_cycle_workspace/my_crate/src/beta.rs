pub struct BetaThing;

// Pure re-export: republishes alpha's type, no behavioral use of alpha.
// alpha <-> beta thus form a re-export-only cycle.
pub use crate::alpha::AlphaThing;
