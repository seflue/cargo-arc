use super::*;

mod resolve_use_tree_tests {
    use super::*;

    fn parse_use_tree(source: &str) -> syn::UseTree {
        let file: syn::File = syn::parse_str(source).unwrap();
        match file.items.into_iter().next().unwrap() {
            syn::Item::Use(u) => u.tree,
            _ => panic!("expected use item"),
        }
    }

    #[test]
    fn test_simple_path() {
        let tree = parse_use_tree("use crate::graph::build;");
        let paths = resolve_use_tree(&tree, "", false);
        assert_eq!(paths, vec!["crate::graph::build"]);
    }

    #[test]
    fn test_multi_import() {
        let tree = parse_use_tree("use cli::{Args, Cargo};");
        let mut paths = resolve_use_tree(&tree, "", false);
        paths.sort();
        assert_eq!(paths, vec!["cli::Args", "cli::Cargo"]);
    }

    #[test]
    fn test_glob() {
        let tree = parse_use_tree("use model::*;");
        let paths = resolve_use_tree(&tree, "", false);
        assert_eq!(paths, vec!["model::*"]);
    }

    #[test]
    fn test_rename() {
        let tree = parse_use_tree("use cli::Args as CliArgs;");
        let paths = resolve_use_tree(&tree, "", false);
        assert_eq!(paths, vec!["cli::Args"]);
    }

    #[test]
    fn test_nested_groups() {
        let tree = parse_use_tree("use a::{b::{C, D}, e::F};");
        let mut paths = resolve_use_tree(&tree, "", false);
        paths.sort();
        assert_eq!(paths, vec!["a::b::C", "a::b::D", "a::e::F"]);
    }

    #[test]
    fn test_empty_prefix_root_level() {
        let tree = parse_use_tree("use std;");
        let paths = resolve_use_tree(&tree, "", false);
        assert_eq!(paths, vec!["std"]);
    }

    #[test]
    fn test_rename_with_alias() {
        let tree = parse_use_tree("use cli::Args as CliArgs;");
        let paths = resolve_use_tree(&tree, "", true);
        assert_eq!(paths, vec!["cli::CliArgs"]);
    }

    #[test]
    fn test_with_prefix() {
        let tree = parse_use_tree("use bar::Baz;");
        let paths = resolve_use_tree(&tree, "foo", false);
        assert_eq!(paths, vec!["foo::bar::Baz"]);
    }
}

mod relative_path_tests {
    use super::*;

    #[test]
    fn test_super_basic() {
        // super::foo from module "a::b", depth 0 → "a::foo"
        let result = resolve_relative_path("super::foo", "a::b");
        assert_eq!(result, Some("a::foo".to_string()));
    }

    #[test]
    fn test_super_from_root() {
        // super::foo from crate root (empty path) → None (no parent)
        let result = resolve_relative_path("super::foo", "");
        assert_eq!(result, None);
    }

    #[test]
    fn test_super_super() {
        // super::super::foo from "a::b::c" → "a::foo"
        let result = resolve_relative_path("super::super::foo", "a::b::c");
        assert_eq!(result, Some("a::foo".to_string()));
    }

    #[test]
    fn test_super_too_many() {
        // super::super::foo from "a" → None (can't go above root)
        let result = resolve_relative_path("super::super::foo", "a");
        assert_eq!(result, None);
    }

    #[test]
    fn test_self_basic() {
        // self::child from "render", depth 0 → "render::child"
        let result = resolve_relative_path("self::child", "render");
        assert_eq!(result, Some("render::child".to_string()));
    }

    #[test]
    fn test_self_from_root() {
        // self::foo from crate root → "foo"
        let result = resolve_relative_path("self::foo", "");
        assert_eq!(result, Some("foo".to_string()));
    }
}
