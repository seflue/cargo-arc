//! Syn-based use statement parsing for workspace dependency extraction.

use crate::model::{
    CrateExportMap, DependencyKind, DependencyRef, EdgeContext, ModulePathMap, TestKind,
    WorkspaceCrates, normalize_crate_name,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use syn::UseTree;
use syn::visit::Visit;

/// Invariant context for dependency resolution within a single source file.
pub(crate) struct ResolutionContext<'a> {
    pub(crate) current_crate: &'a str,
    pub(crate) workspace_crates: &'a WorkspaceCrates,
    pub(crate) source_file: &'a Path,
    pub(crate) all_module_paths: &'a ModulePathMap,
    pub(crate) crate_exports: &'a CrateExportMap,
    pub(crate) current_module_path: &'a str,
}

/// Check whether attributes contain `#[cfg(test)]`, including compound
/// expressions like `#[cfg(all(test, feature = "..."))]`.
pub(crate) fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    fn meta_contains_test(meta: &syn::Meta) -> bool {
        use syn::parse::Parser;
        match meta {
            syn::Meta::Path(path) => path.is_ident("test"),
            syn::Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
                let parser =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated;
                parser
                    .parse2(list.tokens.clone())
                    .is_ok_and(|nested| nested.iter().any(meta_contains_test))
            }
            _ => false,
        }
    }

    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<syn::Meta>()
            .is_ok_and(|meta| meta_contains_test(&meta))
    })
}

/// Promote any context to a test context. Production becomes Unit test;
/// already-test contexts are preserved (idempotent for test contexts).
fn promote_to_test(base: &EdgeContext) -> EdgeContext {
    EdgeContext {
        kind: match base.kind {
            DependencyKind::Production => DependencyKind::Test(TestKind::Unit),
            already_test => already_test,
        },
        features: base.features.clone(),
    }
}

/// Shared `visit_item_mod` for cfg(test) scope tracking via EdgeContext.
/// Used by both `UseCollector` and `PathRefCollector` — the logic is identical.
macro_rules! impl_cfg_test_visit_item_mod {
    () => {
        fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
            let prev_context = self.context.clone();
            let prev_depth = self.inline_depth;
            if is_cfg_test(&node.attrs) {
                self.context = promote_to_test(&self.context);
            }
            // Inline modules (with body) add nesting depth; `mod foo;` (external) does not
            if node.content.is_some() {
                self.inline_depth += 1;
            }
            syn::visit::visit_item_mod(self, node);
            self.context = prev_context;
            self.inline_depth = prev_depth;
        }
    };
}

/// Collect all `use` items from a parsed file, including those nested inside
/// function bodies, blocks, and other scopes. Uses `syn::visit::Visit` to
/// traverse the full AST regardless of nesting depth.
///
/// Returns `(ItemUse, EdgeContext)` tuples: uses inside `#[cfg(test)]` scopes
/// or with `#[cfg(test)]` on the item itself are tagged `Test(Unit)`,
/// all others are `Production`.
pub(crate) fn collect_all_use_items(
    syntax: &syn::File,
    base_context: EdgeContext,
) -> Vec<(syn::ItemUse, EdgeContext, usize)> {
    struct UseCollector {
        uses: Vec<(syn::ItemUse, EdgeContext, usize)>,
        context: EdgeContext,
        inline_depth: usize,
    }
    impl<'ast> Visit<'ast> for UseCollector {
        fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
            let ctx = if is_cfg_test(&node.attrs) {
                promote_to_test(&self.context)
            } else {
                self.context.clone()
            };
            self.uses.push((node.clone(), ctx, self.inline_depth));
        }

        impl_cfg_test_visit_item_mod!();
    }
    let mut collector = UseCollector {
        uses: Vec::new(),
        context: base_context,
        inline_depth: 0,
    };
    collector.visit_file(syntax);
    collector.uses
}

