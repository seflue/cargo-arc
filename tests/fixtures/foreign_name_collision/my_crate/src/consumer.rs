//! Imports two names from outside this crate. Each one also names a module
//! beside this file, and neither reference below means that module.

// A workspace crate the analysis can place. The last path segment collides.
use remote_lib::shared;
// The same crate again, renamed. Here the alias collides instead.
use remote_lib as helpers;
// A crate the analysis has no metadata for, which is what `arc check` sees for
// every external dependency.
use unplaced_lib::tools;

pub struct Marker;

pub fn from_remote(_: shared::Item) {}

pub fn from_renamed(_: helpers::shared::Item) {}

pub fn from_unplaced(_: tools::Item) {}
