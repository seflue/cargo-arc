//! Syn-based use statement parsing for workspace dependency extraction.

use crate::model::{
    CrateExportMap, DefKind, Definition, DependencyRef, EdgeContext, ModulePathMap, SymbolUse,
    TestKind, UsageKind, UseCategory, WorkspaceCrates, normalize_crate_name,
};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::Path;
use syn::UseTree;
use syn::visit::Visit;

use super::mod_resolver::is_cfg_test;

// ---------------------------------------------------------------------------
// Re-export resolution types (moved from reexports.rs)
// ---------------------------------------------------------------------------

/// Check whether visibility qualifies as a re-export:
/// pub, pub(crate), pub(super) — but NOT private or pub(in path).
pub(crate) fn is_reexport_visibility(vis: &syn::Visibility) -> bool {
    match vis {
        syn::Visibility::Public(_) => true,
        syn::Visibility::Restricted(r) => {
            r.in_token.is_none() && (r.path.is_ident("crate") || r.path.is_ident("super"))
        }
        syn::Visibility::Inherited => false,
    }
}

/// Where a re-exported symbol originally comes from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReExportTarget {
    /// Source module path (crate-relative, e.g. `"render::elements"`)
    pub(crate) module: String,
    /// Original name in the source module (differs from map key on rename)
    pub(crate) original_name: String,
}

/// The items a type or trait carries with it, by name. They decide what
/// `Type::item` takes from the type: a variant builds a value, a fn is called,
/// a const is read.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct AssociatedItems {
    pub(crate) variants: HashSet<String>,
    pub(crate) fns: HashSet<String>,
    pub(crate) consts: HashSet<String>,
}

impl AssociatedItems {
    pub(crate) fn add_fn(&mut self, name: &syn::Ident) {
        self.fns.insert(name.to_string());
    }

    pub(crate) fn add_const(&mut self, name: &syn::Ident) {
        self.consts.insert(name.to_string());
    }

    /// Take in `other`'s items, so two `impl` blocks of one type add up.
    pub(crate) fn merge(&mut self, other: Self) {
        self.variants.extend(other.variants);
        self.fns.extend(other.fns);
        self.consts.extend(other.consts);
    }
}

/// Export and re-export information for a single module.
#[derive(Debug, Default, Clone)]
pub(crate) struct ModuleExportInfo {
    /// Own public definitions: name → kind and line at the definition site
    pub(crate) definitions: HashMap<String, Definition>,
    /// The associated items of the types and traits defined here, by the
    /// defined name. An `impl` block in another file of the crate lands here
    /// too, on the module that defines the type.
    pub(crate) associated: HashMap<String, AssociatedItems>,
    /// Explicit re-exports: alias/name → source target
    pub(crate) explicit_reexports: HashMap<String, ReExportTarget>,
    /// Private `use` bindings this module holds. Rust makes them visible to
    /// descendant modules, which may then name the symbol through this module
    /// even though it defines nothing. Resolving through them attributes the
    /// edge to the definer, not to this ancestor. Name → source target.
    pub(crate) private_uses: HashMap<String, ReExportTarget>,
    /// Glob re-export sources (module paths from `pub use *`)
    pub(crate) glob_sources: Vec<String>,
    /// Private glob sources (module paths from a private `use *`). Like
    /// `private_uses`, visible to descendants only, which may name any of
    /// the forwarded symbols through this module.
    pub(crate) private_glob_sources: Vec<String>,
}

impl ModuleExportInfo {
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.definitions.is_empty()
            && self.associated.is_empty()
            && self.explicit_reexports.is_empty()
            && self.private_uses.is_empty()
            && self.glob_sources.is_empty()
            && self.private_glob_sources.is_empty()
    }
}

/// Crate name → (module path → export info).
/// Module paths are crate-relative (e.g. `"render"`, `"analyze::use_parser"`).
/// Empty string "" = crate root (lib.rs/main.rs).
#[derive(Debug, Default, Clone)]
pub(crate) struct ReExportMap(HashMap<String, HashMap<String, ModuleExportInfo>>);

impl ReExportMap {
    /// Look up the export info of the module a dependency points at, where
    /// the map has it.
    pub(crate) fn module_info(&self, dep: &DependencyRef) -> Option<&ModuleExportInfo> {
        self.0.get(&dep.target_crate)?.get(&dep.target_module)
    }
}

