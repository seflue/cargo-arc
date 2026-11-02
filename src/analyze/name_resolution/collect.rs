//! What a file's syntax tree writes: its `use` items and the paths its code
//! names, each with the region it is written in.

use super::super::mod_resolver::is_cfg_test;
use super::super::use_parser::Meaning;
use super::place::append_to_path;
use crate::model::EdgeContext;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::sync::Arc;
use syn::Token;
use syn::punctuated::Punctuated;
use syn::visit::Visit;

/// Where a `use` binds: an inline module body, a block, or the file itself.
pub(super) type RegionId = usize;

/// The binding regions of one file, as parent links.
///
/// A `use` is an item and binds in its enclosing module or block only. A later
/// reference therefore sees a binding when the binding's region is the
/// reference's own region or one enclosing it. A block sees the region
/// around it. An inline module sees the module around it only where it
/// writes `use super::*`, which brings every name in scope there: the file or
/// the next inline module out, past the blocks in between. An item a region
/// declares hides every name of the same spelling from around it.
///
/// Both collectors number the regions of the same file, and a reference from one
/// walk is looked up against bindings from the other. The two numberings have to
/// agree, which is why both take them from `impl_binding_region_visits!` and
/// neither prunes its traversal.
#[derive(Debug, Clone)]
pub(super) struct BindingRegions {
    parents: Vec<Option<RegionId>>,
    /// The regions of inline modules that do not see the region around them.
    closed: HashSet<RegionId>,
    /// For each region, the module it belongs to: itself for the file and an
    /// inline module, the module around it for a block.
    modules: Vec<RegionId>,
    /// The names of the items each region declares. Only the `use` collector
    /// records them.
    declared: HashMap<RegionId, HashSet<String>>,
}

impl BindingRegions {
    /// The file itself. Encloses every other region and is enclosed by none.
    const ROOT: RegionId = 0;

    fn open(&mut self, parent: RegionId) -> RegionId {
        self.parents.push(Some(parent));
        self.modules.push(self.modules[parent]);
        self.parents.len() - 1
    }

    /// A region an inline module opens, closed to the region around it
    /// until [`BindingRegions::see_around`] opens it.
    fn open_module(&mut self, parent: RegionId) -> RegionId {
        let region = self.open(parent);
        self.modules[region] = region;
        self.closed.insert(region);
        region
    }

    /// Let the inline module of `region` see the module around it, as its
    /// `use super::*` does. A block sees the region around it already.
    pub(super) fn see_around(&mut self, region: RegionId) {
        self.closed.remove(&region);
        if let Some(parent) = self.parents[region] {
            self.parents[region] = Some(self.modules[parent]);
        }
    }

    fn declare(&mut self, region: RegionId, name: String) {
        self.declared.entry(region).or_default().insert(name);
    }

    /// Whether `region` declares an item named `name`.
    pub(super) fn declares(&self, region: RegionId, name: &str) -> bool {
        self.declared
            .get(&region)
            .is_some_and(|names| names.contains(name))
    }

    #[cfg(test)]
    fn count(&self) -> usize {
        self.parents.len()
    }

    /// A region and everything enclosing it, innermost first.
    pub(super) fn enclosing(&self, region: RegionId) -> impl Iterator<Item = RegionId> + '_ {
        std::iter::successors(Some(region), move |&inner| {
            if self.closed.contains(&inner) {
                return None;
            }
            self.parents.get(inner).copied().flatten()
        })
    }
}

impl Default for BindingRegions {
    fn default() -> Self {
        Self {
            parents: vec![None],
            closed: HashSet::new(),
            modules: vec![Self::ROOT],
            declared: HashMap::new(),
        }
    }
}

/// Whether a fn carries `#[test]`. The compiler builds such a fn only into the
/// test harness, with or without `#[cfg(test)]` around it.
fn is_test_fn(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("test"))
}

