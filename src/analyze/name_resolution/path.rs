//! A path resolved to the item it names: where it starts, what each of its
//! names stands for, and where it ends.

use super::super::packages::normalize_crate_name;
use super::super::use_parser::{CrateModule, Meaning, NameBinding, NameTarget, Namespace};
use super::context::ResolutionContext;
use super::lookup::{Visited, sees_the_module};
use super::place::{
    AfterName, FirstName, PickedStart, append_to_path, followed_by, module_segments,
    resolve_relative_path,
};
use crate::model::{DependencyRef, EdgeContext, TargetRoot};

/// Resolve a single path written in the file of `ctx`: where it leads
/// ([`PathEnd::of_path`]), and whether it means the module it may end on or
/// the item of that name beside it. `meaning` is what the reference asks of
/// the path's last segment.
pub(super) fn resolve_single_path(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
    inline_path: &str,
    meaning: Meaning,
) -> Option<DependencyRef> {
    let end = PathEnd::of_path(ctx, path, inline_path)?;
    Some(
        end.module_or_item(ctx, meaning)
            .dependency(ctx, line_num, context),
    )
}

/// Where a path leads: the package, the module in it (`""` for its root),
/// the item the path names there, and the root the package is read through.
/// No item where the path names the module itself.
#[derive(Debug)]
pub(super) struct PathEnd {
    package: String,
    module: String,
    item: Option<String>,
    target_root: TargetRoot,
}

impl PathEnd {
    /// The root of the crate of `ctx`, as `crate::` names it.
    fn own_root(ctx: &ResolutionContext) -> Self {
        PathStart::own_crate(ctx).end((String::new(), None))
    }

    /// Where `path`, written in the file of `ctx` inside the inline modules
    /// `inline_path`, leads. Inside an inline module the path is read from
    /// that module, as from a file of its own. [`PickedStart::of`] picks the
    /// start, with what the module binds as the binding table has it
    /// ([`PathEnd::first_name`]); from there [`PathStart::follow`] walks the
    /// rest. A glob `base::*` leads where `base` does, with `*` as the item.
    /// `None` where no start fits, or where the walk enters a crate outside
    /// the workspace.
    fn of_path(ctx: &ResolutionContext, path: &str, inline_path: &str) -> Option<Self> {
        if !inline_path.is_empty() {
            let module = append_to_path(ctx.current_module_path, inline_path);
            return Self::of_path(&ctx.in_module(&module), path, "");
        }
        // `crate` alone, as in `use crate as name;`, is the root.
        if path == "crate" {
            return Some(Self::own_root(ctx));
        }
        // `crate::*` has no module to resolve and is the root's glob.
        if let Some(base) = path.strip_suffix("::*")
            && resolve_relative_path(path, ctx.current_module_path).is_none()
        {
            let base = if base == "crate" {
                Self::own_root(ctx)
            } else {
                Self::of_path(ctx, base, inline_path)?
            };
            return Some(base.glob());
        }
        let extern_prelude = ctx.crate_view.extern_prelude.names(ctx.test_build);
        let picked = PickedStart::of(
            path,
            ctx.current_module_path,
            extern_prelude,
            |first, after| Self::first_name(ctx, first, after),
        )?;
        let own = PathStart::own_crate(ctx);
        match picked {
            PickedStart::OwnRoot(from_root) => {
                let segments: Vec<&str> = from_root.split("::").collect();
                own.follow(ctx, &segments)
            }
            PickedStart::Module {
                crate_name,
                path,
                rest,
            } if crate_name == own.crate_name => {
                let in_crate: Vec<&str> = module_segments(&path)
                    .chain(module_segments(rest))
                    .collect();
                Some(own.walk(ctx, &in_crate))
            }
            PickedStart::Module {
                crate_name,
                path,
                rest,
            } => {
                let in_other: Vec<&str> = std::iter::once(crate_name.as_str())
                    .chain(module_segments(&path))
                    .chain(module_segments(rest))
                    .collect();
                Self::in_workspace_crate(ctx, &in_other.join("::"))
            }
            PickedStart::TypeItem { name } => {
                Some(own.end((ctx.current_module_path.to_string(), Some(name.to_string()))))
            }
            PickedStart::Crate { crate_name, rest } => {
                Self::in_workspace_crate(ctx, &followed_by(&crate_name, rest))
            }
            PickedStart::Unknown { .. } | PickedStart::Outside => {
                Self::in_external_crate(ctx, path)
            }
        }
    }

