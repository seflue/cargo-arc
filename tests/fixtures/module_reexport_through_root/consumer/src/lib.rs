mod check;
mod infer;

use provider::{
    kind::{Marker, Mode},
    report::detail::Failure,
    Thing,
};

pub fn keep(_: Marker, _: Thing) {}
