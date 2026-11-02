//! The text of a path and where it starts: joining and splitting segments,
//! the paths of a `use` tree, relative paths, the extern prelude of a root,
//! and the place a `use` or path starts from.

use super::super::packages::ModulePathMap;
use super::bindings::ReExportTarget;
use crate::model::{EntryFile, EntryKind, PackageInfo};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use syn::UseTree;

/// What a path does after the name it asks of a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum AfterName {
    /// The name is the last segment of the path.
    PathEnds,
    /// More segments follow, which the name has to carry on.
    PathGoesOn,
}

impl AfterName {
    /// What the path does after a name `rest` follows.
    pub(super) fn followed_by(rest: &[&str]) -> Self {
        if rest.is_empty() {
            Self::PathEnds
        } else {
            Self::PathGoesOn
        }
    }
}

/// Join a prefix and segment with `::`, handling empty prefix.
pub(super) fn append_to_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}::{segment}")
    }
}

/// `path` followed by `rest`, `path` alone where `rest` is empty.
pub(super) fn followed_by(path: &str, rest: &str) -> String {
    if rest.is_empty() {
        path.to_string()
    } else {
        format!("{path}::{rest}")
    }
}

/// Whether a use-tree leaf is the `self` that names its parent module.
/// A `self` without a prefix (`use self;`) does not name a module and is left alone.
fn is_self_segment(ident: &syn::Ident, prefix: &str) -> bool {
    ident == "self" && !prefix.is_empty()
}

/// Recursively resolve a `syn::UseTree` into fully-qualified path strings.
///
/// Example: `use cli::{Args, Cargo, run}` → `["cli::Args", "cli::Cargo", "cli::run"]`
///
/// When `use_alias` is true, renames return the alias (`as X` → `X`).
/// When false, renames return the original name (source dependency tracking).
///
/// A trailing `self` (`use foo::{self, ..}`) names the module `foo` itself, so it
/// resolves to the prefix rather than to an item called `self`. Under `use_alias`
/// that keeps the local binding name as the last segment (`foo`), which is what an
/// unrenamed `self` binds.
pub(super) fn resolve_use_tree(tree: &UseTree, prefix: &str, use_alias: bool) -> Vec<String> {
    match tree {
        UseTree::Path(p) => resolve_use_tree(
            &p.tree,
            &append_to_path(prefix, &p.ident.to_string()),
            use_alias,
        ),
        UseTree::Name(n) if is_self_segment(&n.ident, prefix) => vec![prefix.to_string()],
        UseTree::Name(n) => vec![append_to_path(prefix, &n.ident.to_string())],
        // `use foo::{self as bar}`: the alias side binds `bar`, the source side is `foo`.
        UseTree::Rename(r) if !use_alias && is_self_segment(&r.ident, prefix) => {
            vec![prefix.to_string()]
        }
        UseTree::Rename(r) => {
            let name = if use_alias { &r.rename } else { &r.ident };
            vec![append_to_path(prefix, &name.to_string())]
        }
        UseTree::Glob(_) => vec![append_to_path(prefix, "*")],
        UseTree::Group(g) => g
            .items
            .iter()
            .flat_map(|item| resolve_use_tree(item, prefix, use_alias))
            .collect(),
    }
}

/// Resolve `super::` and `self::` relative paths to absolute crate-local paths.
///
/// Returns `None` when the path is not relative, or when too many `super::`
/// would go above crate root.
pub(super) fn resolve_relative_path(path: &str, current_module_path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split("::").collect();
    let super_count = segments.iter().take_while(|&&s| s == "super").count();

    if super_count > 0 {
        return join_module_segments(current_module_path, super_count, &segments[super_count..]);
    }

    if segments.first() == Some(&"self") {
        return join_module_segments(current_module_path, 0, &segments[1..]);
    }

    None
}

/// Strip `levels_up` trailing segments from `base_path`, append `suffix`, join with `::`.
/// Returns `None` if `levels_up` exceeds the number of segments in `base_path`.
fn join_module_segments(base_path: &str, levels_up: usize, suffix: &[&str]) -> Option<String> {
    let mut base: Vec<&str> = base_path.split("::").filter(|s| !s.is_empty()).collect();
    if levels_up > base.len() {
        return None;
    }
    base.truncate(base.len() - levels_up);
    base.extend_from_slice(suffix);
    Some(base.join("::"))
}