    /// What `first`, a path goes on past as `after` says, stands for in the
    /// module of `ctx`, as the binding table has it: a child module, a module
    /// a `use` or glob binds, an item of the type namespace the module
    /// defines.
    fn first_name(ctx: &ResolutionContext, first: &str, after: AfterName) -> FirstName {
        let own = PathStart::own_crate(ctx);
        if ctx.has_child_module(first) {
            return FirstName::Module {
                path: append_to_path(ctx.current_module_path, first),
                crate_name: own.crate_name,
            };
        }
        let module = CrateModule {
            crate_name: &own.crate_name,
            path: ctx.current_module_path,
        };
        let lookup = ctx.lookup(own.target_root);
        let named = ctx.crate_view.modules_bound.bound_or_look_up(
            ctx.test_build,
            ctx.current_module_path,
            first,
            after,
            || lookup.module_named(module, first, after),
        );
        if let Some(NameTarget::Module { crate_name, path }) = named {
            return FirstName::Module { crate_name, path };
        }
        let defines_a_type = after == AfterName::PathGoesOn
            && lookup.viewer.bindings_in(module).is_some_and(|bindings| {
                bindings.named(first).any(|binding| {
                    binding.definition().is_some() && binding.namespace() == Some(Namespace::Type)
                })
            });
        if defines_a_type {
            FirstName::TypeItem
        } else {
            FirstName::Unbound
        }
    }

    /// Where `path`, written from the root of `package`, a workspace crate
    /// other than this one or an external crate, leads. No name of this crate
    /// is asked: the package is known. A glob keeps its `*` as the item.
    pub(super) fn in_package(ctx: &ResolutionContext, package: &str, path: &str) -> Option<Self> {
        if let Some(base) = path
            .strip_suffix("::*")
            .or_else(|| (path == "*").then_some(""))
        {
            return Some(Self::in_package(ctx, package, base)?.glob());
        }
        let full = if path.is_empty() {
            package.to_string()
        } else {
            format!("{package}::{path}")
        };
        Self::in_workspace_crate(ctx, &full).or_else(|| Self::in_external_crate(ctx, &full))
    }

    /// Where a path through a workspace crate leads, `other_crate::module`
    /// or `other_crate` alone for its root.
    fn in_workspace_crate(ctx: &ResolutionContext, path: &str) -> Option<Self> {
        let segments: Vec<&str> = path.split("::").collect();
        let (crate_name, rest) = segments.split_first()?;
        if !ctx.crate_view.workspace_crates.contains(crate_name) {
            return None;
        }
        let start = PathStart::named_crate(ctx, crate_name);
        if rest.is_empty() {
            return Some(start.end((String::new(), None)));
        }
        start.follow(ctx, rest)
    }

    /// Where a path through an external crate leads: `use serde::Deserialize`
    /// names `Deserialize` of `serde`. What the crate holds is unknown, so
    /// everything after its name is the item.
    fn in_external_crate(ctx: &ResolutionContext, path: &str) -> Option<Self> {
        let (first, rest) = path
            .split_once("::")
            .map_or((path, None), |(first, rest)| (first, Some(rest)));
        if first.is_empty() || !ctx.crate_view.external_crate_names.contains_key(first) {
            return None;
        }
        Some(Self {
            package: first.to_string(),
            module: String::new(),
            item: rest.map(ToString::to_string),
            target_root: TargetRoot::AsSeen,
        })
    }

    /// The glob of the module this end names, or of the module holding its
    /// item.
    fn glob(self) -> Self {
        Self {
            item: Some("*".to_string()),
            ..self
        }
    }