/// Collect all qualified path references (2+ segments) from a parsed file.
/// Uses `syn::visit::Visit` to traverse expressions, types, patterns, and trait bounds.
/// Returns `(path_string, line_number, EdgeContext)` tuples: references inside
/// `#[cfg(test)]` scopes are tagged `Test(Unit)`, all others `Production`.
pub(crate) fn collect_all_path_refs(
    syntax: &syn::File,
    base_context: EdgeContext,
) -> Vec<(String, usize, EdgeContext, usize)> {
    struct PathRefCollector {
        paths: Vec<(String, usize, EdgeContext, usize)>,
        context: EdgeContext,
        inline_depth: usize,
    }
    impl<'ast> Visit<'ast> for PathRefCollector {
        fn visit_path(&mut self, node: &'ast syn::Path) {
            if node.segments.len() >= 2 {
                let path_str: String = node
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                let line = node
                    .segments
                    .first()
                    .map(|s| s.ident.span().start().line)
                    .unwrap_or(0);
                self.paths
                    .push((path_str, line, self.context.clone(), self.inline_depth));
            }
            // Continue visiting nested paths (e.g. in generics)
            syn::visit::visit_path(self, node);
        }

        impl_cfg_test_visit_item_mod!();
    }
    let mut collector = PathRefCollector {
        paths: Vec::new(),
        context: base_context,
        inline_depth: 0,
    };
    collector.visit_file(syntax);
    collector.paths
}

/// Join a prefix and segment with `::`, handling empty prefix.
fn append_to_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}::{segment}")
    }
}

/// Recursively resolve a `syn::UseTree` into fully-qualified path strings.
///
/// Example: `use cli::{Args, Cargo, run}` → `["cli::Args", "cli::Cargo", "cli::run"]`
fn resolve_use_tree(tree: &UseTree, prefix: &str) -> Vec<String> {
    match tree {
        UseTree::Path(p) => {
            resolve_use_tree(&p.tree, &append_to_path(prefix, &p.ident.to_string()))
        }
        // Use original name for renames — we track the *source* dependency
        UseTree::Name(n) => vec![append_to_path(prefix, &n.ident.to_string())],
        UseTree::Rename(r) => vec![append_to_path(prefix, &r.ident.to_string())],
        UseTree::Glob(_) => vec![append_to_path(prefix, "*")],
        UseTree::Group(g) => g
            .items
            .iter()
            .flat_map(|item| resolve_use_tree(item, prefix))
            .collect(),
    }
}

/// Find the longest module path prefix from `parts` that exists in `module_paths`.
///
/// Tries from longest to shortest: `["analyze", "use_parser", "normalize"]`
/// checks `"analyze::use_parser"`, then `"analyze"`.
/// Returns `(matched_path, segment_count)`.
/// Fallback: first segment with count 1.
fn find_longest_module_prefix(parts: &[&str], module_paths: &HashSet<String>) -> (String, usize) {
    for end in (1..=parts.len()).rev() {
        let candidate: String = parts[..end].join("::");
        if module_paths.contains(&candidate) {
            return (candidate, end);
        }
    }
    // Fallback: first segment
    (parts[0].to_string(), 1)
}

/// Extract an item from path parts at given index, handling trailing `{` and empty strings.
fn extract_item_from_parts(parts: &[&str], index: usize) -> Option<String> {
    let part = parts
        .get(index)?
        .trim_end_matches('{')
        .trim_end_matches(';')
        .trim();
    if part.is_empty() || part.starts_with('{') {
        None
    } else {
        Some(part.to_string())
    }
}

/// Parse crate-local imports: `use crate::module[::item]`
fn parse_crate_local_import(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    let after_crate = path.strip_prefix("crate::")?;
    let parts: Vec<&str> = after_crate.split("::").collect();

    let first = parts.first()?.trim_end_matches('{').trim();
    if first.is_empty() {
        return None;
    }

    let empty_set = HashSet::new();
    let module_paths = ctx
        .all_module_paths
        .get(&normalize_crate_name(ctx.current_crate))
        .unwrap_or(&empty_set);
    let (target_module, prefix_len) = find_longest_module_prefix(&parts, module_paths);

    Some(DependencyRef {
        target_crate: normalize_crate_name(ctx.current_crate),
        target_module,
        target_item: extract_item_from_parts(&parts, prefix_len),
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
    })
}