/// The segments of the module path `path`, none for the crate root (`""`).
pub(super) fn module_segments(path: &str) -> impl Iterator<Item = &str> {
    path.split("::").filter(|segment| !segment.is_empty())
}

/// What the first name of a path stands for in the module the path is
/// written in, as far as the one who asks can tell.
pub(super) enum FirstName {
    /// A module: a child module, or one a binding of the module names.
    Module { crate_name: String, path: String },
    /// An item of the type namespace the module defines.
    TypeItem,
    /// Something only the binding table tells, asked while it is collected.
    Unknown,
    /// Nothing the module binds.
    Unbound,
}

/// Where a path starts, picked in rustc's order: `self::` and `super::` an
/// ancestor of the module, `crate::` the root, then what the first name
/// stands for in the module, then a crate of the extern prelude.
pub(super) enum PickedStart<'p> {
    /// The own crate, with the path from its root, `::`-joined.
    OwnRoot(String),
    /// A module the first name stands for, with the rest of the path, `""`
    /// where it ends on the name.
    Module {
        crate_name: String,
        path: String,
        rest: &'p str,
    },
    /// The item `name` of the module; the rest of the path names something
    /// in the item.
    TypeItem { name: &'p str },
    /// A name only the binding table tells, then the rest of the path.
    Unknown { rest: &'p str },
    /// The workspace crate the extern prelude names, with the rest.
    Crate { crate_name: String, rest: &'p str },
    /// None of these: an external crate, or nothing.
    Outside,
}

impl<'p> PickedStart<'p> {
    /// The start of `path`, written in `module_path`. `first_name` tells what
    /// the first name stands for in the module, and is asked only where the
    /// path starts with a name. An unknown first name gives way to a crate
    /// of `extern_prelude`: in a `use` path a name both bound and a crate is
    /// ambiguous to rustc, so the crate is what compiles. `None` where the
    /// path starts with an empty segment.
    pub(super) fn of(
        path: &'p str,
        module_path: &str,
        extern_prelude: &HashMap<String, String>,
        first_name: impl FnOnce(&str, AfterName) -> FirstName,
    ) -> Option<Self> {
        if let Some(resolved) = resolve_relative_path(path, module_path) {
            return Some(Self::OwnRoot(resolved));
        }
        if let Some(after_crate) = path.strip_prefix("crate::") {
            return Some(Self::OwnRoot(after_crate.to_string()));
        }
        let (first, rest) = path.split_once("::").unwrap_or((path, ""));
        if first.is_empty() {
            return None;
        }
        let in_prelude = || {
            extern_prelude.get(first).map(|crate_name| Self::Crate {
                crate_name: crate_name.clone(),
                rest,
            })
        };
        let after = if rest.is_empty() {
            AfterName::PathEnds
        } else {
            AfterName::PathGoesOn
        };
        Some(match first_name(first, after) {
            FirstName::Module { crate_name, path } => Self::Module {
                crate_name,
                path,
                rest,
            },
            FirstName::TypeItem => Self::TypeItem { name: first },
            FirstName::Unknown => in_prelude().unwrap_or(Self::Unknown { rest }),
            FirstName::Unbound => in_prelude().unwrap_or(Self::Outside),
        })
    }
}

/// The workspace crates in the extern prelude of one crate root: name in
/// the code -> key of the crate named, in the root's build without `test`
/// and in its test build. Only these names start a path at a workspace
/// crate.
#[derive(Debug, Clone, Default)]
pub(super) struct ExternPrelude {
    without_test: HashMap<String, String>,
    test: HashMap<String, String>,
}

impl ExternPrelude {
    /// The workspace crates in the extern prelude of the root `entry` of
    /// `package`, or of an integration test where `entry` is `None`, in each
    /// build. Each dependency stands under the name the manifest gives it
    /// (`code_names`), never under its key where that differs. A bin and an
    /// integration test also name the package's lib; the test build adds the
    /// dev-dependencies. `extern crate X as Y;` in the root adds `Y` for `X`,
    /// `extern crate self as Y;` adds `Y` for the package itself.
    pub(super) fn of_root(package: &PackageInfo, entry: Option<&EntryFile>) -> Self {
        let key = package.key.as_str();
        let names_of = |dependency: &str| -> Vec<(String, String)> {
            let renamed: Vec<_> = package
                .code_names
                .iter()
                .filter(|(_, named)| named.as_str() == dependency)
                .map(|(name, named)| (name.clone(), named.clone()))
                .collect();
            if renamed.is_empty() {
                vec![(dependency.to_string(), dependency.to_string())]
            } else {
                renamed
            }
        };
        let mut without_test: HashMap<String, String> = package
            .dependencies
            .iter()
            .flat_map(|dependency| names_of(dependency))
            .collect();
        if entry.is_none_or(|entry| entry.kind == EntryKind::Bin) {
            without_test.extend(names_of(key));
        }
        if let Some(entry) = entry {
            without_test.extend(extern_crate_aliases(&entry.file, key, &without_test));
        }
        let mut test = without_test.clone();
        test.extend(
            package
                .dev_dependencies
                .iter()
                .flat_map(|dependency| names_of(dependency)),
        );
        Self { without_test, test }
    }

    /// The names of the test build where `test` is set, otherwise of the
    /// build without `test`.
    pub(super) fn names(&self, test: bool) -> &HashMap<String, String> {
        if test { &self.test } else { &self.without_test }
    }
}

/// The alias and crate of each `extern crate X as Y;` in `root_file` whose
/// `X` is in `names` or is `self`, the package `key`. Empty when the file
/// cannot be read or does not parse.
fn extern_crate_aliases(
    root_file: &Path,
    key: &str,
    names: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let Some(syntax) = std::fs::read_to_string(root_file)
        .ok()
        .and_then(|source| syn::parse_file(&source).ok())
    else {
        return Vec::new();
    };
    syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::ExternCrate(extern_crate) => {
                let (_, alias) = extern_crate.rename.as_ref()?;
                let name = extern_crate.ident.to_string();
                let crate_name = if name == "self" {
                    key.to_string()
                } else {
                    names.get(&name)?.clone()
                };
                Some((alias.to_string(), crate_name))
            }
            _ => None,
        })
        .collect()
}