impl Deref for ReExportMap {
    type Target = HashMap<String, HashMap<String, ModuleExportInfo>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromIterator<(String, HashMap<String, ModuleExportInfo>)> for ReExportMap {
    fn from_iter<I: IntoIterator<Item = (String, HashMap<String, ModuleExportInfo>)>>(
        iter: I,
    ) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// Resolve re-exports in a [`DependencyRef`]: follow the re-export chain
/// until the original definition module is found.
/// Modifies `dep.target_module` in place. No-op if no re-export applies.
pub(crate) fn resolve_reexport(
    dep: &mut DependencyRef,
    reexport_map: &ReExportMap,
    importer_module: &str,
) {
    let Some(crate_exports) = reexport_map.get(&dep.target_crate) else {
        return;
    };
    let mut visited = HashSet::new();
    let mut lookup_name = dep.target_item.clone();

    loop {
        if !visited.insert(dep.target_module.clone()) {
            break;
        }
        let Some(module_info) = crate_exports.get(&dep.target_module) else {
            break;
        };
        let Some(item) = &lookup_name else {
            break;
        };

        if module_info.definitions.contains_key(item) {
            break;
        }

        // Tier 1: Explicit re-export
        if let Some(target) = module_info.explicit_reexports.get(item) {
            let original_target = dep.target_module.clone();
            dep.target_module = target.module.clone();
            tracing::debug!(
                "re-export resolved: {} -> {} (via re-export in {})",
                original_target,
                dep.target_module,
                original_target
            );
            lookup_name = Some(target.original_name.clone());
            continue;
        }

        // Tier 1b: Private `use` binding, visible only to descendants of the
        // module that holds it. A descendant naming the symbol through this
        // ancestor really depends on the definer.
        if is_descendant(importer_module, &dep.target_module)
            && let Some(target) = module_info.private_uses.get(item)
        {
            dep.target_module = target.module.clone();
            lookup_name = Some(target.original_name.clone());
            continue;
        }

        // Tier 2: Glob re-exports
        let mut found = false;
        for glob_src in &module_info.glob_sources {
            let mut glob_visited = HashSet::new();
            if module_exports_symbol(crate_exports, glob_src, item, &mut glob_visited) {
                let original_target = dep.target_module.clone();
                dep.target_module = glob_src.clone();
                tracing::debug!(
                    "re-export resolved: {} -> {} (via glob re-export in {})",
                    original_target,
                    dep.target_module,
                    original_target
                );
                found = true;
                break;
            }
        }
        if found {
            continue;
        }

        // Tier 2b: Private glob, visible only to descendants of the module
        // that holds it, like Tier 1b.
        if is_descendant(importer_module, &dep.target_module)
            && let Some(glob_src) = module_info.private_glob_sources.iter().find(|glob_src| {
                module_scope_holds_symbol(
                    crate_exports,
                    glob_src,
                    item,
                    importer_module,
                    &mut HashSet::new(),
                )
            })
        {
            dep.target_module = glob_src.clone();
            continue;
        }

        break;
    }
}

/// Check whether a module exports a symbol (own definition OR re-export).
fn module_exports_symbol(
    crate_exports: &HashMap<String, ModuleExportInfo>,
    module_path: &str,
    symbol: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(module_path.to_string()) {
        return false;
    }
    let Some(info) = crate_exports.get(module_path) else {
        return false;
    };
    if info.definitions.contains_key(symbol) {
        return true;
    }
    if info.explicit_reexports.contains_key(symbol) {
        return true;
    }
    for glob_src in &info.glob_sources {
        if module_exports_symbol(crate_exports, glob_src, symbol, visited) {
            return true;
        }
    }
    false
}

/// Whether `importer_module` can name `symbol` through `module_path`: what
/// the module exports, plus what its private globs forward to a descendant.
/// A private glob is followed on direct hops only; a `pub use m::*` on the
/// way republishes `m`'s public items, none of `m`'s private imports.
fn module_scope_holds_symbol(
    crate_exports: &HashMap<String, ModuleExportInfo>,
    module_path: &str,
    symbol: &str,
    importer_module: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(module_path.to_string()) {
        return false;
    }
    if module_exports_symbol(crate_exports, module_path, symbol, &mut HashSet::new()) {
        return true;
    }
    if !is_descendant(importer_module, module_path) {
        return false;
    }
    let Some(info) = crate_exports.get(module_path) else {
        return false;
    };
    info.private_glob_sources.iter().any(|glob_src| {
        module_scope_holds_symbol(crate_exports, glob_src, symbol, importer_module, visited)
    })
}

/// Whether `module` is `ancestor` itself or nested below it. The crate root
/// (`""`) is an ancestor of every module.
fn is_descendant(module: &str, ancestor: &str) -> bool {
    ancestor.is_empty() || module == ancestor || module.starts_with(&format!("{ancestor}::"))
}

/// Invariant context for dependency resolution within a single source file.
pub(crate) struct ResolutionContext<'a> {
    pub(crate) current_crate: &'a str,
    pub(crate) workspace_crates: &'a WorkspaceCrates,
    pub(crate) source_file: &'a Path,
    pub(crate) all_module_paths: &'a ModulePathMap,
    pub(crate) crate_exports: &'a CrateExportMap,
    pub(crate) current_module_path: &'a str,
    pub(crate) reexport_map: &'a ReExportMap,
    /// Code-side crate name -> `package_id` for external crates visible to
    /// the current workspace crate. Populated from `crate_name_map[current_crate]`.
    pub(crate) external_crate_names: &'a HashMap<String, String>,
}

/// Promote any context to a test context. Production becomes Unit test;
/// already-test contexts are preserved (idempotent for test contexts).
fn promote_to_test(base: &EdgeContext) -> EdgeContext {
    EdgeContext {
        kind: match base.kind {
            UsageKind::Production => UsageKind::Test(TestKind::Unit),
            already_test => already_test,
        },
        features: base.features.clone(),
    }
}

/// Where a `use` binds: an inline module body, a block, or the file itself.
pub(crate) type RegionId = usize;

/// The binding regions of one file, as parent links.
///
/// A `use` is an item and binds in its enclosing module or block only. A later
/// reference therefore sees a binding when the binding's region is the
/// reference's own region or one enclosing it.
///
/// Both collectors number the regions of the same file, and a reference from one
/// walk is looked up against bindings from the other. The two numberings have to
/// agree, which is why both take them from `impl_binding_region_visits!` and
/// neither prunes its traversal.
#[derive(Debug, Clone)]
pub(crate) struct BindingRegions {
    parents: Vec<Option<RegionId>>,
}

impl BindingRegions {
    /// The file itself. Encloses every other region and is enclosed by none.
    pub(crate) const ROOT: RegionId = 0;

    fn open(&mut self, parent: RegionId) -> RegionId {
        self.parents.push(Some(parent));
        self.parents.len() - 1
    }

    #[cfg(test)]
    pub(crate) fn count(&self) -> usize {
        self.parents.len()
    }

    /// A region and everything enclosing it, innermost first.
    fn enclosing(&self, region: RegionId) -> impl Iterator<Item = RegionId> + '_ {
        std::iter::successors(Some(region), move |&inner| {
            self.parents.get(inner).copied().flatten()
        })
    }
}

impl Default for BindingRegions {
    fn default() -> Self {
        Self {
            parents: vec![None],
        }
    }
}

/// Shared `visit_item_mod` and `visit_block` for both collectors.
///
/// Both open a binding region. `visit_item_mod` also carries the cfg(test)
/// context and the inline module depth, which counts modules only: `use super::X`
/// in a function body still means the parent module.
macro_rules! impl_binding_region_visits {
    () => {
        fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
            let prev_context = self.context.clone();
            let prev_depth = self.inline_depth;
            let prev_region = self.region;
            if is_cfg_test(&node.attrs) {
                self.context = promote_to_test(&self.context);
            }
            // Inline modules (with body) add nesting depth; `mod foo;` (external) does not
            if node.content.is_some() {
                self.inline_depth += 1;
                self.region = self.regions.open(prev_region);
            }
            syn::visit::visit_item_mod(self, node);
            self.context = prev_context;
            self.inline_depth = prev_depth;
            self.region = prev_region;
        }

        fn visit_block(&mut self, node: &'ast syn::Block) {
            let prev_region = self.region;
            self.region = self.regions.open(prev_region);
            syn::visit::visit_block(self, node);
            self.region = prev_region;
        }
    };
}