/// Parse bare module imports: `use cli::Args` where `cli` is a known module of the current crate.
/// Rust 2018+ resolves bare paths from any file, not just the crate root.
///
/// `ctx.current_module_path` is the relative module path of the file containing this import
/// (e.g. `"render"` for `render/mod.rs`, `""` for crate root). When non-empty, child modules
/// are checked first: `use css::X` in `render/mod.rs` resolves to `render::css` before
/// trying top-level `css`. This matches Rust 2018+/2024 semantics where bare paths in
/// non-root modules refer to children, not siblings.
fn parse_bare_module_import(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    let parts: Vec<&str> = path.split("::").collect();
    let first = parts.first()?.trim_end_matches('{').trim();
    if first.is_empty() {
        return None;
    }

    let empty_set = HashSet::new();
    let module_paths = ctx
        .all_module_paths
        .get(&normalize_crate_name(ctx.current_crate))
        .unwrap_or(&empty_set);

    // Child-module has priority (Rust 2018+/2024 semantics:
    // bare `use foo::X` in non-root module means child, not sibling/top-level)
    let effective_parts: Vec<&str> = if !ctx.current_module_path.is_empty()
        && module_paths.contains(&format!("{}::{first}", ctx.current_module_path))
    {
        ctx.current_module_path
            .split("::")
            .chain(parts.iter().copied())
            .collect()
    } else if module_paths.contains(first) {
        parts
    } else {
        return None;
    };

    let (target_module, prefix_len) = find_longest_module_prefix(&effective_parts, module_paths);

    Some(DependencyRef {
        target_crate: normalize_crate_name(ctx.current_crate),
        target_module,
        target_item: extract_item_from_parts(&effective_parts, prefix_len),
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
    })
}

/// Parse workspace crate imports: `use other_crate::module[::item]`
fn parse_workspace_import(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    let parts: Vec<&str> = path.split("::").collect();
    let crate_name = parts.first()?.trim();

    if !ctx.workspace_crates.contains(crate_name) || parts.len() < 2 {
        return None;
    }

    let module_segment = parts[1].trim_end_matches('{').trim_end_matches(';').trim();
    if module_segment.is_empty() {
        return None;
    }

    let empty_set = HashSet::new();
    let target_crate_name = normalize_crate_name(crate_name);
    let module_paths = ctx
        .all_module_paths
        .get(&target_crate_name)
        .unwrap_or(&empty_set);
    let (target_module, prefix_len) = find_longest_module_prefix(&parts[1..], module_paths);

    // Entry-point detection: if the resolved target_module is not a known module
    // and the first segment after the crate name is a known export, treat it as
    // an entry-point dependency (target_module = "").
    let is_entry_point = !module_paths.contains(&target_module)
        && ctx
            .crate_exports
            .get(&target_crate_name)
            .is_some_and(|e| e.contains(module_segment));

    let (target_module, target_item) = if is_entry_point {
        (String::new(), Some(module_segment.to_string()))
    } else {
        (
            target_module,
            extract_item_from_parts(&parts, 1 + prefix_len),
        )
    };

    Some(DependencyRef {
        target_crate: crate_name.to_string(),
        target_module,
        target_item,
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
    })
}

