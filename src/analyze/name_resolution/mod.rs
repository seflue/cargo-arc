//! Name resolution: the item each path in the workspace's source names.

mod collect;
mod context;
mod dependencies;
mod lookup;
mod package_resolver;
mod path;
mod table;
mod workspace;

#[cfg(test)]
mod tests;

pub(crate) use package_resolver::{FileInModule, PackageResolver, Root};
pub(crate) use workspace::NameResolution;