/// One `use` item with what its position says about it.
pub(crate) struct CollectedUse {
    pub(crate) item: syn::ItemUse,
    pub(crate) context: EdgeContext,
    pub(crate) inline_depth: usize,
    pub(crate) region: RegionId,
}

/// The `use` items of one file, with the regions they were written in.
#[derive(Default)]
pub(crate) struct CollectedUses {
    items: Vec<CollectedUse>,
    regions: BindingRegions,
}

impl Deref for CollectedUses {
    type Target = [CollectedUse];

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

/// Collect all `use` items from a parsed file, including those nested inside
/// function bodies, blocks, and other scopes. Uses `syn::visit::Visit` to
/// traverse the full AST regardless of nesting depth.
///
/// Uses inside `#[cfg(test)]` scopes or with `#[cfg(test)]` on the item itself
/// are tagged `Test(Unit)`, all others are `Production`.
pub(crate) fn collect_all_use_items(
    syntax: &syn::File,
    base_context: EdgeContext,
) -> CollectedUses {
    struct UseCollector {
        uses: Vec<CollectedUse>,
        context: EdgeContext,
        inline_depth: usize,
        regions: BindingRegions,
        region: RegionId,
    }
    impl<'ast> Visit<'ast> for UseCollector {
        fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
            let ctx = if is_cfg_test(&node.attrs) {
                promote_to_test(&self.context)
            } else {
                self.context.clone()
            };
            self.uses.push(CollectedUse {
                item: node.clone(),
                context: ctx,
                inline_depth: self.inline_depth,
                region: self.region,
            });
        }

        impl_binding_region_visits!();
    }
    let mut collector = UseCollector {
        uses: Vec::new(),
        context: base_context,
        inline_depth: 0,
        regions: BindingRegions::default(),
        region: BindingRegions::ROOT,
    };
    collector.visit_file(syntax);
    CollectedUses {
        items: collector.uses,
        regions: collector.regions,
    }
}

/// Where a path stands. Together with what the path names, this decides the
/// category of the use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathPosition {
    /// A type, a trait bound, or the trait of an `impl`.
    Type,
    /// The callee of a call expression, or the name of a macro.
    Call,
    /// The path of a struct expression or of a struct pattern.
    Construct,
    /// A path read as a value or matched as a pattern, and a path anywhere
    /// else, such as in an attribute.
    Value,
    /// A bare identifier in a pattern. It matches a const, a static or a unit
    /// struct a `use` brings in, and declares a fresh variable otherwise.
    Binding,
}

/// One path reference with what its position says about it.
#[derive(Debug)]
pub(crate) struct PathRef {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) context: EdgeContext,
    pub(crate) inline_depth: usize,
    pub(crate) region: RegionId,
    pub(crate) position: PathPosition,
    /// Written in macro input or an attribute, where syn does not parse and
    /// the segments are read off the tokens. A binding resolves such a path;
    /// the resolution chain is not asked, so no edge comes from macro input
    /// alone.
    pub(crate) in_macro: bool,
}

/// The qualified path references of one file, with the regions they stand in.
#[derive(Debug, Default)]
pub(crate) struct CollectedPathRefs {
    refs: Vec<PathRef>,
    /// The regions this walk numbered. A reference is looked up against bindings
    /// numbered by the other walk, so the two numberings have to match. Only the
    /// test that guards that match reads them; resolution goes through the
    /// numbering that came with the bindings.
    #[cfg(test)]
    regions: BindingRegions,
}

impl Deref for CollectedPathRefs {
    type Target = [PathRef];

    fn deref(&self) -> &Self::Target {
        &self.refs
    }
}

/// The walker behind [`collect_all_path_refs`].
struct PathRefCollector {
    paths: Vec<PathRef>,
    context: EdgeContext,
    inline_depth: usize,
    regions: BindingRegions,
    region: RegionId,
    position: PathPosition,
}
impl PathRefCollector {
    fn in_position(&mut self, position: PathPosition, visit: impl FnOnce(&mut Self)) {
        let prev = self.position;
        self.position = position;
        visit(self);
        self.position = prev;
    }

    fn record(&mut self, path: String, line: usize, in_macro: bool) {
        self.paths.push(PathRef {
            path,
            line,
            context: self.context.clone(),
            inline_depth: self.inline_depth,
            region: self.region,
            position: self.position,
            in_macro,
        });
    }

    /// Read the paths off a token stream syn leaves unparsed: a run of
    /// identifiers joined by `::`, at any nesting of brackets. Every other
    /// token is skipped, so nothing here knows what position a path is in.
    fn record_token_paths(&mut self, tokens: &proc_macro2::TokenStream) {
        use proc_macro2::TokenTree;
        let tokens: Vec<TokenTree> = tokens.clone().into_iter().collect();
        let mut index = 0;
        while index < tokens.len() {
            match &tokens[index] {
                TokenTree::Group(group) => self.record_token_paths(&group.stream()),
                // A lifetime is a `'` followed by an identifier, not a name.
                TokenTree::Ident(ident) if !is_lifetime_tick(tokens.get(index.wrapping_sub(1))) => {
                    let line = ident.span().start().line;
                    let mut path = vec![ident.to_string()];
                    while is_path_separator(&tokens[index + 1..])
                        && let Some(TokenTree::Ident(next)) = tokens.get(index + 3)
                    {
                        path.push(next.to_string());
                        index += 3;
                    }
                    self.in_position(PathPosition::Value, |s| {
                        s.record(path.join("::"), line, true);
                    });
                }
                _ => {}
            }
            index += 1;
        }
    }
}

/// Whether the tokens start with `::`, which arrives as two colons with the
/// first joint to the second.
fn is_path_separator(tokens: &[proc_macro2::TokenTree]) -> bool {
    use proc_macro2::{Spacing, TokenTree};
    matches!(
        tokens,
        [TokenTree::Punct(first), TokenTree::Punct(second), ..]
            if first.as_char() == ':' && first.spacing() == Spacing::Joint && second.as_char() == ':'
    )
}