    /// Decide whether a path that ends on a module means the module or the
    /// item of the same name in the module's parent. Beside `mod helper; pub
    /// use helper::helper;` the path `parent::helper` names the module and
    /// the fn. A call or a value means the fn, where the parent holds one; an
    /// item the parent does not show keeps the module. Otherwise only the fn
    /// is visible outside `parent`: a file that sees the module, as its
    /// declaration says from the file's crate, root and module, keeps the
    /// module. A re-export of a module under that name keeps the module, and
    /// so does a public module, which is visible wherever the item is.
    pub(super) fn module_or_item(self, ctx: &ResolutionContext, meaning: Meaning) -> Self {
        if self.item.is_some() || self.module.is_empty() {
            return self;
        }
        let (parent, name) = self
            .module
            .rsplit_once("::")
            .unwrap_or(("", self.module.as_str()));
        let lookup = ctx.lookup(self.target_root);
        let parent_module = CrateModule {
            crate_name: &self.package,
            path: parent,
        };
        let item_asked =
            !meaning.admits_a_module() && lookup.holds_an_item_named(parent_module, name);
        let item_meant = item_asked
            || lookup
                .viewer
                .bindings_in(parent_module)
                .filter(|bindings| !sees_the_module(*bindings, name))
                .and_then(|bindings| bindings.named(name).find_map(NameBinding::reexported))
                .is_some_and(|target| !lookup.binds_a_module(target));
        if !item_meant {
            return self;
        }
        let (parent, name) = (parent.to_string(), name.to_string());
        Self {
            module: parent,
            item: Some(name),
            ..self
        }
    }

    /// The dependency of the reference at `line_num` in the file of `ctx` on
    /// this end.
    pub(super) fn dependency(
        self,
        ctx: &ResolutionContext,
        line_num: usize,
        context: &EdgeContext,
    ) -> DependencyRef {
        DependencyRef {
            target_package: self.package,
            target_module: self.module,
            target_item: self.item,
            source_file: ctx.source_file.to_path_buf(),
            line: line_num,
            context: context.clone(),
            via_reexport: false,
            uses: Vec::new(),
            target_root: self.target_root,
        }
    }
}

/// The root of a workspace crate a path starts at, and the root of the
/// crate's build it is read through.
pub(super) struct PathStart {
    pub(super) crate_name: String,
    pub(super) target_root: TargetRoot,
    /// Whether the path names the crate, rather than writing `crate::` or a
    /// relative path in it. Its root's names then also come from
    /// `crate_exports`, which reads the root file alone.
    by_name: bool,
}

impl PathStart {
    /// The root of the crate of `ctx`, as `crate::` names it.
    pub(super) fn own_crate(ctx: &ResolutionContext) -> Self {
        Self {
            crate_name: normalize_crate_name(ctx.crate_view.crate_name),
            target_root: TargetRoot::AsSeen,
            by_name: false,
        }
    }

    /// The root of the workspace crate `crate_name` names. The own package by
    /// name is its lib, also from a file of the bin tree.
    fn named_crate(ctx: &ResolutionContext, crate_name: &str) -> Self {
        let crate_name = normalize_crate_name(crate_name);
        let target_root = if crate_name == normalize_crate_name(ctx.crate_view.crate_name) {
            TargetRoot::Lib
        } else {
            TargetRoot::AsSeen
        };
        Self {
            crate_name,
            target_root,
            by_name: true,
        }
    }

