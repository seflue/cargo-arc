//! Test-only constructors for rule configuration types.

use crate::rules::config::AllowedEdge;

/// An `allow` entry written on the rule; `to` may be a relative target.
pub(super) fn allow_edge(from: &str, to: &str) -> AllowedEdge {
    AllowedEdge {
        from: from.into(),
        to: to.parse().unwrap(),
        reason: None,
        dependency_pattern: None,
    }
}

/// An `allow` entry that came in through a `[dependency-patterns]` definition.
pub(super) fn pattern_edge(name: &str, from: &str, to: &str) -> AllowedEdge {
    AllowedEdge {
        dependency_pattern: Some(name.into()),
        ..allow_edge(from, to)
    }
}
