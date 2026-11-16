//! `query tangles`: each module of a tangle with its level.

use crate::diagnose::ModuleLevel;
use serde_json::Value;

pub(crate) const COLUMNS: [&str; 4] = ["tangle", "size", "module", "level"];

/// Return the header fields `query tangles` adds: the `tangles` with their
/// sizes, then what a record is, what the columns hold and how the level
/// order is found.
pub(crate) fn header_fields(tangles: &[Vec<ModuleLevel>]) -> Vec<(&'static str, Value)> {
    vec![
        (
            "tangles",
            super::tangle_sizes(
                tangles
                    .iter()
                    .map(|modules| (name_of(modules), modules.len())),
            ),
        ),
        ("records", "one per module of a tangle".into()),
        (
            "tangle",
            "the tangle of the module, named by its first module in name order as in the \
             header field tangles, which gives each tangle's number of modules"
                .into(),
        ),
        (
            "level",
            "1 for a module that uses no module of its package, otherwise one more than the \
             highest level among the modules of its package it uses; in a tangle the edges \
             against its level order that close a cycle are left out"
                .into(),
        ),
        (
            "level-order",
            "the modules of a tangle in an order, a module before the modules it uses, that \
             leaves few symbols on the edges against it; a search finds it, and another order \
             can leave as few symbols or fewer and give other levels"
                .into(),
        ),
        (
            "order",
            "by tangle, the largest first, then by level, the highest first, then by module".into(),
        ),
    ]
}

/// Return one record per module of `tangles`, in their order.
pub(crate) fn records(tangles: &[Vec<ModuleLevel>]) -> Vec<Vec<Value>> {
    tangles
        .iter()
        .flat_map(|modules| {
            let tangle = name_of(modules);
            modules.iter().map(move |member| {
                vec![
                    tangle.into(),
                    modules.len().into(),
                    member.module.as_str().into(),
                    member.level.into(),
                ]
            })
        })
        .collect()
}

/// The name of the tangle of `modules`: its first module in name order.
fn name_of(modules: &[ModuleLevel]) -> &str {
    modules
        .iter()
        .map(|member| member.module.as_str())
        .min()
        .expect("a tangle has modules")
}