    /// Where `segments`, written from this root, lead. The first is asked of
    /// the root's names ([`super::lookup::NameLookup::name`]): a
    /// module carries the rest of the path into it, also in another workspace
    /// crate; an item, or a module of another crate the path ends on, is the
    /// item of the root. A name the root does not bind starts down the module
    /// tree ([`PathStart::walk`]), unless the path names the crate and its
    /// root file names the name.
    fn follow(&self, ctx: &ResolutionContext, segments: &[&str]) -> Option<PathEnd> {
        let (&first, rest) = segments.split_first()?;
        if first.is_empty() {
            return None;
        }
        let root = CrateModule {
            crate_name: &self.crate_name,
            path: "",
        };
        let named = ctx
            .lookup(self.target_root)
            .name(root, first, AfterName::followed_by(rest));
        let item_in_root = (String::new(), Some(first.to_string()));
        let reached = match named {
            Some(NameTarget::Module { crate_name, path }) if crate_name != self.crate_name => {
                if rest.is_empty() {
                    item_in_root
                } else {
                    let in_other: Vec<&str> = std::iter::once(crate_name.as_str())
                        .chain(module_segments(&path))
                        .chain(rest.iter().copied())
                        .collect();
                    return PathEnd::in_workspace_crate(ctx, &in_other.join("::"));
                }
            }
            Some(NameTarget::Module { path, .. }) => {
                let in_crate: Vec<&str> =
                    module_segments(&path).chain(rest.iter().copied()).collect();
                if in_crate.is_empty() {
                    item_in_root
                } else {
                    return Some(self.walk(ctx, &in_crate));
                }
            }
            Some(NameTarget::Item) => item_in_root,
            None if self.by_name && self.root_file_names(ctx, first) => item_in_root,
            None => return Some(self.walk(ctx, segments)),
        };
        Some(self.end(reached))
    }

    /// Whether the root file of this crate names `name`, as `crate_exports`
    /// has it.
    fn root_file_names(&self, ctx: &ResolutionContext, name: &str) -> bool {
        ctx.crate_view
            .crate_exports
            .get(&self.crate_name)
            .is_some_and(|exports| exports.contains(name))
    }

    /// Where `segments` lead from this root, one segment at a time. A segment
    /// that names a child module of the module reached so far enters it.
    /// Another segment the path goes on past is asked of the module's names,
    /// and a module below this crate's root it stands for carries the rest:
    /// beside `use crate::store::{self as bound};` in `holder`,
    /// `holder::bound::Set` names `Set` in `store`. A module of another
    /// workspace crate carries the rest into that crate. The first segment
    /// that carries the path no further is the item.
    fn walk(&self, ctx: &ResolutionContext, segments: &[&str]) -> PathEnd {
        let module_paths = ctx.crate_view.module_paths.get_or_empty(&self.crate_name);
        let lookup = ctx.lookup(self.target_root);
        let mut visited = Visited::default();
        let mut module = String::new();
        let mut rest = segments;
        loop {
            while let Some((first, tail)) = rest.split_first() {
                let child = append_to_path(&module, first);
                if !module_paths.contains(&child) {
                    break;
                }
                module = child;
                rest = tail;
            }
            let Some((&name, tail)) = rest.split_first() else {
                return self.end((module, None));
            };
            let item = (!name.is_empty()).then(|| name.to_string());
            if tail.is_empty() {
                return self.end((module, item));
            }
            let holder = CrateModule {
                crate_name: &self.crate_name,
                path: &module,
            };
            match lookup.name_passing(holder, name, AfterName::PathGoesOn, &mut visited) {
                Some(NameTarget::Module { crate_name, path })
                    if crate_name == self.crate_name && !path.is_empty() =>
                {
                    module = path;
                    rest = tail;
                }
                Some(NameTarget::Module { crate_name, path }) if crate_name != self.crate_name => {
                    let in_other: Vec<&str> = std::iter::once(crate_name.as_str())
                        .chain(module_segments(&path))
                        .chain(tail.iter().copied())
                        .collect();
                    return PathEnd::in_workspace_crate(ctx, &in_other.join("::"))
                        .unwrap_or_else(|| self.end((module, item)));
                }
                Some(NameTarget::Module { .. } | NameTarget::Item) | None => {
                    return self.end((module, item));
                }
            }
        }
    }

    /// The end at `reached`, a module of this crate and the item there.
    fn end(&self, (module, item): (String, Option<String>)) -> PathEnd {
        PathEnd {
            package: self.crate_name.clone(),
            module,
            item,
            target_root: self.target_root,
        }
    }
}
