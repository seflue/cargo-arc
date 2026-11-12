//! `query usages`: every usage of a symbol, or of every symbol, with its
//! line and usage category.

use crate::graph::Usage;
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) const COLUMNS: [&str; 7] = [
    "symbol",
    "provider",
    "module",
    "enclosing-item",
    "path",
    "line",
    "usage-category",
];

/// Return the usages of the items named `symbol`, or all `usages` without
/// one, ordered by provider, path, line, usage category and symbol.
pub(crate) fn selected(usages: Vec<Usage>, symbol: Option<&str>) -> Vec<Usage> {
    let mut selected: Vec<Usage> = usages
        .into_iter()
        .filter(|usage| symbol.is_none_or(|symbol| usage.symbol == symbol))
        .collect();
    selected.sort_by(|a, b| {
        (&a.provider, &a.source_file, a.line)
            .cmp(&(&b.provider, &b.source_file, b.line))
            .then_with(|| a.category.to_string().cmp(&b.category.to_string()))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    selected
}

/// Return the header fields `query usages` adds: the name asked for and the
/// items `usages` found under it, where a name was asked for, and what
/// counts as a usage.
pub(crate) fn header_fields(symbol: Option<&str>, usages: &[Usage]) -> Vec<(&'static str, Value)> {
    let mut fields = Vec::new();
    if let Some(symbol) = symbol {
        let items: BTreeSet<String> = usages
            .iter()
            .map(|usage| format!("{}::{}", usage.provider, usage.symbol))
            .collect();
        let items = if items.is_empty() {
            Value::Null
        } else {
            Value::from(items.into_iter().collect::<Vec<_>>())
        };
        fields.push(("symbol", symbol.into()));
        fields.push(("items", items));
    }
    fields.extend([
        (
            "usage",
            "a place where a module uses a symbol, its own or another module's, through an \
             import or by its full path: a call, a type in a signature or a bound, a \
             struct literal, a const or static read; impl Writer uses Writer. An import \
             nothing in the file names counts once, at the import's line and in no \
             enclosing item: a trait as functions, used for its methods, another type as \
             types, a const or static as values, a function as functions, any other item \
             by the case of its name"
                .into(),
        ),
        (
            "not-listed",
            "method calls on a receiver, a recursive call, a field of the item's own type, \
             Self, pub use re-exports, usages in test code unless \
             --include-tests (with it, test and production usages share the records \
             unmarked)"
                .into(),
        ),
        (
            "order",
            "by provider, then path, line, usage category name and symbol".into(),
        ),
    ]);
    fields
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
                usage
                    .enclosing_item
                    .as_deref()
                    .map_or(Value::Null, Value::from),
                usage.source_file.display().to_string().into(),
                usage.line.into(),
                usage.category.to_string().into(),
            ]
        })
        .collect()
}