fn is_lifetime_tick(token: Option<&proc_macro2::TokenTree>) -> bool {
    matches!(token, Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == '\'')
}
impl<'ast> Visit<'ast> for PathRefCollector {
    fn visit_path(&mut self, node: &'ast syn::Path) {
        let path_str: String = node
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        let line = node
            .segments
            .first()
            .map_or(0, |s| s.ident.span().start().line);
        self.record(path_str, line, false);
        syn::visit::visit_path(self, node);
    }

    // Syntax alone cannot tell a const matched from a variable declared; the
    // lookup against the file's bindings and the item's definition can.
    fn visit_pat_ident(&mut self, node: &'ast syn::PatIdent) {
        if node.by_ref.is_none() && node.mutability.is_none() && node.subpat.is_none() {
            self.in_position(PathPosition::Binding, |s| {
                s.record(
                    node.ident.to_string(),
                    node.ident.span().start().line,
                    false,
                );
            });
        }
        syn::visit::visit_pat_ident(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        self.in_position(PathPosition::Type, |s| syn::visit::visit_type_path(s, node));
    }

    fn visit_trait_bound(&mut self, node: &'ast syn::TraitBound) {
        self.in_position(PathPosition::Type, |s| {
            syn::visit::visit_trait_bound(s, node);
        });
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.in_position(PathPosition::Type, |s| syn::visit::visit_item_impl(s, node));
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        self.in_position(PathPosition::Value, |s| {
            syn::visit::visit_expr_path(s, node);
        });
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(func) = &*node.func {
            self.in_position(PathPosition::Call, |s| syn::visit::visit_expr_path(s, func));
            for arg in &node.args {
                self.visit_expr(arg);
            }
        } else {
            syn::visit::visit_expr_call(self, node);
        }
    }

    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        self.in_position(PathPosition::Construct, |s| {
            syn::visit::visit_expr_struct(s, node);
        });
    }

    fn visit_pat_struct(&mut self, node: &'ast syn::PatStruct) {
        self.in_position(PathPosition::Construct, |s| {
            syn::visit::visit_pat_struct(s, node);
        });
    }

    fn visit_pat_tuple_struct(&mut self, node: &'ast syn::PatTupleStruct) {
        self.in_position(PathPosition::Construct, |s| {
            syn::visit::visit_pat_tuple_struct(s, node);
        });
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.in_position(PathPosition::Call, |s| syn::visit::visit_macro(s, node));
        self.record_token_paths(&node.tokens);
    }

    // `#[derive(Serialize)]` and the like name what they use in tokens too.
    fn visit_meta_list(&mut self, node: &'ast syn::MetaList) {
        syn::visit::visit_meta_list(self, node);
        self.record_token_paths(&node.tokens);
    }

    // `pub(in path)` names the scope that may see the item; nothing is used
    // from that module, so its path is no dependency.
    fn visit_vis_restricted(&mut self, _node: &'ast syn::VisRestricted) {}

    impl_binding_region_visits!();
}

/// Collect all path references from a parsed file, each with its position.
/// Uses `syn::visit::Visit` to traverse expressions, types, patterns, and trait bounds.
/// References inside `#[cfg(test)]` scopes are tagged `Test(Unit)`, all others
/// `Production`.
///
/// A single-segment path is kept too: it may name what a `use` in the file
/// binds. The position is the innermost syntactic context around the path;
/// the collector sets it on entering that context, so a path nested in another
/// path's generic arguments gets its own.
pub(crate) fn collect_all_path_refs(
    syntax: &syn::File,
    base_context: EdgeContext,
) -> CollectedPathRefs {
    let mut collector = PathRefCollector {
        paths: Vec::new(),
        context: base_context,
        inline_depth: 0,
        regions: BindingRegions::default(),
        region: BindingRegions::ROOT,
        position: PathPosition::Value,
    };
    collector.visit_file(syntax);
    CollectedPathRefs {
        refs: collector.paths,
        #[cfg(test)]
        regions: collector.regions,
    }
}

/// Join a prefix and segment with `::`, handling empty prefix.
fn append_to_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}::{segment}")
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
pub(crate) fn resolve_use_tree(tree: &UseTree, prefix: &str, use_alias: bool) -> Vec<String> {
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

    let module_paths = ctx
        .all_module_paths
        .get_or_empty(&normalize_crate_name(ctx.current_crate));
    let (target_module, prefix_len) = find_longest_module_prefix(&parts, module_paths);

    Some(DependencyRef {
        target_crate: normalize_crate_name(ctx.current_crate),
        target_module,
        target_item: extract_item_from_parts(&parts, prefix_len),
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
        via_reexport: false,
        uses: Vec::new(),
    })
}

/// Parse bare module imports: `use cli::Args` where `cli` is a module in scope
/// of the file.
///
/// `ctx.current_module_path` is the relative module path of the file containing
/// this import (e.g. `"render"` for `render/mod.rs`, `""` for crate root). A
/// bare first segment names a child of that module: `use css::X` in
/// `render/mod.rs` is `render::css`, `use cli::X` in the root is `cli`. A
/// top-level module is a child of the root only; from any other file its bare
/// name is not in scope, and what stands there is an extern crate or a name a
/// `use` bound.
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

    let module_paths = ctx
        .all_module_paths
        .get_or_empty(&normalize_crate_name(ctx.current_crate));

    let effective_parts: Vec<&str> = if ctx.current_module_path.is_empty() {
        if !module_paths.contains(first) {
            return None;
        }
        parts
    } else if module_paths.contains(&format!("{}::{first}", ctx.current_module_path)) {
        ctx.current_module_path
            .split("::")
            .chain(parts.iter().copied())
            .collect()
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
        via_reexport: false,
        uses: Vec::new(),
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

    let target_crate_name = normalize_crate_name(crate_name);
    let module_paths = ctx.all_module_paths.get_or_empty(&target_crate_name);
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
        via_reexport: false,
        uses: Vec::new(),
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
/// Handles glob paths (`crate::module::*`) by stripping the glob and setting `target_item` = "*".
pub(crate) fn resolve_single_path(
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
                    via_reexport: false,
                    uses: Vec::new(),
                })
            } else {
                None
            }
        })
        .or_else(|| parse_external_crate_import(ctx, path, line_num, context))
}

