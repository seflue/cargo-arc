//! The Lua language adapter: a Neovim plugin's `lua/` tree, parsed with
//! `full_moon` as Lua 5.1.

use anyhow::{Context, Result};
use full_moon::LuaVersion;
use full_moon::ast::{Call, Expression, FunctionArgs, FunctionCall, Prefix, Suffix, VarExpression};
use full_moon::tokenizer::TokenType;
use full_moon::visitors::Visitor;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::LanguageAdapter;
use crate::model::{
    Analysis, DependencyRef, EdgeContext, EntryFile, LanguageProfile, ModuleInfo, ModuleTree,
    PackageInfo, Wording,
};

pub(crate) static PROFILE: LanguageProfile = LanguageProfile {
    wording: Wording {
        package: "namespace",
        packages: "namespaces",
        manifest: "rockspec",
    },
    extensions: &["lua"],
    manifest_files: &[],
    tokei_language: tokei::LanguageType::Lua,
};

/// A Neovim plugin: every top-level namespace `lua/<name>/` of the plugin
/// directory is a package.
pub(crate) struct Lua {
    pub plugin_dir: PathBuf,
}

impl LanguageAdapter for Lua {
    fn profile(&self) -> &'static LanguageProfile {
        &PROFILE
    }

    fn analyze(&self) -> Result<Analysis> {
        let root = dunce::canonicalize(&self.plugin_dir)
            .with_context(|| format!("cannot read {}", self.plugin_dir.display()))?;
        let lua_dir = root.join("lua");
        if !lua_dir.is_dir() {
            anyhow::bail!("cannot read {}", lua_dir.display());
        }

        let mut packages = Vec::new();
        let mut modules = Vec::new();
        for root_module in child_modules(&lua_dir, None) {
            let key = root_module.name.clone();
            let dir = lua_dir.join(&key);
            packages.push(PackageInfo {
                name: key.clone(),
                key,
                path: if dir.is_dir() {
                    dir
                } else {
                    lua_dir.join(format!("{}.lua", root_module.name))
                },
                workspace_root: root.clone(),
                entry_files: root_module.file.iter().map(EntryFile::lib).collect(),
                manifest: None,
                dependencies: Vec::new(),
                dev_dependencies: Vec::new(),
            });
            modules.push(ModuleTree {
                root: root_module,
                unparsed: Vec::new(),
            });
        }

        let known: HashSet<String> = modules
            .iter()
            .flat_map(|tree| loadable_paths(&tree.root))
            .collect();
        let mut tally = RequireTally::default();
        for tree in &mut modules {
            add_dependencies(
                &mut tree.root,
                &known,
                &root,
                &mut tree.unparsed,
                &mut tally,
            );
        }
        add_entry_imports(&root, &mut packages, &mut modules, &known, &mut tally);
        for (tree, package) in modules.iter().zip(packages.iter_mut()) {
            package.dependencies = package_dependencies(&tree.root, &package.key);
        }
        tracing::debug!(
            "discarded {} of {} require calls: no module of the plugin",
            tally.discarded,
            tally.read
        );

        Ok(Analysis {
            packages,
            modules,
            externals: None,
        })
    }
}

/// Build the module `full_path` from its `file`, and below it the modules of
/// the Lua files in `dir`.
fn module(name: &str, full_path: String, file: Option<PathBuf>, dir: Option<&Path>) -> ModuleInfo {
    let children = dir.map_or_else(Vec::new, |dir| child_modules(dir, Some(&full_path)));
    ModuleInfo {
        name: name.to_string(),
        full_path,
        file,
        children,
        dependencies: Vec::new(),
        definitions: HashMap::new(),
    }
}

/// The directories that hold a plugin's test code, never its modules.
const TEST_DIRS: [&str; 2] = ["spec", "tests"];

