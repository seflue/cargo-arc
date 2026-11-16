//! `query moves`: each item whose move into a module of its own leaves only
//! smaller tangles, with the tangles the move leaves.

use crate::diagnose::MoveReport;
use serde_json::{Map, Value};

pub(crate) const COLUMNS: [&str; 6] = [
    "symbol",
    "provider",
    "tangle",
    "size",
    "sizes-after",
    "released",
];

/// Return the header fields `query moves` adds: the tangles of `report` and
/// the number of moves tried, then what a move is, what each column says and
/// what the prediction leaves out.
pub(crate) fn header_fields(report: &MoveReport) -> Vec<(&'static str, Value)> {
    let tangles: Map<String, Value> = report
        .clusters
        .iter()
        .map(|modules| (modules[0].clone(), modules.len().into()))
        .collect();
    let tangles = if tangles.is_empty() {
        Value::Null
    } else {
        Value::Object(tangles)
    };
    vec![
        ("tangles", tangles),
        ("candidates", report.candidates.into()),
        (
            "move",
            "one item gets a new module of its own; a struct, enum or union takes along the \
             impl blocks of its file that name it by one segment; the usages and imports of \
             the item then lead to the new module, and the new module uses what the item \
             used, items of its old module included"
                .into(),
        ),
        (
            "stays",
            "everything else the old module uses or imports stays in it, including what \
             belongs to no item: a bare import, a glob import nothing takes a name from, a \
             path to a module, an import nothing in the file names; an impl block that names \
             its type by a path, or whose type the file does not declare, stays too"
                .into(),
        ),
        (
            "records",
            "one per candidate whose move leaves only tangles that are smaller than its \
             tangle, the new module counted; a move that only splits a tangle has one; a \
             candidate is an item that uses, or is used by, another module of its tangle in \
             production code; each move is tried alone on the graph of this run, so run \
             again after a move is made in the code"
                .into(),
        ),
        (
            "tangle",
            "the tangle of the provider, named by its first module in name order as in the \
             header field tangles, which gives each tangle's number of modules"
                .into(),
        ),
        (
            "sizes-after",
            "the sizes of the tangles that the modules of the tangle and the new module \
             form after the move, largest first; empty when none is left"
                .into(),
        ),
        (
            "released",
            "the modules of the tangle that lie in no tangle after the move".into(),
        ),
        (
            "not-seen",
            "method calls on a receiver and field accesses, so the new module can need an \
             import that the prediction leaves in the old module; whether the code builds \
             after the move"
                .into(),
        ),
        (
            "order",
            "by tangle, the largest first, then by the largest tangle the move leaves, the \
             smallest first, then by the modules it leaves in tangles, the fewest first, \
             then provider and symbol"
                .into(),
        ),
    ]
}

/// Return one record per move of `report`, in its order.
pub(crate) fn records(report: &MoveReport) -> Vec<Vec<Value>> {
    report
        .moves
        .iter()
        .map(|a_move| {
            let tangle = &report.clusters[a_move.cluster];
            vec![
                a_move.symbol.as_str().into(),
                a_move.provider.as_str().into(),
                tangle[0].as_str().into(),
                tangle.len().into(),
                a_move.sizes_after.clone().into(),
                a_move.released.clone().into(),
            ]
        })
        .collect()
}
