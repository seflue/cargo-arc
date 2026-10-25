use std::collections::HashMap;
use std::path::PathBuf;

use tempfile::TempDir;

use super::{FileInModule, NameResolution, PackageResolver, Root};
use crate::analyze::packages::{ModulePathMap, ModulePaths};
use crate::model::{DependencyRef, EdgeContext, PackageInfo, TestKind, UsageKind};
use crate::test_support::conventional_package;

/// The package `p` written to a temporary directory, with the module paths
/// its files declare.
struct Fixture {
    _dir: TempDir,
    package: PackageInfo,
    module_paths: ModulePathMap,
}

impl Fixture {
    /// Write `files` of `p`, which declare the modules `modules`, those in
    /// `inline` with a body inside the file of their parent.
    fn new(files: &[(&str, &str)], modules: &[&str], inline: &[&str]) -> Self {
        Self::named("p", files, modules, inline)
    }

    /// [`Fixture::new`] for the package `name`.
    fn named(name: &str, files: &[(&str, &str)], modules: &[&str], inline: &[&str]) -> Self {
        let dir = TempDir::new().expect("a temporary directory");
        let root = dir.path().join(name);
        for (path, source) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().expect("a file sits in a directory"))
                .expect("the package directory is writable");
            std::fs::write(&file, source).expect("the source file is writable");
        }
        let package = PackageInfo {
            workspace_root: dir.path().to_path_buf(),
            ..conventional_package(name, root)
        };
        let paths = ModulePaths {
            paths: modules.iter().map(ToString::to_string).collect(),
            inline: inline.iter().map(ToString::to_string).collect(),
        };
        let module_paths = [(package.key.clone(), paths)].into_iter().collect();
        Self {
            _dir: dir,
            package,
            module_paths,
        }
    }

    fn path(&self, file: &str) -> PathBuf {
        self.package.path.join(file)
    }
}

/// The dependencies `file`, the file of `module_path` in `root`, writes as
/// production code, before the descendants of its module are read.
fn read(
    resolver: &mut PackageResolver,
    fixture: &Fixture,
    root: Root,
    file: &str,
    module_path: &str,
) -> super::package_resolver::Pending {
    read_as(
        resolver,
        fixture,
        root,
        file,
        module_path,
        EdgeContext::production(),
    )
}

fn read_as(
    resolver: &mut PackageResolver,
    fixture: &Fixture,
    root: Root,
    file: &str,
    module_path: &str,
    context: EdgeContext,
) -> super::package_resolver::Pending {
    let path = fixture.path(file);
    let source = std::fs::read_to_string(&path).expect("the fixture wrote the file");
    let syntax = syn::parse_file(&source).expect("the fixture file parses");
    resolver.dependencies(
        root,
        FileInModule {
            file: &path,
            module_path,
            context,
        },
        &syntax,
    )
}

fn targets(dependencies: &[DependencyRef]) -> Vec<(&str, &str, Option<&str>)> {
    dependencies
        .iter()
        .map(|dep| {
            (
                dep.target_package.as_str(),
                dep.target_module.as_str(),
                dep.target_item.as_deref(),
            )
        })
        .collect()
}

/// `b` re-exports `a::Item`, and `c` imports it through `b`.
fn reexport_chain() -> Fixture {
    Fixture::new(
        &[
            ("src/lib.rs", "mod a;\nmod b;\nmod c;"),
            ("src/a.rs", "pub struct Item;"),
            ("src/b.rs", "pub use crate::a::Item;"),
            ("src/c.rs", "use crate::b::Item;"),
        ],
        &["a", "b", "c"],
        &[],
    )
}

#[test]
fn a_reexport_chain_ends_on_the_definer() {
    let fixture = reexport_chain();
    let no_externals = HashMap::new();
    let resolution = NameResolution::new(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
        false,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);

    let pending = read(&mut resolver, &fixture, Root::entry(0), "src/c.rs", "c");

    assert_eq!(
        targets(&resolver.finish(pending)),
        [("p", "a", Some("Item"))]
    );
}

#[test]
fn a_private_use_only_a_descendant_names_something_through_is_no_dependency() {
    let fixture = Fixture::new(
        &[
            ("src/lib.rs", "mod a;\nmod b;"),
            ("src/a.rs", "pub struct Item;"),
            ("src/b/mod.rs", "mod c;\nuse crate::a::Item;"),
            ("src/b/c.rs", "use super::Item;"),
        ],
        &["a", "b", "b::c"],
        &[],
    );
    let no_externals = HashMap::new();
    let resolution = NameResolution::new(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
        false,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);

    let holder = read(&mut resolver, &fixture, Root::entry(0), "src/b/mod.rs", "b");
    let descendant = read(
        &mut resolver,
        &fixture,
        Root::entry(0),
        "src/b/c.rs",
        "b::c",
    );

    assert_eq!(
        targets(&resolver.finish(descendant)),
        [("p", "a", Some("Item"))]
    );
    assert_eq!(targets(&resolver.finish(holder)), []);
}

/// `a` declares `inner` inline, with `X` in it.
fn inline_module() -> Fixture {
    Fixture::new(
        &[
            ("src/lib.rs", "mod a;\nmod b;"),
            (
                "src/a.rs",
                "pub mod inner { pub struct X; }\nuse self::inner::X;",
            ),
            ("src/b.rs", "use crate::a::inner;\nuse crate::a::inner::X;"),
        ],
        &["a", "a::inner", "b"],
        &["a::inner"],
    )
}