/// Parse external crate imports: `use serde::Deserialize` where `serde` is a known external crate.
/// Fallback at the end of the resolution chain — only matches if no workspace resolution succeeded.
fn parse_external_crate_import(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    let parts: Vec<&str> = path.split("::").collect();
    let first = parts.first()?.trim();
    if first.is_empty() {
        return None;
    }

    // Check if the first path segment is a known external crate name
    if !ctx.external_crate_names.contains_key(first) {
        return None;
    }

    let target_item = if parts.len() > 1 {
        Some(parts[1..].join("::"))
    } else {
        None
    };

    Some(DependencyRef {
        target_crate: first.to_string(),
        target_module: String::new(),
        target_item,
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
        via_reexport: false,
        uses: Vec::new(),
    })
}

/// Parse syn-based use items, extracting workspace-relevant dependencies.
///
/// Returns `DependencyRefs` for:
/// - Crate-local imports (`use crate::module`)
/// - Workspace crate imports (`use other_crate::module` where `other_crate` is in workspace)
///
/// Deduplicates by `full_target()` to keep distinct symbols but avoid duplicates.
pub(crate) fn parse_workspace_dependencies(
    use_items: &CollectedUses,
    ctx: &ResolutionContext,
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashMap<(String, UsageKind), usize> = HashMap::new();

    for collected in use_items.iter() {
        let item = &collected.item;
        let line_num = item.use_token.span.start().line;
        let paths = resolve_use_tree(&item.tree, "", false);
        // A `use` that republishes a name under a visibility reachable from
        // outside the module is a re-export. Path references (non-use) are never
        // re-exports.
        let via_reexport = is_reexport_visibility(&item.vis);

        for path in paths {
            if let Some(mut dep) = resolve_single_path(
                ctx,
                &path,
                line_num,
                &collected.context,
                collected.inline_depth,
            ) {
                // A private glob whose target the map knows is resolved into
                // the names the file uses, by `parse_path_ref_dependencies`.
                // A `pub use` glob republishes every name and uses none, so it
                // is the whole payload.
                if is_glob(&dep) && glob_payload_known(&dep, ctx.reexport_map) && !via_reexport {
                    continue;
                }
                dep.via_reexport = via_reexport;
                for mut dep in expand_glob(dep, ctx.reexport_map) {
                    resolve_reexport(&mut dep, ctx.reexport_map, ctx.current_module_path);
                    DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
                }
            }
        }
    }

    deps
}

/// The names `use <module>::*` brings into scope: the module's own definitions
/// plus what it republishes under a name.
///
/// `None` when the module is absent from the map or exports nothing under a name
/// it can see. Sorted, because the map iterates in arbitrary order and the names
/// reach the rendered output.
fn glob_payload(dep: &DependencyRef, reexport_map: &ReExportMap) -> Option<Vec<String>> {
    let module_info = reexport_map.module_info(dep)?;
    let mut names: Vec<String> = module_info
        .definitions
        .keys()
        .chain(module_info.explicit_reexports.keys())
        .cloned()
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort_unstable();
    names.dedup();
    Some(names)
}

/// Split a re-export glob into one dependency per name it republishes, so an
/// edge weighs what crosses it rather than the one line that spells it.
///
/// Non-globs pass through. So does a glob whose payload is unknown: it keeps the
/// `*`, which scores as a single unnamed symbol — an understatement, but a
/// smaller one than dropping the edge to zero.
fn expand_glob(dep: DependencyRef, reexport_map: &ReExportMap) -> Vec<DependencyRef> {
    if !is_glob(&dep) {
        return vec![dep];
    }
    let Some(names) = glob_payload(&dep, reexport_map) else {
        return vec![dep];
    };
    names
        .into_iter()
        .map(|name| DependencyRef {
            target_item: Some(name),
            ..dep.clone()
        })
        .collect()
}

/// The `*` marker on a glob's dependency. It must survive upstream of here:
/// `collect_use_reexports` reads it out of its own `resolve_use_tree` pass to
/// build `glob_sources`, which is what `resolve_reexport` resolves glob chains
/// against.
fn is_glob(dep: &DependencyRef) -> bool {
    dep.target_item.as_deref() == Some("*")
}

/// Whether the map holds the module a glob imports from, and so can say which
/// names the glob brings in.
fn glob_payload_known(dep: &DependencyRef, reexport_map: &ReExportMap) -> bool {
    reexport_map.module_info(dep).is_some()
}

/// What every name in one file means, per region: module aliases, names bound
/// elsewhere, names bound to items, glob imports, and the file's own item
/// names.
///
/// `modules` holds a binding name → the absolute path of the module it names.
/// Absolute means `crate::a::b` for the current crate and `other_crate::b`
/// otherwise, so a binding stays valid wherever in the file it is used.
///
/// `elsewhere` holds the names bound to something this crate does not contain.
/// The parser need not know what that something is. An import from a crate it has
/// no metadata for still binds the name, and the module beside the file is then not
/// what a bare path starting with that name means.
///
/// `items` holds the names bound to an item, with the dependency the `use`
/// resolved to. A later path starting with such a name is an occurrence of that
/// item, and what follows the name is an associated item of it.
///
/// `globs` holds the glob imports whose payload the map knows, as the
/// dependency on the module with the `*` still in place. A name nothing else
/// binds and the file does not define is the glob's where the module exports
/// it.
#[derive(Debug, Default)]
pub(crate) struct FileBindings {
    regions: BindingRegions,
    modules: HashMap<RegionId, HashMap<String, String>>,
    elsewhere: HashMap<RegionId, HashSet<String>>,
    items: HashMap<RegionId, HashMap<String, DependencyRef>>,
    globs: HashMap<RegionId, Vec<DependencyRef>>,
    /// The names of the items the file defines, at any depth. A glob never
    /// brings in a name the file defines itself.
    local_definitions: HashSet<String>,
}

/// What one path reference stands for, given the bindings around it.
enum PathBinding<'a> {
    /// A path to run through the resolution chain, with a bound module alias
    /// replaced by the module's absolute path.
    Path(Cow<'a, str>),
    /// An occurrence of an item a `use` binds, plus the associated item the
    /// path goes on to name: `Config::new` on the binding `Config` leaves
    /// `new`.
    Item(&'a DependencyRef, Option<&'a str>),
    /// A path whose first segment is bound to something this crate does not
    /// contain.
    Elsewhere,
    /// A bare name nothing binds, split into the name and what follows it.
    /// It may be a glob's.
    Unbound(&'a str, Option<&'a str>),
}