/// The outer attributes of an item; none for tokens syn could not parse.
pub(super) fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::ExternCrate(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::ForeignMod(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Macro(i) => &i.attrs,
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        syn::Item::TraitAlias(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Union(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

/// The context inside an item: a test context when the item has
/// `#[cfg(test)]`, the surrounding context otherwise.
fn item_context(item: &syn::Item, outer: &EdgeContext) -> EdgeContext {
    if is_cfg_test(item_attrs(item)) {
        outer.promoted_to_test()
    } else {
        outer.clone()
    }
}

/// Shared `visit_item_mod`, `visit_item_fn` and `visit_block` for both collectors.
///
/// `visit_item_mod` and `visit_block` open a binding region. `visit_item_mod`
/// also carries the path of the inline module, which counts modules only:
/// `use super::X` in a function body still means the parent module.
/// `visit_item_fn` carries the test context of a fn with `#[test]`; each
/// collector's `visit_item` carries the one of any item with `#[cfg(test)]`.
macro_rules! impl_binding_region_visits {
    () => {
        fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
            let prev_inline_path = Arc::clone(&self.inline_path);
            let prev_region = self.region;
            // Inline modules (with body) extend the path; `mod foo;` (external) does not
            if node.content.is_some() {
                self.inline_path =
                    append_to_path(&prev_inline_path, &node.ident.to_string()).into();
                self.region = self.regions.open_module(prev_region);
            }
            syn::visit::visit_item_mod(self, node);
            self.inline_path = prev_inline_path;
            self.region = prev_region;
        }

        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            let prev_context = self.context.clone();
            if is_test_fn(&node.attrs) {
                self.context = self.context.promoted_to_test();
            }
            syn::visit::visit_item_fn(self, node);
            self.context = prev_context;
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
#[derive(Clone)]
pub(super) struct CollectedUse {
    pub(super) item: syn::ItemUse,
    pub(super) context: EdgeContext,
    /// The inline modules around the item, as a path below the file's
    /// module: `a::b` inside `mod a { mod b { … } }`, empty outside any.
    pub(super) inline_path: Arc<str>,
    pub(super) region: RegionId,
}

/// The `use` items of one file, with the regions they were written in.
#[derive(Default)]
pub(super) struct CollectedUses {
    items: Vec<CollectedUse>,
    pub(super) regions: BindingRegions,
}

impl Deref for CollectedUses {
    type Target = [CollectedUse];

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl CollectedUses {
    /// The `use` items whose context `keep` accepts, in the same regions.
    pub(super) fn retaining(&self, keep: impl Fn(&EdgeContext) -> bool) -> Self {
        Self {
            items: self
                .items
                .iter()
                .filter(|collected| keep(&collected.context))
                .cloned()
                .collect(),
            regions: self.regions.clone(),
        }
    }
}

/// Collect all `use` items from a parsed file, including those nested inside
/// function bodies, blocks, and other scopes. Uses `syn::visit::Visit` to
/// traverse the full AST regardless of nesting depth.
///
/// Uses inside `#[cfg(test)]` scopes or with `#[cfg(test)]` on the item itself
/// are tagged `Test(Unit)`, all others are `Production`.
pub(super) fn collect_all_use_items(
    syntax: &syn::File,
    base_context: EdgeContext,
) -> CollectedUses {
    struct UseCollector {
        uses: Vec<CollectedUse>,
        context: EdgeContext,
        inline_path: Arc<str>,
        regions: BindingRegions,
        region: RegionId,
    }
    impl<'ast> Visit<'ast> for UseCollector {
        fn visit_item(&mut self, node: &'ast syn::Item) {
            let prev_context = self.context.clone();
            self.context = item_context(node, &prev_context);
            if let Some(ident) = item_ident(node) {
                self.regions.declare(self.region, ident.to_string());
            }
            syn::visit::visit_item(self, node);
            self.context = prev_context;
        }

        fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
            if !attributes_expand(&node.attrs) {
                syn::visit::visit_item_foreign_mod(self, node);
            }
        }

        fn visit_foreign_item(&mut self, node: &'ast syn::ForeignItem) {
            if let Some(ident) = foreign_item_ident(node) {
                self.regions.declare(self.region, ident.to_string());
            }
        }

        fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
            self.uses.push(CollectedUse {
                item: node.clone(),
                context: self.context.clone(),
                inline_path: Arc::clone(&self.inline_path),
                region: self.region,
            });
        }

        fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
            let prev_context = self.context.clone();
            if is_cfg_test(&node.attrs) {
                self.context = self.context.promoted_to_test();
            }
            syn::visit::visit_impl_item_fn(self, node);
            self.context = prev_context;
        }

        impl_binding_region_visits!();
    }
    let mut collector = UseCollector {
        uses: Vec::new(),
        context: base_context,
        inline_path: Arc::from(""),
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
pub(super) enum PathPosition {
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

impl PathPosition {
    /// The meaning this position asks of the last segment of its path.
    fn meaning(self) -> Meaning {
        match self {
            PathPosition::Type | PathPosition::Construct => Meaning::Type,
            PathPosition::Call | PathPosition::Value | PathPosition::Binding => Meaning::Value,
        }
    }
}

/// One path reference with what its position says about it.
#[derive(Debug, Clone)]
pub(super) struct PathRef {
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) context: EdgeContext,
    /// As [`CollectedUse::inline_path`].
    pub(super) inline_path: Arc<str>,
    pub(super) region: RegionId,
    pub(super) position: PathPosition,
    /// What the position asks the last segment to mean: the namespace of its
    /// position, the macro namespace for the name of a macro or a derive, any
    /// for another path read off tokens.
    pub(super) meaning: Meaning,
    /// Written in macro input or an attribute, where syn does not parse and
    /// the segments are read off the tokens. A binding resolves such a path;
    /// the resolution chain is not asked, so no edge comes from macro input
    /// alone.
    pub(super) in_macro: bool,
    /// The local variable a bare name in an expression may refer to: the
    /// innermost one of that name in scope where the path is written.
    pub(super) local: Option<LocalBinding>,
    /// Written in an attribute whose path starts with a registered tool
    /// ([`TOOL_NAMES`]), such as `#[rustfmt::skip]`.
    pub(super) in_tool_attribute: bool,
}

/// The tools whose name starts an attribute path rustc reads as the tool's,
/// unless a crate of the extern prelude has the name.
const TOOL_NAMES: [&str; 5] = ["clippy", "diagnostic", "miri", "rust_analyzer", "rustfmt"];

/// The attributes rustc builds in that may stand on an item and expand to no
/// item, from the Rust Reference's index of built-in attributes.
const BUILT_IN_ATTRIBUTES: [&str; 39] = [
    "allow",
    "automatically_derived",
    "cold",
    "collapse_debuginfo",
    "coverage",
    "debugger_visualizer",
    "deny",
    "deprecated",
    "doc",
    "expect",
    "export_name",
    "forbid",
    "global_allocator",
    "ignore",
    "inline",
    "instruction_set",
    "link",
    "link_name",
    "link_ordinal",
    "link_section",
    "macro_export",
    "macro_use",
    "must_use",
    "naked",
    "no_link",
    "no_mangle",
    "non_exhaustive",
    "panic_handler",
    "path",
    "proc_macro",
    "proc_macro_attribute",
    "proc_macro_derive",
    "repr",
    "should_panic",
    "target_feature",
    "test",
    "track_caller",
    "used",
    "warn",
];

/// The derives rustc builds in, which implement a trait and define no name.
const BUILT_IN_DERIVES: [&str; 9] = [
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "Eq",
    "Hash",
    "Ord",
    "PartialEq",
    "PartialOrd",
];

/// Whether one of `attrs` is an attribute or derive macro rustc does not
/// build in, inside a `cfg_attr` too.
pub(super) fn attributes_expand(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| meta_expands(&attr.meta))
}

/// Whether the attribute `meta` is a macro rustc does not build in, or
/// derives one.
fn meta_expands(meta: &syn::Meta) -> bool {
    let path = meta.path();
    let first = path
        .segments
        .first()
        .map(|segment| segment.ident.to_string());
    if first
        .as_deref()
        .is_some_and(|tool| TOOL_NAMES.contains(&tool))
    {
        return false;
    }
    let Some(name) = path.get_ident().map(ToString::to_string) else {
        return true;
    };
    let nested = || {
        meta.require_list().ok().and_then(|list| {
            list.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
                .ok()
        })
    };
    match name.as_str() {
        "cfg" => false,
        // `cfg_attr(predicate, attributes…)`: the predicate is no macro.
        "cfg_attr" => nested().is_none_or(|metas| metas.iter().skip(1).any(meta_expands)),
        "derive" => {
            nested().is_none_or(|metas| metas.iter().any(|meta| !is_built_in_derive(meta.path())))
        }
        "unsafe" => nested().is_none_or(|metas| metas.iter().any(meta_expands)),
        _ => !BUILT_IN_ATTRIBUTES.contains(&name.as_str()),
    }
}

/// Whether the attribute `meta` derives a macro rustc does not build in,
/// inside a `cfg_attr` too: a derive macro may declare helper attributes.
fn derives_a_macro(meta: &syn::Meta) -> bool {
    let nested = || {
        meta.require_list().ok().and_then(|list| {
            list.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
                .ok()
        })
    };
    if meta.path().is_ident("derive") {
        meta_expands(meta)
    } else if meta.path().is_ident("cfg_attr") {
        nested().is_none_or(|metas| metas.iter().skip(1).any(derives_a_macro))
    } else {
        false
    }
}

/// Whether the derive `path` names is one rustc builds in, written bare or
/// from `std` or `core`.
fn is_built_in_derive(path: &syn::Path) -> bool {
    let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let from_std = match segments.as_slice() {
        [_] => true,
        [first, ..] => first == "std" || first == "core",
        [] => false,
    };
    from_std
        && segments
            .last()
            .is_some_and(|name| BUILT_IN_DERIVES.contains(&name.as_str()))
}

impl PathRef {
    /// The meaning the path asks of the item it resolved to, with `associated`
    /// the segment it writes after the item: an item the path goes on past is
    /// a type.
    pub(super) fn meaning_of_item(&self, associated: Option<&str>) -> Meaning {
        if associated.is_some() {
            Meaning::Type
        } else {
            self.meaning
        }
    }
}

/// The pattern that declares a local variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalBinding {
    /// `mut x`, `ref x` or `x @ ..`: a variable for certain.
    Variable,
    /// A bare identifier, the path reference at this index. It declares a
    /// variable unless it matches an item (see [`PathPosition::Binding`]).
    Pattern(usize),
}

/// A local variable while the walk is inside its scope.
#[derive(Debug)]
struct ScopedLocal {
    name: String,
    binding: LocalBinding,
    /// The block or module the variable was declared in. Leaving it ends the
    /// variable's scope.
    region: RegionId,
}

/// The qualified path references of one file, with the regions they stand in.
#[derive(Debug, Default)]
pub(super) struct CollectedPathRefs {
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

impl CollectedPathRefs {
    /// The references whose context `keep` accepts.
    pub(super) fn retaining(&self, keep: impl Fn(&EdgeContext) -> bool) -> Self {
        Self {
            refs: self
                .refs
                .iter()
                .filter(|path_ref| keep(&path_ref.context))
                .cloned()
                .collect(),
            #[cfg(test)]
            regions: self.regions.clone(),
        }
    }
}

/// The walker behind [`collect_all_path_refs`].
struct PathRefCollector {
    paths: Vec<PathRef>,
    context: EdgeContext,
    inline_path: Arc<str>,
    regions: BindingRegions,
    region: RegionId,
    position: PathPosition,
    /// The local variables in scope, innermost last.
    locals: Vec<ScopedLocal>,
    /// The variables a pattern declares that come into scope only after the
    /// expression the pattern stands before: `let x = <expr>`,
    /// `if let x = <expr>`, `for x in <expr>`.
    declared_later: Option<Vec<ScopedLocal>>,
    /// Inside the name of a macro, which no variable can hide.
    naming_macro: bool,
    /// Inside an attribute whose path starts with a tool name.
    in_tool_attribute: bool,
    /// Inside an item that derives a macro rustc does not build in, whose
    /// helper attributes a bare attribute path may name.
    in_derive_helpers_reach: bool,
}
impl PathRefCollector {
    /// Visit an item that may carry derives with `attrs`, its fields and
    /// variants by `visit`, as [`PathRefCollector::in_derive_helpers_reach`]
    /// tells.
    fn under_derives(&mut self, attrs: &[syn::Attribute], visit: impl FnOnce(&mut Self)) {
        let prev = self.in_derive_helpers_reach;
        self.in_derive_helpers_reach = attrs.iter().any(|attr| derives_a_macro(&attr.meta));
        visit(self);
        self.in_derive_helpers_reach = prev;
    }

    /// Record the paths in the tokens of the attribute list `node`: a name in
    /// `#[derive(…)]` is a derive macro.
    fn record_meta_list_tokens(&mut self, node: &syn::MetaList) {
        let meaning = if node.path.is_ident("derive") {
            Meaning::Macro
        } else {
            Meaning::Any
        };
        self.record_token_paths(&node.tokens, meaning);
    }

    fn in_position(&mut self, position: PathPosition, visit: impl FnOnce(&mut Self)) {
        let prev = self.position;
        self.position = position;
        visit(self);
        self.position = prev;
    }

    /// Visit a scope that ends with the visit: a fn, a closure, a match arm.
    fn in_scope(&mut self, visit: impl FnOnce(&mut Self)) {
        let outer = self.locals.len();
        visit(self);
        self.locals.truncate(outer);
    }

    /// Visit a pattern and return the variables it declares instead of
    /// bringing them into scope.
    fn declarations_of(&mut self, pattern: impl FnOnce(&mut Self)) -> Vec<ScopedLocal> {
        let outer = self.declared_later.replace(Vec::new());
        pattern(self);
        std::mem::replace(&mut self.declared_later, outer).unwrap_or_default()
    }

    fn declare(&mut self, name: String, binding: LocalBinding) {
        let local = ScopedLocal {
            name,
            binding,
            region: self.region,
        };
        match &mut self.declared_later {
            Some(later) => later.push(local),
            None => self.locals.push(local),
        }
    }

    /// The variable a bare name in an expression refers to. Types, struct
    /// paths and macros live in other namespaces than variables.
    fn local_named(&self, path: &str) -> Option<LocalBinding> {
        if self.naming_macro
            || path.contains("::")
            || !matches!(self.position, PathPosition::Value | PathPosition::Call)
        {
            return None;
        }
        self.locals
            .iter()
            .rev()
            .find(|local| {
                local.name == path
                    && self
                        .regions
                        .enclosing(self.region)
                        .any(|r| r == local.region)
            })
            .map(|local| local.binding)
    }

    fn record(&mut self, path: String, line: usize, in_macro: bool) {
        let meaning = if self.naming_macro {
            Meaning::Macro
        } else {
            self.position.meaning()
        };
        self.record_meaning(path, line, in_macro, meaning);
    }

    fn record_meaning(&mut self, path: String, line: usize, in_macro: bool, meaning: Meaning) {
        let local = self.local_named(&path);
        self.paths.push(PathRef {
            path,
            line,
            context: self.context.clone(),
            inline_path: Arc::clone(&self.inline_path),
            region: self.region,
            position: self.position,
            meaning,
            in_macro,
            local,
            in_tool_attribute: self.in_tool_attribute,
        });
    }

    /// Read the paths off a token stream syn leaves unparsed: a run of
    /// identifiers joined by `::`, at any nesting of brackets. Every other
    /// token is skipped, so nothing here knows what position a path is in;
    /// `meaning` is what each path is asked to mean, any where nothing says.
    fn record_token_paths(&mut self, tokens: &proc_macro2::TokenStream, meaning: Meaning) {
        use proc_macro2::TokenTree;
        let tokens: Vec<TokenTree> = tokens.clone().into_iter().collect();
        let mut index = 0;
        while index < tokens.len() {
            match &tokens[index] {
                TokenTree::Group(group) => self.record_token_paths(&group.stream(), meaning),
                // A lifetime is a `'` followed by an identifier, and a field
                // or method follows a `.`; neither is a name.
                TokenTree::Ident(ident)
                    if !is_lifetime_tick(tokens.get(index.wrapping_sub(1)))
                        && !follows_a_field_dot(&tokens[..index]) =>
                {
                    let line = ident.span().start().line;
                    let mut path = vec![ident.to_string()];
                    while is_path_separator(&tokens[index + 1..])
                        && let Some(TokenTree::Ident(next)) = tokens.get(index + 3)
                    {
                        path.push(next.to_string());
                        index += 3;
                    }
                    self.in_position(PathPosition::Value, |s| {
                        s.record_meaning(path.join("::"), line, true, meaning);
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

/// Whether the tokens before an identifier end with the `.` of a field or
/// method access. Two dots are a range, and a path may follow them.
fn follows_a_field_dot(before: &[proc_macro2::TokenTree]) -> bool {
    let is_dot = |token: &proc_macro2::TokenTree| matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '.');
    match before {
        [.., previous, dot] => is_dot(dot) && !is_dot(previous),
        [dot] => is_dot(dot),
        [] => false,
    }
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
        let binding = if node.by_ref.is_none() && node.mutability.is_none() && node.subpat.is_none()
        {
            self.in_position(PathPosition::Binding, |s| {
                s.record(
                    node.ident.to_string(),
                    node.ident.span().start().line,
                    false,
                );
            });
            LocalBinding::Pattern(self.paths.len() - 1)
        } else {
            LocalBinding::Variable
        };
        self.declare(node.ident.to_string(), binding);
        syn::visit::visit_pat_ident(self, node);
    }

    // An item nested in a fn body sees none of the fn's variables.
    fn visit_item(&mut self, node: &'ast syn::Item) {
        let outer = std::mem::take(&mut self.locals);
        let prev_context = self.context.clone();
        self.context = item_context(node, &prev_context);
        syn::visit::visit_item(self, node);
        self.context = prev_context;
        self.locals = outer;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let prev_context = self.context.clone();
        if is_cfg_test(&node.attrs) {
            self.context = self.context.promoted_to_test();
        }
        self.in_scope(|s| syn::visit::visit_impl_item_fn(s, node));
        self.context = prev_context;
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.in_scope(|s| syn::visit::visit_trait_item_fn(s, node));
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        self.in_scope(|s| syn::visit::visit_expr_closure(s, node));
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        self.in_scope(|s| syn::visit::visit_arm(s, node));
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        let declared = self.declarations_of(|s| s.visit_pat(&node.pat));
        if let Some(init) = &node.init {
            self.visit_local_init(init);
        }
        self.locals.extend(declared);
    }

    // The variables of an `if let` are in scope in the condition after it and
    // in the `then` block, not in the `else` branch.
    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        self.in_scope(|s| {
            s.visit_expr(&node.cond);
            s.visit_block(&node.then_branch);
        });
        if let Some((_, else_branch)) = &node.else_branch {
            self.visit_expr(else_branch);
        }
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.in_scope(|s| syn::visit::visit_expr_while(s, node));
    }

    fn visit_expr_let(&mut self, node: &'ast syn::ExprLet) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        let declared = self.declarations_of(|s| s.visit_pat(&node.pat));
        self.visit_expr(&node.expr);
        self.locals.extend(declared);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        if let Some(label) = &node.label {
            self.visit_label(label);
        }
        let declared = self.declarations_of(|s| s.visit_pat(&node.pat));
        self.visit_expr(&node.expr);
        self.in_scope(|s| {
            s.locals.extend(declared);
            s.visit_block(&node.body);
        });
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
        self.naming_macro = true;
        self.in_position(PathPosition::Call, |s| syn::visit::visit_macro(s, node));
        self.naming_macro = false;
        self.record_token_paths(&node.tokens, Meaning::Any);
    }

    // `#[derive(Serialize)]` and the like name what they use in tokens too.
    // A name in `#[derive(…)]` is a derive macro.
    fn visit_meta_list(&mut self, node: &'ast syn::MetaList) {
        syn::visit::visit_meta_list(self, node);
        self.record_meta_list_tokens(node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        self.under_derives(&node.attrs, |s| syn::visit::visit_item_struct(s, node));
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        self.under_derives(&node.attrs, |s| syn::visit::visit_item_enum(s, node));
    }

    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        self.under_derives(&node.attrs, |s| syn::visit::visit_item_union(s, node));
    }

    // `pub(in path)` names the scope that may see the item; nothing is used
    // from that module, so its path is no dependency.
    fn visit_vis_restricted(&mut self, _node: &'ast syn::VisRestricted) {}

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        let segments = &node.path().segments;
        self.in_tool_attribute = segments.len() > 1
            && segments
                .first()
                .is_some_and(|tool| TOOL_NAMES.iter().any(|name| tool.ident == name));
        // The attribute's own path names a macro; the value of `name = value`
        // is an expression. A bare path where a derive macro may declare
        // helper attributes may name a helper, which is no macro.
        let helper = self.in_derive_helpers_reach && segments.len() == 1;
        match &node.meta {
            syn::Meta::NameValue(name_value) => {
                if !helper {
                    self.naming_macro = true;
                    self.visit_path(&name_value.path);
                    self.naming_macro = false;
                }
                self.visit_expr(&name_value.value);
            }
            syn::Meta::List(list) if helper => self.record_meta_list_tokens(list),
            syn::Meta::Path(_) if helper => {}
            meta => {
                self.naming_macro = true;
                self.visit_meta(meta);
                self.naming_macro = false;
            }
        }
        self.in_tool_attribute = false;
    }

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
pub(super) fn collect_all_path_refs(
    syntax: &syn::File,
    base_context: EdgeContext,
) -> CollectedPathRefs {
    let mut collector = PathRefCollector {
        paths: Vec::new(),
        context: base_context,
        inline_path: Arc::from(""),
        regions: BindingRegions::default(),
        region: BindingRegions::ROOT,
        position: PathPosition::Value,
        locals: Vec::new(),
        declared_later: None,
        naming_macro: false,
        in_tool_attribute: false,
        in_derive_helpers_reach: false,
    };
    collector.visit_file(syntax);
    CollectedPathRefs {
        refs: collector.paths,
        #[cfg(test)]
        regions: collector.regions,
    }
}

/// The name an item declares; none for `use`, `impl` and the like.
pub(super) fn item_ident(item: &syn::Item) -> Option<&syn::Ident> {
    match item {
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
    }
}

/// The name an item of an `extern` block declares in the module holding the
/// block; none for a macro call and for an item under an attribute macro,
/// which may remove or rewrite it.
pub(super) fn foreign_item_ident(item: &syn::ForeignItem) -> Option<&syn::Ident> {
    let (attrs, ident) = match item {
        syn::ForeignItem::Fn(i) => (&i.attrs, &i.sig.ident),
        syn::ForeignItem::Static(i) => (&i.attrs, &i.ident),
        syn::ForeignItem::Type(i) => (&i.attrs, &i.ident),
        _ => return None,
    };
    (!attributes_expand(attrs)).then_some(ident)
}

#[cfg(test)]
mod tests;
