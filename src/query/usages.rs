//! `query usages`: every usage of a symbol in another module, with its line
//! and usage category.

use crate::graph::Usage;
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) const COLUMNS: [&str; 6] = [
    "symbol",
    "provider",
    "module",
    "path",
    "line",
    "usage-category",
];

/// Return the usages of the items named `symbol`, ordered by provider, path,
/// line and usage category.
pub(crate) fn of_symbol(usages: Vec<Usage>, symbol: &str) -> Vec<Usage> {
    let mut selected: Vec<Usage> = usages
        .into_iter()
        .filter(|usage| usage.symbol == symbol)
        .collect();
    selected.sort_by(|a, b| {
        (&a.provider, &a.source_file, a.line)
            .cmp(&(&b.provider, &b.source_file, b.line))
            .then_with(|| a.category.to_string().cmp(&b.category.to_string()))
    });
    selected
}

/// Return the header fields `query usages` adds: the name asked for, the
/// items `usages` found under it, and what counts as a usage.
pub(crate) fn header_fields(symbol: &str, usages: &[Usage]) -> Vec<(&'static str, Value)> {
    let items: BTreeSet<String> = usages
        .iter()
        .map(|usage| format!("{}::{}", usage.provider, usage.symbol))
        .collect();
    let items = if items.is_empty() {
        Value::Null
    } else {
        Value::from(items.into_iter().collect::<Vec<_>>())
    };
    vec![
        ("symbol", symbol.into()),
        ("items", items),
        (
            "usage",
            "a place where a module uses a symbol of another module, through an import or \
             by its full path: a call, a type in a signature or a bound, a struct \
             literal, a const or static read. An import nothing in the file names counts \
             once, at the import's line: a trait as functions, used for its methods, \
             another type as types, a const or static as values, a function as \
             functions, any other item by the case of its name"
                .into(),
        ),
        (
            "not-listed",
            "method calls on a receiver, usages inside the module that provides the symbol \
             (an item used only there is missing from items as well), pub use re-exports, \
             usages in test code unless --include-tests (with it, test and production usages \
             share the records unmarked)"
                .into(),
        ),
        (
            "order",
            "by provider, then path, line and usage category name".into(),
        ),
    ]
}

/// Return one record per usage, in the order of `usages`.
pub(crate) fn records(usages: &[Usage]) -> Vec<Vec<Value>> {
    usages
        .iter()
        .map(|usage| {
            vec![
                usage.symbol.as_str().into(),
                usage.provider.as_str().into(),
                usage.module.as_str().into(),
                usage.source_file.display().to_string().into(),
                usage.line.into(),
                usage.category.to_string().into(),
            ]
        })
        .collect()
}