/// List one module per name in `dir`, as `require` looks it up: `<name>.lua`
/// before `<name>/init.lua`. A directory `<name>/` holds the module's
/// children; `dir`'s own `init.lua` belongs to the parent. Without a
/// `parent_path`, `dir` is `lua/` and each module is a namespace's root.
/// Test code is no module: directories `spec/` and `tests/`, files
/// `*_spec.lua`.
fn child_modules(dir: &Path, parent_path: Option<&str>) -> Vec<ModuleInfo> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let names: BTreeSet<String> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_dir() {
                let name = path.file_name()?.to_str()?;
                return (!TEST_DIRS.contains(&name)).then(|| name.to_string());
            }
            let stem = path.file_stem()?.to_str()?;
            (PROFILE.is_source(&path) && stem != "init" && !stem.ends_with("_spec"))
                .then(|| stem.to_string())
        })
        .collect();

    names
        .into_iter()
        .filter_map(|name| {
            let sub = dir.join(&name);
            let file = [dir.join(format!("{name}.lua")), sub.join("init.lua")]
                .into_iter()
                .find(|candidate| candidate.is_file());
            // A module `tests` from `tests.lua` takes no children from `tests/`.
            let sub = (sub.is_dir() && !TEST_DIRS.contains(&name.as_str())).then_some(sub);
            let full_path = match parent_path {
                Some(parent_path) => format!("{parent_path}::{name}"),
                None => name.clone(),
            };
            let child = module(&name, full_path, file, sub.as_deref());
            (child.file.is_some() || !child.children.is_empty()).then_some(child)
        })
        .collect()
}

/// Return the paths of `module` and its descendants that `require` can load:
/// those with a file. A directory without `init.lua` loads nothing.
fn loadable_paths(module: &ModuleInfo) -> Vec<String> {
    module
        .file
        .is_some()
        .then(|| module.full_path.clone())
        .into_iter()
        .chain(module.children.iter().flat_map(loadable_paths))
        .collect()
}

/// How many `require` calls with a string literal the run read, and how many
/// of them named no module of the plugin.
#[derive(Default)]
struct RequireTally {
    read: usize,
    discarded: usize,
}

/// Give `module` and its descendants a dependency for each `require` in
/// their files that names one of the `known` modules, and count the calls in
/// `tally`. A file that does not read or parse goes to `unparsed` instead.
fn add_dependencies(
    module: &mut ModuleInfo,
    known: &HashSet<String>,
    root: &Path,
    unparsed: &mut Vec<PathBuf>,
    tally: &mut RequireTally,
) {
    if let Some(file) = &module.file {
        match required_names(file) {
            Some(requires) => {
                let read = requires.len();
                module.dependencies = requires
                    .into_iter()
                    .filter_map(|(name, line)| dependency(&name, line, file, root, known))
                    .collect();
                tally.read += read;
                tally.discarded += read - module.dependencies.len();
            }
            None => unparsed.push(file.clone()),
        }
    }
    for child in &mut module.children {
        add_dependencies(child, known, root, unparsed, tally);
    }
}

/// Read each `plugin/<x>.lua` of the plugin at `root` as the entry file of
/// the namespace `<x>`, as `main.rs` is a crate's: no module, and its
/// `require` calls become dependencies of the namespace's root module. A
/// script whose `<x>` names no namespace belongs to the plugin's only
/// namespace with a root file (`lua/<n>.lua` or `lua/<n>/init.lua`); with
/// none or several, it has no owner, so its calls are counted as discarded.
fn add_entry_imports(
    root: &Path,
    packages: &mut [PackageInfo],
    modules: &mut [ModuleTree],
    known: &HashSet<String>,
    tally: &mut RequireTally,
) {
    let Ok(entries) = fs::read_dir(root.join("plugin")) else {
        return;
    };
    let mut scripts: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.is_file() && PROFILE.is_source(path))
        .collect();
    scripts.sort();
    let mut rooted = modules
        .iter()
        .enumerate()
        .filter(|(_, tree)| tree.root.file.is_some())
        .map(|(index, _)| index);
    let only_rooted = match (rooted.next(), rooted.next()) {
        (Some(index), None) => Some(index),
        _ => None,
    };
    for script in scripts {
        let owner = script
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| packages.iter().position(|package| package.key == stem))
            .or(only_rooted);
        let Some(requires) = required_names(&script) else {
            if let Some(owner) = owner {
                modules[owner].unparsed.push(script);
            } else {
                tracing::debug!("{} does not parse", script.display());
            }
            continue;
        };
        let read = requires.len();
        tally.read += read;
        let Some(owner) = owner else {
            tracing::debug!(
                "discarded {read} require calls in {}: no namespace of that name, \
                 and not exactly one with a root file",
                script.display()
            );
            tally.discarded += read;
            continue;
        };
        let dependencies: Vec<DependencyRef> = requires
            .into_iter()
            .filter_map(|(name, line)| dependency(&name, line, &script, root, known))
            .collect();
        tally.discarded += read - dependencies.len();
        modules[owner].root.dependencies.extend(dependencies);
        packages[owner].entry_files.push(EntryFile::bin(script));
    }
}