/// The module of a workspace crate a `use` is written in, with what places
/// its paths while the binding table is collected: the module tree of every
/// workspace crate and the names that stand for one.
pub(super) struct UseScope<'a> {
    /// The crate of the module, normalized.
    pub(super) crate_name: &'a str,
    pub(super) module_path: &'a str,
    pub(super) module_paths: &'a ModulePathMap,
    /// The workspace crates of the extern prelude, by name, in the build the
    /// `use` is read in ([`ExternPrelude::names`]).
    pub(super) extern_prelude: &'a HashMap<String, String>,
    /// The names of the type items the module defines.
    pub(super) type_items: &'a HashSet<String>,
}

/// What a `use` path binds, placed by the module tree alone.
#[derive(Debug)]
pub(super) enum UsePlace {
    /// A name in a module of a workspace crate, or a crate root as the name
    /// `""` in its own root. Where the path starts from a name in scope, its
    /// first name is a binding of the module or an external crate
    /// (`from_scope`), and the target holds the path as written, from the
    /// root.
    Name {
        target: ReExportTarget,
        from_scope: bool,
    },
    /// The names a glob takes from a module, or the path of the glob as
    /// written where it goes on past the modules of the tree.
    Glob { crate_name: String, module: String },
    /// Nothing: the path names its own crate alone.
    Nothing,
}

