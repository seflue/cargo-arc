//! `query edges`: each symbol that crosses an edge between two modules, with
//! its usage categories, its imports and the cycles on the edge.

use crate::layout::{LayoutIR, symbol_groups};
use serde_json::Value;

pub(crate) const COLUMNS: [&str; 7] = [
    "from",
    "to",
    "symbol",
    "usage-categories",
    "re-exported",
    "locations",
    "cycles",
];

/// Return the header fields `query edges` adds: what a record is, how the
/// records are ordered, what each column counts and what an edge leaves out.
pub(crate) fn header_fields() -> Vec<(&'static str, Value)> {
    vec![
        (
            "records",
            "one per edge between modules and per symbol the edge's imports name, a package \
             standing for its root module; the bare imports of an edge, which name no \
             symbol, share one record with an empty symbol; a manifest edge no import \
             backs has none"
                .into(),
        ),
        ("order", "by from, then to, then symbol".into()),
        (
            "usage-categories",
            "how the importing files use the symbol: types, values, functions; empty when \
             they never name it, as when they only re-export it, and always for the bare \
             imports"
                .into(),
        ),
        (
            "re-exported",
            "true when every import line that names the symbol is a pub use and the \
             importing file refers to no name on that line otherwise, as the diagram's \
             viaReexport; false for the bare imports"
                .into(),
        ),
        (
            "locations",
            "the imports that name the symbol, as path:line, the path relative to the \
             workspace root"
                .into(),
        ),
        (
            "cycles",
            "the representative cycles through the edge; a re-export, an edge whose \
             imports are all pub use, lies on none unless --include-reexports"
                .into(),
        ),
        (
            "not-shown",
            "method calls on a receiver, field access, usages inside the same module, and \
             test usages unless --include-tests"
                .into(),
        ),
    ]
}

/// Return one record per edge of `layout` and symbol group on it, ordered by
/// the edge's ends and then the symbol.
pub(crate) fn records(layout: &LayoutIR) -> Vec<Vec<Value>> {
    let mut records: Vec<Vec<Value>> = layout
        .edges
        .iter()
        .flat_map(|edge| {
            let from = layout.qualified_name(edge.from);
            let to = layout.qualified_name(edge.to);
            symbol_groups(&edge.source_locations)
                .into_iter()
                .map(|group| {
                    let categories: Vec<Value> = group
                        .usage_categories()
                        .iter()
                        .map(|category| category.to_string().into())
                        .collect();
                    let locations: Vec<Value> = group
                        .locations
                        .iter()
                        .map(|located| {
                            let location = &located.location;
                            format!("{}:{}", location.file.display(), location.line).into()
                        })
                        .collect();
                    vec![
                        from.as_str().into(),
                        to.as_str().into(),
                        group.symbol.into(),
                        categories.into(),
                        group.via_reexport.into(),
                        locations.into(),
                        edge.cycle_ids.len().into(),
                    ]
                })
                .collect::<Vec<_>>()
        })
        .collect();
    records.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    records
}

/// Return a record's from, to and symbol, the columns the records are
/// ordered by.
fn sort_key(record: &[Value]) -> [Option<&str>; 3] {
    [record[0].as_str(), record[1].as_str(), record[2].as_str()]
}
