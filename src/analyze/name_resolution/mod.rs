//! Name resolution: the item each path in the workspace's source names.

mod package_resolver;
mod workspace;

#[cfg(test)]
mod tests;

pub(crate) use package_resolver::{FileInModule, PackageResolver, Root};
pub(crate) use workspace::{NameResolution, collect_crate_exports};
