//! JS module registry: build-time discovery and codegen (`parse.rs`, shared
//! textually with `build.rs`) plus the runtime bundle over the generated
//! `MODULES` table.

// Build-time-only: discovery, @deps validation and MODULES codegen, shared
// textually with build.rs. Nothing at runtime needs it, only its own tests.
#[cfg(test)]
include!("parse.rs");

mod bundle;
mod table;

pub(crate) use bundle::bundle;