/// Resolve `super::` and `self::` relative paths to absolute crate-local paths.
///
/// Returns `None` when the path is not relative, when `super::`/`self::` is absorbed
/// by inline module depth, or when too many `super::` would go above crate root.
fn resolve_relative_path(
    path: &str,
    current_module_path: &str,
    inline_depth: usize,
) -> Option<String> {
    let segments: Vec<&str> = path.split("::").collect();
    let super_count = segments.iter().take_while(|&&s| s == "super").count();

    if super_count > inline_depth {
        let levels_up = super_count - inline_depth;
        return join_module_segments(current_module_path, levels_up, &segments[super_count..]);
    }

    if segments.first() == Some(&"self") && inline_depth == 0 {
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

/// Resolve a single use path through the resolution chain: crate-local → bare module → workspace.
/// Handles glob paths (`crate::module::*`) by stripping the glob and setting target_item = "*".
fn resolve_single_path(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
    inline_depth: usize,
) -> Option<DependencyRef> {
    // Resolve super::/self:: to absolute crate-local path, then route to crate:: handler
    if let Some(resolved) = resolve_relative_path(path, ctx.current_module_path, inline_depth) {
        let as_crate_path = format!("crate::{resolved}");
        return parse_crate_local_import(ctx, &as_crate_path, line_num, context);
    }

    // Handle glob: `crate::module::*` → resolve base, set target_item = "*"
    if let Some(base) = path.strip_suffix("::*") {
        let mut dep = resolve_single_path(ctx, base, line_num, context, inline_depth)?;
        // The base resolved as a module — push "*" as the item
        dep.target_item = Some("*".to_string());
        return Some(dep);
    }

    parse_crate_local_import(ctx, path, line_num, context)
        .or_else(|| parse_bare_module_import(ctx, path, line_num, context))
        .or_else(|| parse_workspace_import(ctx, path, line_num, context))
        .or_else(|| {
            // Bare workspace crate name (e.g. from `use other_crate::{Foo}` → path = "other_crate")
            if !path.contains("::") && ctx.workspace_crates.contains(path) {
                Some(DependencyRef {
                    target_crate: path.to_string(),
                    target_module: String::new(),
                    target_item: None,
                    source_file: ctx.source_file.to_path_buf(),
                    line: line_num,
                    context: context.clone(),
                })
            } else {
                None
            }
        })
}

/// Parse syn-based use items, extracting workspace-relevant dependencies.
///
/// Returns DependencyRefs for:
/// - Crate-local imports (`use crate::module`)
/// - Workspace crate imports (`use other_crate::module` where other_crate is in workspace)
///
/// Deduplicates by full_target() to keep distinct symbols but avoid duplicates.
pub(crate) fn parse_workspace_dependencies(
    use_items: &[(syn::ItemUse, EdgeContext, usize)],
    ctx: &ResolutionContext,
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashMap<(String, DependencyKind), usize> = HashMap::new();

    for (item, context, inline_depth) in use_items {
        let line_num = item.use_token.span.start().line;
        let paths = resolve_use_tree(&item.tree, "");

        for path in paths {
            if let Some(dep) = resolve_single_path(ctx, &path, line_num, context, *inline_depth) {
                DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
            }
        }
    }

    deps
}

/// Parse path references into workspace-relevant dependencies.
///
/// Takes pre-collected path refs from `collect_all_path_refs()` and resolves
/// each through the existing resolution chain (`resolve_single_path()`).
/// Deduplicates by `full_target()` — same strategy as `parse_workspace_dependencies()`.
pub(crate) fn parse_path_ref_dependencies(
    paths: &[(String, usize, EdgeContext, usize)],
    ctx: &ResolutionContext,
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashMap<(String, DependencyKind), usize> = HashMap::new();

    for (path, line_num, context, inline_depth) in paths {
        if let Some(dep) = resolve_single_path(ctx, path, *line_num, context, *inline_depth) {
            DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
        }
    }

    deps
}

/// Convenience wrapper: parse source text into syn::ItemUse items and extract dependencies.
/// Used by hir.rs which has source text but no pre-parsed AST.
#[cfg(feature = "hir")]
pub(crate) fn parse_workspace_dependencies_from_source(
    source: &str,
    ctx: &ResolutionContext,
) -> Vec<DependencyRef> {
    let syntax = match syn::parse_file(source) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let uses = collect_all_use_items(&syntax, EdgeContext::production());
    parse_workspace_dependencies(&uses, ctx)
}

#[cfg(test)]
mod tests;