impl UseScope<'_> {
    /// Place the `use` path `path` by the module tree. Which names the
    /// modules bind is not asked, since the table that holds them is what
    /// is being collected. From the start [`UseScope::start`] picks, each
    /// segment that names a child module enters it, and the next segment is
    /// the name. A path that ends on a module binds it as a name in its
    /// parent; a whole workspace crate binds its root. A path that goes on
    /// past the name keeps the path as written, with its last segment as the
    /// name, for
    /// [`super::bindings::ReExportMap::settle_written_paths`] to
    /// follow once the table is whole, in this crate as in another. A glob
    /// takes the names of the module its path ends on; one whose path goes on
    /// past the modules of the tree keeps it as written the same way. `None`
    /// where no start fits.
    pub(super) fn place(&self, path: &str) -> Option<UsePlace> {
        let (crate_name, segments, from_scope) = self.start(path)?;
        if segments.first().is_some_and(String::is_empty) {
            return None;
        }
        let foreign = crate_name != self.crate_name;
        if let Some((last, base)) = segments.split_last()
            && last == "*"
        {
            let (module, rest) = self.enter_modules(&crate_name, base);
            let module = if rest.is_empty() {
                module
            } else {
                base.join("::")
            };
            return Some(UsePlace::Glob { crate_name, module });
        }
        let (module, rest) = self.enter_modules(&crate_name, &segments);
        let (module, name) = match rest {
            [] if module.is_empty() && !foreign => return Some(UsePlace::Nothing),
            [] => {
                let (parent, name) = module.rsplit_once("::").unwrap_or(("", module.as_str()));
                (parent.to_string(), name.to_string())
            }
            [name] => (module, name.clone()),
            [.., last] => {
                let written = &segments[..segments.len() - 1];
                (written.join("::"), last.clone())
            }
        };
        let target = ReExportTarget {
            crate_name,
            module,
            original_name: name,
        };
        Some(UsePlace::Name { target, from_scope })
    }

    /// The crate a `use` path starts in, its segments from that crate's root,
    /// and whether it starts from a name in scope. [`PickedStart::of`] picks
    /// the start, with what this module binds as far as the table being
    /// collected tells: its child modules and the type items it defines. Any
    /// other first name the path goes on past is a binding of this module or
    /// an external crate: the path starts in this module and is marked as
    /// from scope, and
    /// [`super::bindings::ReExportMap::settle_written_paths`] tells
    /// which once the table is whole.
    fn start(&self, path: &str) -> Option<(String, Vec<String>, bool)> {
        let own = |in_crate: &str, from_scope: bool| {
            let segments = in_crate.split("::").map(ToString::to_string);
            Some((self.crate_name.to_string(), segments.collect(), from_scope))
        };
        let first_name = |first: &str, _| {
            let child = append_to_path(self.module_path, first);
            if self.module_paths.holds(self.crate_name, &child) {
                FirstName::Module {
                    crate_name: self.crate_name.to_string(),
                    path: child,
                }
            } else if self.type_items.contains(first) {
                FirstName::TypeItem
            } else {
                FirstName::Unknown
            }
        };
        match PickedStart::of(path, self.module_path, self.extern_prelude, first_name)? {
            PickedStart::OwnRoot(from_root) => own(&from_root, false),
            PickedStart::Module { path, rest, .. } => own(&followed_by(&path, rest), false),
            PickedStart::TypeItem { .. } => own(&append_to_path(self.module_path, path), false),
            PickedStart::Crate { crate_name, rest } => {
                let segments = module_segments(rest).map(ToString::to_string).collect();
                Some((crate_name, segments, false))
            }
            PickedStart::Unknown { rest } if !rest.is_empty() => {
                own(&append_to_path(self.module_path, path), true)
            }
            PickedStart::Unknown { .. } | PickedStart::Outside => None,
        }
    }

    /// Enter each of `segments` that names a child module, from the root of
    /// `crate_name`: the module reached and the segments after it.
    fn enter_modules<'s>(
        &self,
        crate_name: &str,
        segments: &'s [String],
    ) -> (String, &'s [String]) {
        let module_paths = self.module_paths.get_or_empty(crate_name);
        let mut module = String::new();
        let mut rest = segments;
        while let Some((first, tail)) = rest.split_first() {
            let child = append_to_path(&module, first);
            if !module_paths.contains(&child) {
                break;
            }
            module = child;
            rest = tail;
        }
        (module, rest)
    }
}

#[cfg(test)]
mod tests;
