//! `query hotspots`: files of the hotspot ranking with their code lines and
//! commits.

use crate::hotspots::{HotspotNode, HotspotTree};
use crate::volatility::Window;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use super::utc_date;

pub(crate) const COLUMNS: [&str; 4] = ["rank", "path", "code-lines", "commits"];

/// Which files of the ranking the records list.
pub(crate) enum Selection<'a> {
    /// The first N hotspots.
    Top(usize),
    /// Exactly these workspace-relative files, each with its rank among all
    /// files.
    Paths(&'a [PathBuf]),
}

/// Return the header fields `query hotspots` adds: N, the ranking and
/// counting rules, and the window the commits were counted in (`None`
/// without volatility data).
pub(crate) fn header_fields(
    selection: &Selection<'_>,
    window: Option<Window>,
) -> Vec<(&'static str, Value)> {
    let top = match selection {
        Selection::Top(n) => Value::from(*n),
        Selection::Paths(_) => Value::Null,
    };
    let date = |pick: fn(Window) -> i64| window.map_or(Value::Null, |w| utc_date(pick(w)).into());
    vec![
        ("top", top),
        (
            "ranking",
            "code lines times commits, highest first; a file scoring zero is never among \
             the top N"
                .into(),
        ),
        ("ties", "by path, relative to the workspace root".into()),
        (
            "code-lines",
            "tokei's code count: source lines without comments and blank lines".into(),
        ),
        (
            "commits",
            "distinct commits in the window that add, modify, rename or copy the file \
             (git log --diff-filter=AMRC); merge commits and deletions do not count"
                .into(),
        ),
        ("window-start", date(|w| w.start)),
        ("window-end", date(|w| w.end)),
    ]
}

/// Return the records `selection` picks from `tree`, in ranking order.
///
/// # Errors
/// Names each path of a `Selection::Paths` that is not in the ranking.
pub(crate) fn records(
    tree: &HotspotTree,
    selection: &Selection<'_>,
) -> anyhow::Result<Vec<Vec<Value>>> {
    match selection {
        Selection::Top(_) => Ok(tree
            .hotspots
            .iter()
            .enumerate()
            .map(|(index, file)| record(index + 1, file))
            .collect()),
        Selection::Paths(paths) => named_files(tree, paths),
    }
}

fn named_files(tree: &HotspotTree, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<Value>>> {
    let ranking = tree.ranking();
    let known: BTreeSet<&Path> = ranking.iter().map(|file| file.file.as_path()).collect();
    let named: BTreeSet<PathBuf> = paths.iter().map(|path| without_dot(path)).collect();
    let unknown: Vec<String> = paths
        .iter()
        .filter(|path| !known.contains(without_dot(path).as_path()))
        .map(|path| path.display().to_string())
        .collect();
    if !unknown.is_empty() {
        anyhow::bail!(
            "not in the ranking (files of the module tree; test files only with \
             --include-tests): {}",
            unknown.join(", ")
        );
    }
    Ok(ranking
        .iter()
        .enumerate()
        .filter(|(_, file)| named.contains(&file.file))
        .map(|(index, file)| record(index + 1, file))
        .collect())
}

/// Return `path` without its `.` components, so `./src/a.rs` names the
/// ranking's `src/a.rs`.
fn without_dot(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}

fn record(rank: usize, file: &HotspotNode) -> Vec<Value> {
    vec![
        rank.into(),
        file.file.display().to_string().into(),
        file.lines.into(),
        file.commits.into(),
    ]
}