/// Return the dependency a `require(name)` on `line` of `file` stands for,
/// or `None` when `name` is no module of the plugin.
fn dependency(
    name: &str,
    line: usize,
    file: &Path,
    root: &Path,
    known: &HashSet<String>,
) -> Option<DependencyRef> {
    let target = name.replace('.', "::");
    if !known.contains(&target) {
        tracing::debug!("discarded require {name:?} in {}", file.display());
        return None;
    }
    let (package, module) = target.split_once("::").unwrap_or((&target, ""));
    Some(DependencyRef {
        target_package: package.to_string(),
        target_module: module.to_string(),
        target_item: None,
        source_file: file.strip_prefix(root).unwrap_or(file).to_path_buf(),
        line,
        context: EdgeContext::production(),
        via_reexport: false,
        uses: Vec::new(),
    })
}

/// Collect the other packages `module` and its descendants require, sorted.
fn package_dependencies(module: &ModuleInfo, own_key: &str) -> Vec<String> {
    fn collect<'a>(module: &'a ModuleInfo, into: &mut BTreeSet<&'a str>) {
        into.extend(
            module
                .dependencies
                .iter()
                .map(|dep| dep.target_package.as_str()),
        );
        for child in &module.children {
            collect(child, into);
        }
    }
    let mut targets = BTreeSet::new();
    collect(module, &mut targets);
    targets.remove(own_key);
    targets.into_iter().map(str::to_string).collect()
}

/// Return the names `file` passes to `require` as a string literal, each
/// with the line of the call; `None` when the file does not read or parse.
fn required_names(file: &Path) -> Option<Vec<(String, usize)>> {
    let code = fs::read_to_string(file).ok()?;
    let ast = full_moon::parse_fallible(&code, LuaVersion::lua51())
        .into_result()
        .ok()?;
    let mut requires = Requires::default();
    requires.visit_ast(&ast);
    Some(requires.0)
}

#[derive(Default)]
struct Requires(Vec<(String, usize)>);

impl Visitor for Requires {
    fn visit_function_call(&mut self, call: &FunctionCall) {
        self.note(call.prefix(), call.suffixes());
    }

    /// `require("x").field` is no call itself, but starts with one.
    fn visit_var_expression(&mut self, var: &VarExpression) {
        self.note(var.prefix(), var.suffixes());
    }
}

impl Requires {
    fn note<'a>(&mut self, prefix: &Prefix, mut suffixes: impl Iterator<Item = &'a Suffix>) {
        let Prefix::Name(callee) = prefix else {
            return;
        };
        if callee.token().to_string() != "require" {
            return;
        }
        let Some(Suffix::Call(Call::AnonymousCall(args))) = suffixes.next() else {
            return;
        };
        let argument = match args.as_ref() {
            FunctionArgs::Parentheses { arguments, .. } if arguments.len() == 1 => {
                match arguments.iter().next() {
                    Some(Expression::String(token)) => token,
                    _ => return,
                }
            }
            FunctionArgs::String(token) => token,
            _ => return,
        };
        if let TokenType::StringLiteral { literal, .. } = argument.token_type() {
            let line = callee.token().start_position().line();
            self.0.push((literal.as_str().to_string(), line));
        }
    }
}