impl FileBindings {
    /// Build the bindings of one file from its `use` items and the items it
    /// defines.
    ///
    /// `use crate::device::{queue, Device}` binds `queue` to the module
    /// `device::queue`; a later `queue::TempResource` is only resolvable against
    /// that binding. What each binding means for later references is decided in
    /// [`classify_binding`].
    pub(crate) fn of(
        syntax: &syn::File,
        use_items: &CollectedUses,
        ctx: &ResolutionContext,
    ) -> FileBindings {
        let mut bindings = FileBindings {
            regions: use_items.regions.clone(),
            ..FileBindings::default()
        };

        for collected in use_items.iter() {
            let alias_paths = resolve_use_tree(&collected.item.tree, "", true);
            let original_paths = resolve_use_tree(&collected.item.tree, "", false);

            for (alias_path, original_path) in alias_paths.iter().zip(original_paths.iter()) {
                let Some(binding) = alias_path.rsplit("::").next() else {
                    continue;
                };
                let dep = resolve_single_path(
                    ctx,
                    original_path,
                    collected.item.use_token.span.start().line,
                    &collected.context,
                    collected.inline_depth,
                );
                // A glob binds every name its module exports and none of them is
                // written here; the names are found where the file uses them.
                if binding == "*" {
                    if let Some(mut dep) = dep
                        && glob_payload_known(&dep, ctx.reexport_map)
                    {
                        dep.via_reexport = is_reexport_visibility(&collected.item.vis);
                        bindings.bind_glob(collected.region, dep);
                    }
                    continue;
                }
                match classify_binding(ctx, dep.as_ref(), binding) {
                    Binding::Module(module_path) => {
                        bindings.bind_module(collected.region, binding, module_path);
                    }
                    Binding::Elsewhere => bindings.bind_elsewhere(collected.region, binding),
                    Binding::OwnItem => {}
                }
                // A name bound to an item of any crate the parser placed carries the
                // item's occurrences. A name bound elsewhere may also be one: the
                // parser has no module for it, but it knows the crate.
                if let Some(mut dep) = dep
                    && dep.target_item.is_some()
                {
                    dep.via_reexport = is_reexport_visibility(&collected.item.vis);
                    bindings.bind_item(collected.region, binding, dep);
                }
            }
        }

        bindings.define_locally(syntax);
        bindings
    }

    /// The module path a name is bound to as seen from `region`, where it is
    /// bound to one.
    #[cfg(test)]
    fn target(&self, region: RegionId, name: &str) -> Option<&str> {
        self.regions
            .enclosing(region)
            .find_map(|enclosing| self.modules.get(&enclosing)?.get(name))
            .map(String::as_str)
    }

    /// Look up what a reference standing in `region` stands for.
    ///
    /// `queue::TempResource` under `queue → crate::device::queue` is the path
    /// `crate::device::queue::TempResource`. `Config::new` under an item binding
    /// `Config` is an occurrence of that item. A first segment bound elsewhere
    /// rules out the module beside the file, and nothing here says what to put
    /// in its place. A path nothing binds is `Unbound`: qualified, it may still
    /// resolve on its own; bare, it resolves through a glob or not at all.
    ///
    /// Only bindings written in `region` or around it are asked. A `use` in the
    /// body of one function says nothing about the function beside it, so the
    /// innermost region that binds the name wins and a sibling region is never
    /// consulted.
    fn binding_of<'p>(&'p self, path: &'p str, region: RegionId) -> PathBinding<'p> {
        let (first, rest) = match path.split_once("::") {
            Some((first, rest)) => (first, Some(rest)),
            None => (path, None),
        };
        let last = rest.and_then(|r| r.rsplit("::").next());
        for enclosing in self.regions.enclosing(region) {
            if let Some(target) = self.modules.get(&enclosing).and_then(|m| m.get(first))
                && let Some(rest) = rest
            {
                return PathBinding::Path(Cow::Owned(format!("{target}::{rest}")));
            }
            if let Some(dep) = self.items.get(&enclosing).and_then(|m| m.get(first)) {
                return PathBinding::Item(dep, last);
            }
            if self
                .elsewhere
                .get(&enclosing)
                .is_some_and(|names| names.contains(first))
            {
                return PathBinding::Elsewhere;
            }
        }
        PathBinding::Unbound(first, last)
    }

    /// List the glob imports a name standing in `region` may come from,
    /// innermost first. Empty when the file defines the name itself.
    fn globs_for(&self, name: &str, region: RegionId) -> impl Iterator<Item = &DependencyRef> {
        let defined_here = self.local_definitions.contains(name);
        self.regions
            .enclosing(region)
            .filter(move |_| !defined_here)
            .filter_map(|enclosing| self.globs.get(&enclosing))
            .flatten()
    }

    fn bind_module(&mut self, region: RegionId, name: &str, module_path: String) {
        self.modules
            .entry(region)
            .or_default()
            .insert(name.to_string(), module_path);
    }

    fn bind_item(&mut self, region: RegionId, name: &str, dep: DependencyRef) {
        self.items
            .entry(region)
            .or_default()
            .insert(name.to_string(), dep);
    }

    fn bind_glob(&mut self, region: RegionId, dep: DependencyRef) {
        self.globs.entry(region).or_default().push(dep);
    }

    /// Record the names of the items the file defines, so no glob claims them.
    fn define_locally(&mut self, syntax: &syn::File) {
        struct DefinedNames(HashSet<String>);
        impl<'ast> Visit<'ast> for DefinedNames {
            fn visit_item(&mut self, node: &'ast syn::Item) {
                let ident = match node {
                    syn::Item::Fn(i) => Some(&i.sig.ident),
                    syn::Item::Struct(i) => Some(&i.ident),
                    syn::Item::Enum(i) => Some(&i.ident),
                    syn::Item::Union(i) => Some(&i.ident),
                    syn::Item::Trait(i) => Some(&i.ident),
                    syn::Item::Type(i) => Some(&i.ident),
                    syn::Item::Const(i) => Some(&i.ident),
                    syn::Item::Static(i) => Some(&i.ident),
                    syn::Item::Mod(i) => Some(&i.ident),
                    syn::Item::Macro(i) => i.ident.as_ref(),
                    _ => None,
                };
                if let Some(ident) = ident {
                    self.0.insert(ident.to_string());
                }
                syn::visit::visit_item(self, node);
            }
        }
        let mut names = DefinedNames(HashSet::new());
        names.visit_file(syntax);
        self.local_definitions.extend(names.0);
    }