#[test]
fn a_target_in_an_inline_module_is_the_module_of_its_file() {
    let fixture = inline_module();
    let no_externals = HashMap::new();
    let resolution = NameResolution::new(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
        false,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);

    let pending = read(&mut resolver, &fixture, Root::entry(0), "src/b.rs", "b");

    assert_eq!(
        targets(&resolver.finish(pending)),
        [("p", "a", Some("inner")), ("p", "a", Some("X"))]
    );
}

#[test]
fn a_path_that_stays_in_its_file_is_no_dependency() {
    let fixture = inline_module();
    let no_externals = HashMap::new();
    let resolution = NameResolution::new(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
        false,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);

    let pending = read(&mut resolver, &fixture, Root::entry(0), "src/a.rs", "a");

    assert_eq!(targets(&resolver.finish(pending)), []);
}

#[test]
fn a_module_has_the_definitions_the_table_holds_for_it() {
    let fixture = reexport_chain();
    let no_externals = HashMap::new();
    let resolution = NameResolution::new(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
        false,
    );
    let resolver = resolution.package(&fixture.package, &no_externals);

    let definitions = resolver.definitions(Root::entry(0), "a", UsageKind::Production);

    let names: Vec<&str> = definitions.keys().map(String::as_str).collect();
    assert_eq!(names, ["Item"]);
}

#[test]
fn an_integration_test_names_its_package_as_a_crate_of_its_own() {
    let fixture = Fixture::new(
        &[
            ("src/lib.rs", "pub mod a;"),
            ("src/a.rs", "pub struct Item;"),
            (
                "tests/t.rs",
                "mod helpers { pub fn help() {} }\nuse p::a::Item;\nuse crate::helpers::help;",
            ),
        ],
        &["a"],
        &[],
    );
    let no_externals = HashMap::new();
    let resolution = NameResolution::new(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
        true,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);
    let modules = ModulePaths {
        paths: ["helpers".to_string()].into(),
        inline: ["helpers".to_string()].into(),
    };

    let root = resolver.integration_test("p::tests::t", modules);
    let pending = read_as(
        &mut resolver,
        &fixture,
        root,
        "tests/t.rs",
        "",
        EdgeContext::test(TestKind::Integration),
    );

    assert_eq!(
        targets(&resolver.finish(pending)),
        [("p", "a", Some("Item"))]
    );
}

#[test]
fn without_the_table_a_path_ends_where_it_is_written() {
    let fixture = reexport_chain();
    let no_externals = HashMap::new();
    let resolution = NameResolution::without_table(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);

    let pending = read(&mut resolver, &fixture, Root::entry(0), "src/c.rs", "c");

    assert_eq!(
        targets(&resolver.finish(pending)),
        [("p", "b", Some("Item"))]
    );
}

/// The package `name` without an entry file: `a` defines `Item`, and `b`
/// imports it.
fn hir_package(name: &str) -> Fixture {
    Fixture::named(
        name,
        &[
            ("src/a.rs", "pub struct Item;"),
            ("src/b.rs", "use crate::a::Item;"),
        ],
        &["a", "b"],
        &[],
    )
}

#[test]
fn without_the_table_a_package_needs_no_entry_file() {
    let fixture = hir_package("my-pkg");
    let no_externals = HashMap::new();
    let resolution = NameResolution::without_table(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);

    let pending = read(&mut resolver, &fixture, Root::entry(0), "src/b.rs", "b");

    assert_eq!(
        targets(&resolver.finish(pending)),
        [("my_pkg", "a", Some("Item"))]
    );
}

#[test]
fn without_the_table_targets_in_inline_modules_stay_as_written() {
    let fixture = inline_module();
    let no_externals = HashMap::new();
    let resolution = NameResolution::without_table(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);

    let holder = read(&mut resolver, &fixture, Root::entry(0), "src/a.rs", "a");
    let user = read(&mut resolver, &fixture, Root::entry(0), "src/b.rs", "b");

    assert_eq!(
        targets(&resolver.finish(holder)),
        [("p", "a::inner", Some("X"))]
    );
    assert_eq!(
        targets(&resolver.finish(user)),
        [("p", "a::inner", None), ("p", "a::inner", Some("X"))]
    );
}

#[test]
fn without_the_table_test_code_is_read() {
    let fixture = Fixture::new(
        &[
            ("src/lib.rs", "mod a;\nmod b;"),
            ("src/a.rs", "pub struct Item;"),
            (
                "src/b.rs",
                "#[cfg(test)]\nmod tests {\n    use crate::a::Item;\n}",
            ),
        ],
        &["a", "b"],
        &[],
    );
    let no_externals = HashMap::new();
    let resolution = NameResolution::without_table(
        std::slice::from_ref(&fixture.package),
        &fixture.module_paths,
    );
    let mut resolver = resolution.package(&fixture.package, &no_externals);

    let pending = read(&mut resolver, &fixture, Root::entry(0), "src/b.rs", "b");

    assert_eq!(
        targets(&resolver.finish(pending)),
        [("p", "a", Some("Item"))]
    );
}