    fn bind_elsewhere(&mut self, region: RegionId, name: &str) {
        self.elsewhere
            .entry(region)
            .or_default()
            .insert(name.to_string());
    }
}

/// What one `use` binding means for later references to its name.
enum Binding {
    /// Names a module. A later `binding::Item` resolves through this path.
    Module(String),
    /// Names something this crate does not contain.
    Elsewhere,
    /// Names an item of this crate. A qualified use of an item is a reference to
    /// the item and already resolved, so later paths resolve on their own.
    OwnItem,
}

/// Classify one binding. `dep` is what its path resolved to, `None` where the
/// resolution chain could not place it.
///
/// A path the parser cannot place is a path out of this crate: every form that
/// stays inside resolves. Its name is therefore bound elsewhere, and that is what
/// keeps `parse_bare_module_import` off it.
///
/// A binding to a module, or to the root of another crate, is a module binding:
/// what follows the name resolves under that path.
///
/// A binding from another crate to an item needs an entry only where its name
/// also names a module of this crate. Without one, the reference lands on that
/// module. Whether it would is asked of `parse_bare_module_import` rather than
/// restated here, so the two cannot drift apart.
fn classify_binding(
    ctx: &ResolutionContext,
    dep: Option<&DependencyRef>,
    binding: &str,
) -> Binding {
    let Some(dep) = dep else {
        return Binding::Elsewhere;
    };
    if dep.target_item.is_none() {
        return Binding::Module(absolute_module_path(ctx, dep));
    }
    if dep.target_crate == normalize_crate_name(ctx.current_crate) {
        return Binding::OwnItem;
    }
    if parse_bare_module_import(ctx, binding, 0, &EdgeContext::production()).is_some() {
        return Binding::Module(dep.full_target());
    }
    Binding::Elsewhere
}

/// Render a resolved module dependency as a path that `resolve_single_path` can
/// re-resolve from any position in the file. Another crate's root is its name.
fn absolute_module_path(ctx: &ResolutionContext, dep: &DependencyRef) -> String {
    if dep.target_crate == normalize_crate_name(ctx.current_crate) {
        format!("crate::{}", dep.target_module)
    } else {
        dep.full_target()
    }
}

/// Parse path references into workspace-relevant dependencies, each carrying
/// the use the reference is.
///
/// Takes pre-collected path refs from `collect_all_path_refs()` and asks the
/// file's bindings what each stands for ([`FileBindings::binding_of`]). An
/// occurrence of a bound item is that item's dependency again; any other path
/// resolves through the existing resolution chain (`resolve_single_path()`).
/// The binding is authoritative, so a path it rules out is dropped rather than
/// resolved on its own.
/// Deduplicates by `full_target()` — same strategy as `parse_workspace_dependencies()`.
pub(crate) fn parse_path_ref_dependencies(
    paths: &CollectedPathRefs,
    ctx: &ResolutionContext,
    bindings: &FileBindings,
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashMap<(String, UsageKind), usize> = HashMap::new();
    // The globs a name was taken from, by `(full_target, kind)` of the glob.
    let mut used_globs: HashSet<(String, UsageKind)> = HashSet::new();

    for path_ref in paths.iter() {
        // A path written in macro input resolves through a binding only.
        let resolve = |path: &str| {
            (!path_ref.in_macro)
                .then(|| {
                    resolve_single_path(
                        ctx,
                        path,
                        path_ref.line,
                        &path_ref.context,
                        path_ref.inline_depth,
                    )
                })
                .flatten()
        };
        let (mut dep, associated, glob_taken_from) =
            match bindings.binding_of(&path_ref.path, path_ref.region) {
                PathBinding::Unbound(name, associated) => {
                    // A qualified path resolves on its own where it can; what
                    // does not, and a bare name, is a glob's where a glob
                    // exports it.
                    if let Some(dep) = associated.and_then(|_| resolve(&path_ref.path)) {
                        let associated = associated_segment(&path_ref.path, &dep);
                        (dep, associated, None)
                    } else {
                        let Some(glob) = bindings
                            .globs_for(name, path_ref.region)
                            .find(|glob| glob_exports(ctx, glob, name))
                        else {
                            continue;
                        };
                        let dep = DependencyRef {
                            target_item: Some(name.to_string()),
                            ..glob.clone()
                        };
                        let taken_from = (glob.full_target(), glob.context.kind);
                        (dep, associated, Some(taken_from))
                    }
                }
                // A qualified path may still name a module beside the file: a
                // `use` of a fn `helper` leaves the module `helper` addressable,
                // the two being in different namespaces. The module wins where
                // it resolves.
                PathBinding::Item(bound, associated) => {
                    match associated.and_then(|_| resolve(&path_ref.path)) {
                        Some(dep) => {
                            let associated = associated_segment(&path_ref.path, &dep);
                            (dep, associated, None)
                        }
                        None => (bound.clone(), associated, None),
                    }
                }
                PathBinding::Path(effective) => {
                    let Some(dep) = resolve(&effective) else {
                        continue;
                    };
                    let associated = associated_segment(&path_ref.path, &dep);
                    (dep, associated, None)
                }
                PathBinding::Elsewhere => continue,
            };
        resolve_reexport(&mut dep, ctx.reexport_map, ctx.current_module_path);
        let Some(item) = dep.target_item.as_deref() else {
            DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
            continue;
        };
        if path_ref.position == PathPosition::Binding && !is_matched_by_a_bare_pattern(ctx, &dep) {
            continue;
        }
        if let Some(glob) = glob_taken_from {
            used_globs.insert(glob);
        }
        let symbol_use = SymbolUse {
            line: path_ref.line,
            category: categorize(ctx, &dep, item, path_ref.position, associated),
            imported_for_methods: false,
        };
        dep.uses.push(symbol_use);
        DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
    }

    // A glob the file takes no name from keeps its `*`: the import is there,
    // and the edge with it. A re-export glob already carries its payload.
    for glob in bindings.globs.values().flatten() {
        if !glob.via_reexport && !used_globs.contains(&(glob.full_target(), glob.context.kind)) {
            DependencyRef::dedup_push(&mut deps, &mut seen_targets, glob.clone());
        }
    }

    deps
}

/// Whether the module a glob imports from brings `name` into this file's
/// scope: by its own definition, a named re-export, a glob re-export chain,
/// or a private glob the file's module sees as a descendant.
fn glob_exports(ctx: &ResolutionContext, glob: &DependencyRef, name: &str) -> bool {
    ctx.reexport_map
        .get(&glob.target_crate)
        .is_some_and(|crate_exports| {
            module_scope_holds_symbol(
                crate_exports,
                &glob.target_module,
                name,
                ctx.current_module_path,
                &mut HashSet::new(),
            )
        })
}

/// Take the segment a path writes after the item it resolved to:
/// `crate::a::S::new` on a dependency on `S` leaves `new`. A path ending in the
/// item leaves nothing.
fn associated_segment<'p>(path: &'p str, dep: &DependencyRef) -> Option<&'p str> {
    let last = path.rsplit("::").next()?;
    (Some(last) != dep.target_item.as_deref()).then_some(last)
}

fn definition_kind(ctx: &ResolutionContext, dep: &DependencyRef, item: &str) -> Option<DefKind> {
    ctx.reexport_map
        .module_info(dep)
        .and_then(|info| info.definitions.get(item))
        .map(|definition| definition.kind)
}

/// Whether a bare identifier pattern matches `dep`'s item rather than
/// declaring a variable of that name: only a const, a static or a unit struct
/// can be matched that way. An item the map does not know is taken for a
/// variable, since a variable is what such a pattern nearly always declares.
fn is_matched_by_a_bare_pattern(ctx: &ResolutionContext, dep: &DependencyRef) -> bool {
    let Some(item) = &dep.target_item else {
        return false;
    };
    matches!(
        definition_kind(ctx, dep, item),
        Some(DefKind::Const | DefKind::Static | DefKind::Struct)
    )
}

/// Categorize one use by what its position takes from the item and, where the
/// position leaves that open, by what the item is.
fn categorize(
    ctx: &ResolutionContext,
    dep: &DependencyRef,
    item: &str,
    position: PathPosition,
    associated: Option<&str>,
) -> UseCategory {
    let kind = definition_kind(ctx, dep, item);
    let items = ctx
        .reexport_map
        .module_info(dep)
        .and_then(|info| info.associated.get(item));
    match (associated, position) {
        (_, PathPosition::Type | PathPosition::Construct) => UseCategory::Types,
        (Some(name), _) if items.is_some_and(|items| items.variants.contains(name)) => {
            UseCategory::Types
        }
        (Some(name), _) if items.is_some_and(|items| items.fns.contains(name)) => UseCategory::Fns,
        (Some(name), _) if items.is_some_and(|items| items.consts.contains(name)) => {
            UseCategory::Values
        }
        (Some(name), PathPosition::Call) => category_of_unknown_call(name),
        (Some(name), PathPosition::Value | PathPosition::Binding) => category_by_convention(name),
        (None, PathPosition::Call) => match kind {
            Some(DefKind::Struct | DefKind::Enum) => UseCategory::Types,
            Some(_) => UseCategory::Fns,
            None => category_of_unknown_call(item),
        },
        (None, PathPosition::Value | PathPosition::Binding) => match kind {
            Some(DefKind::Const | DefKind::Static) => UseCategory::Values,
            Some(DefKind::Fn) => UseCategory::Fns,
            Some(DefKind::Struct | DefKind::Enum | DefKind::Type | DefKind::Trait) => {
                UseCategory::Types
            }
            None => category_by_convention(item),
        },
    }
}

/// Categorize a name by its spelling, where no definition says what it is:
/// Rust names constants in upper case, types in camel case and functions in
/// snake case.
fn category_by_convention(name: &str) -> UseCategory {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {
            if chars.all(|c| !c.is_lowercase()) {
                UseCategory::Values
            } else {
                UseCategory::Types
            }
        }
        _ => UseCategory::Fns,
    }
}

/// Categorize a call of a name no definition explains: capitalised, it
/// constructs a tuple struct or a variant; otherwise it calls a fn.
fn category_of_unknown_call(name: &str) -> UseCategory {
    if name.starts_with(char::is_uppercase) {
        UseCategory::Types
    } else {
        UseCategory::Fns
    }
}

/// Parse every dependency one file writes, with the uses each carries.
///
/// The `use` items name the imports; the path references say where and how
/// each imported name, and each qualified path, is used. Both are resolved
/// against the same bindings and deduplicated by `(full_target, kind)`, the
/// `use` items first so a location stays at the import line.
pub(crate) fn parse_file_dependencies(
    syntax: &syn::File,
    ctx: &ResolutionContext,
    base_context: EdgeContext,
) -> Vec<DependencyRef> {
    let use_items = collect_all_use_items(syntax, base_context.clone());
    let use_deps = parse_workspace_dependencies(&use_items, ctx);

    let path_refs = collect_all_path_refs(syntax, base_context);
    let bindings = FileBindings::of(syntax, &use_items, ctx);
    let path_deps = parse_path_ref_dependencies(&path_refs, ctx, &bindings);

    let mut seen = DependencyRef::build_seen_index(&use_deps);
    let mut deps = use_deps;
    for dep in path_deps {
        DependencyRef::dedup_push(&mut deps, &mut seen, dep);
    }
    for dep in &mut deps {
        if dep.uses.is_empty()
            && !dep.via_reexport
            && let Some(item) = dep.target_item.as_deref()
            && item != "*"
        {
            let symbol_use = import_only_use(ctx, dep, item);
            dep.uses.push(symbol_use);
        }
    }
    deps
}

/// Derive the use of an import nothing in the file names. A trait is then
/// there for its methods, which method-call syntax never spells out. Any other
/// item is a dead import, and the use is what the definition says it is. A
/// re-export republishes the name and uses nothing, so it gets no use.
fn import_only_use(ctx: &ResolutionContext, dep: &DependencyRef, item: &str) -> SymbolUse {
    let is_trait = definition_kind(ctx, dep, item) == Some(DefKind::Trait);
    if is_trait {
        SymbolUse {
            line: dep.line,
            category: UseCategory::Fns,
            imported_for_methods: true,
        }
    } else {
        SymbolUse {
            line: dep.line,
            category: categorize(ctx, dep, item, PathPosition::Value, None),
            imported_for_methods: false,
        }
    }
}

/// Convenience wrapper: parse source text and extract dependencies.
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
    parse_file_dependencies(&syntax, ctx, EdgeContext::production())
}

#[cfg(test)]
mod tests;
