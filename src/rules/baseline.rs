//! Frozen violations from `arc-baseline.toml`.

use crate::model::{Edge, EdgeSymbols};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Identifies a violation independent of the rule wording that produced it: the
/// edge it runs on, plus the symbols observed crossing it.
///
/// The edge is what a stored entry is looked up by. The symbols are what the
/// run observed on a fresh key, and what the entry tolerates on a stored one.
///
/// A cycle has no key of its own: it is frozen when every one of its edges is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ViolationKey {
    pub edge: Edge,
    pub symbols: EdgeSymbols,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaselineEntry {
    pub rule: String,
    pub key: ViolationKey,
}

/// An entry that no longer describes what the run finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleEntry {
    /// The edge no longer violates: fixed, or the rule renamed out from under
    /// the entry.
    Gone(BaselineEntry),
    /// The edge still violates, but carries less than the entry tolerates.
    /// `entry` holds the tolerated set, `surplus` the part of it nothing
    /// crosses any more.
    TooWide {
        entry: BaselineEntry,
        surplus: EdgeSymbols,
    },
}

impl StaleEntry {
    #[must_use]
    pub fn entry(&self) -> &BaselineEntry {
        match self {
            Self::Gone(entry) | Self::TooWide { entry, .. } => entry,
        }
    }
}

/// The edges one rule freezes, and the symbols each of them tolerates.
type FrozenEdges = HashMap<Edge, EdgeSymbols>;

/// Rule name outside, edge inside, so a lookup borrows both and allocates
/// nothing.
#[derive(Debug, Default)]
pub struct Baseline {
    entries: HashMap<String, FrozenEdges>,
}

impl Baseline {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Missing file is not an error: nothing is frozen then.
    ///
    /// # Errors
    /// Returns `BaselineError::Io` for I/O failures other than a missing file,
    /// or `BaselineError::Parse` for invalid TOML.
    pub fn load(path: &Path) -> Result<Self, BaselineError> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(BaselineError::Io(path.to_path_buf(), e)),
        };
        let on_disk: OnDiskBaseline =
            toml::from_str(&content).map_err(|e| BaselineError::Parse(path.to_path_buf(), e))?;
        let mut entries: HashMap<String, FrozenEdges> = HashMap::new();
        for violation in on_disk.violations {
            let entry = violation.into_entry();
            entries
                .entry(entry.rule)
                .or_default()
                .entry(entry.key.edge)
                .or_default()
                .merge(&entry.key.symbols);
        }
        Ok(Self { entries })
    }

    /// The symbols `rule` tolerates on `edge`, if it is frozen at all.
    ///
    /// Returns the set rather than a verdict: the message for an edge that
    /// outgrew its entry has to name what was frozen.
    #[must_use]
    pub fn frozen_for(&self, rule: &str, edge: &Edge) -> Option<&EdgeSymbols> {
        self.entries.get(rule)?.get(edge)
    }

    /// Entries the run no longer confirms, sorted so a report over them reads
    /// the same on every run. `hits` are the frozen edges this run found, each
    /// carrying the symbols observed on it.
    ///
    /// Whether such an entry is worth reporting is the caller's call: the
    /// baseline does not know which rules a run skipped.
    #[must_use]
    pub fn unmatched(&self, hits: &[BaselineEntry]) -> Vec<StaleEntry> {
        let observed: HashMap<(&str, &Edge), &EdgeSymbols> = hits
            .iter()
            .map(|hit| ((hit.rule.as_str(), &hit.key.edge), &hit.key.symbols))
            .collect();
        let mut stale: Vec<StaleEntry> = self
            .entries
            .iter()
            .flat_map(|(rule, edges)| {
                edges
                    .iter()
                    .map(move |(edge, symbols)| (rule, edge, symbols))
            })
            .filter_map(|(rule, edge, tolerated)| {
                let entry = BaselineEntry {
                    rule: rule.clone(),
                    key: ViolationKey {
                        edge: edge.clone(),
                        symbols: tolerated.clone(),
                    },
                };
                match observed.get(&(rule.as_str(), edge)) {
                    None => Some(StaleEntry::Gone(entry)),
                    Some(found) => {
                        let surplus = tolerated.difference(found);
                        (!surplus.is_empty()).then_some(StaleEntry::TooWide { entry, surplus })
                    }
                }
            })
            .collect();
        stale.sort_by(|a, b| a.entry().cmp(b.entry()));
        stale
    }

    /// Number of frozen edges across all rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.values().map(HashMap::len).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Writes the file from scratch, entries sorted deterministically. Two
    /// records of the same edge under the same rule become one entry
    /// tolerating both symbol sets.
    ///
    /// # Errors
    /// Returns `BaselineError::Serialize` if encoding fails, or
    /// `BaselineError::Io` if the file cannot be written.
    pub fn write(path: &Path, entries: &[BaselineEntry]) -> Result<(), BaselineError> {
        let mut merged: BTreeMap<(&str, &Edge), EdgeSymbols> = BTreeMap::new();
        for entry in entries {
            merged
                .entry((&entry.rule, &entry.key.edge))
                .or_default()
                .merge(&entry.key.symbols);
        }
        let on_disk = OnDiskBaseline {
            config: OnDiskConfig { version: 1 },
            violations: merged
                .into_iter()
                .map(|((rule, edge), symbols)| OnDiskViolation {
                    rule: rule.to_string(),
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    symbols: symbols.named.into_iter().collect(),
                    bare: symbols.bare,
                })
                .collect(),
        };
        let content = toml::to_string_pretty(&on_disk).map_err(BaselineError::Serialize)?;
        std::fs::write(path, format!("{HEADER}{content}"))
            .map_err(|e| BaselineError::Io(path.to_path_buf(), e))
    }
}

/// Prepended by [`Baseline::write`]; TOML ignores it on the way back in.
const HEADER: &str = "\
# Generated by cargo-arc. Every entry freezes one dependency edge and the
# symbols crossing it; a symbol that appears later is reported again.
# Regenerate with: cargo arc check --generate-baseline
";

#[derive(Debug, Deserialize, Serialize)]
struct OnDiskConfig {
    version: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct OnDiskBaseline {
    config: OnDiskConfig,
    #[serde(default)]
    violations: Vec<OnDiskViolation>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OnDiskViolation {
    rule: String,
    from: String,
    to: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    symbols: Vec<String>,
    /// True when the edge carries a reference the resolver could not name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    bare: bool,
}

impl OnDiskViolation {
    fn into_entry(self) -> BaselineEntry {
        BaselineEntry {
            rule: self.rule,
            key: ViolationKey {
                edge: Edge::new(self.from, self.to),
                symbols: EdgeSymbols {
                    named: self.symbols.into_iter().collect(),
                    bare: self.bare,
                },
            },
        }
    }
}

#[derive(Debug)]
pub enum BaselineError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    Serialize(toml::ser::Error),
}

impl std::fmt::Display for BaselineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, err) => {
                write!(f, "cannot read baseline file {}: {err}", path.display())
            }
            Self::Parse(path, err) => {
                write!(f, "invalid baseline file {}: {err}", path.display())
            }
            Self::Serialize(err) => write!(f, "cannot serialize baseline: {err}"),
        }
    }
}

impl std::error::Error for BaselineError {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn symbols(named: &[&str]) -> EdgeSymbols {
        EdgeSymbols {
            named: named.iter().map(|s| (*s).to_string()).collect(),
            bare: false,
        }
    }

    fn bare() -> EdgeSymbols {
        EdgeSymbols {
            named: std::collections::BTreeSet::new(),
            bare: true,
        }
    }

    fn entry(rule: &str, from: &str, to: &str, symbols: EdgeSymbols) -> BaselineEntry {
        BaselineEntry {
            rule: rule.to_string(),
            key: ViolationKey {
                edge: Edge::new(from, to),
                symbols,
            },
        }
    }

    /// Round-trips `entries` through a throwaway file, the only way to get a
    /// populated [`Baseline`] (its fields are private).
    fn baseline_of(entries: &[BaselineEntry]) -> (TempDir, Baseline) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, entries).unwrap();
        let baseline = Baseline::load(&path).unwrap();
        (tmp, baseline)
    }

    #[test]
    fn an_entry_covers_a_subset_of_its_symbols_but_not_a_new_one() {
        let stored = entry(
            "no infra in domain",
            "domain::legacy",
            "infra::db",
            symbols(&["Foo", "Bar"]),
        );
        let (_tmp, baseline) = baseline_of(&[stored]);

        let tolerated = baseline
            .frozen_for(
                "no infra in domain",
                &Edge::new("domain::legacy", "infra::db"),
            )
            .unwrap();
        assert!(tolerated.covers(&symbols(&["Foo"])));
        assert!(!tolerated.covers(&symbols(&["Foo", "Baz"])));
    }

    #[test]
    fn an_unnamed_reference_needs_the_bare_flag_in_the_file() {
        let named_only = entry("a rule", "a", "b", symbols(&["Foo"]));
        let (_tmp, baseline) = baseline_of(&[named_only]);
        assert!(
            !baseline
                .frozen_for("a rule", &Edge::new("a", "b"))
                .unwrap()
                .covers(&bare())
        );

        let mut both = symbols(&["Foo"]);
        both.bare = true;
        let (_tmp, baseline) = baseline_of(&[entry("a rule", "a", "b", both)]);
        assert!(
            baseline
                .frozen_for("a rule", &Edge::new("a", "b"))
                .unwrap()
                .covers(&bare())
        );
    }

    #[test]
    fn frozen_for_is_scoped_to_rule_name() {
        let stored = entry(
            "no infra in domain",
            "domain::legacy",
            "infra::db",
            symbols(&["Db"]),
        );
        let (_tmp, baseline) = baseline_of(&[stored]);
        assert!(
            baseline
                .frozen_for(
                    "no infra in domain",
                    &Edge::new("domain::legacy", "infra::db")
                )
                .is_some()
        );
        assert!(
            baseline
                .frozen_for("other rule", &Edge::new("domain::legacy", "infra::db"))
                .is_none()
        );
    }

    #[test]
    fn round_trip_keeps_symbols_and_writes_violations_table() {
        let mut with_bare = symbols(&["Read"]);
        with_bare.bare = true;
        let entries = [
            entry(
                "no infra in domain",
                "domain::legacy",
                "infra::db",
                symbols(&["Pool", "Row"]),
            ),
            entry(
                "domain acyclic",
                "domain::a",
                "domain::b",
                with_bare.clone(),
            ),
        ];
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, &entries).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[[violations]]"));

        let baseline = Baseline::load(&path).unwrap();
        assert_eq!(baseline.len(), 2);
        assert_eq!(
            baseline.frozen_for(
                "no infra in domain",
                &Edge::new("domain::legacy", "infra::db")
            ),
            Some(&symbols(&["Pool", "Row"]))
        );
        assert_eq!(
            baseline.frozen_for("domain acyclic", &Edge::new("domain::a", "domain::b")),
            Some(&with_bare)
        );
    }

    #[test]
    fn the_generated_file_says_how_to_regenerate_it_without_disturbing_the_parser() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, &[entry("a rule", "a", "b", symbols(&["X"]))]).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("--generate-baseline"), "got:\n{content}");
        assert_eq!(Baseline::load(&path).unwrap().len(), 1);
    }

    #[test]
    fn write_is_deterministic_regardless_of_input_order() {
        let a = entry(
            "no infra in domain",
            "domain::legacy",
            "infra::db",
            symbols(&["Pool"]),
        );
        let b = entry(
            "domain acyclic",
            "domain::a",
            "domain::b",
            symbols(&["Node"]),
        );

        let tmp = TempDir::new().unwrap();
        let path_1 = tmp.path().join("order-1.toml");
        let path_2 = tmp.path().join("order-2.toml");
        Baseline::write(&path_1, &[a.clone(), b.clone()]).unwrap();
        Baseline::write(&path_2, &[b, a]).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path_1).unwrap(),
            std::fs::read_to_string(&path_2).unwrap()
        );
    }

    #[test]
    fn two_records_of_one_edge_become_one_entry_tolerating_both() {
        let (_tmp, baseline) = baseline_of(&[
            entry("a rule", "a", "b", symbols(&["One"])),
            entry("a rule", "a", "b", symbols(&["Two"])),
        ]);
        assert_eq!(baseline.len(), 1);
        assert_eq!(
            baseline.frozen_for("a rule", &Edge::new("a", "b")),
            Some(&symbols(&["One", "Two"]))
        );
    }

    #[test]
    fn missing_file_is_an_empty_baseline() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("does-not-exist.toml");
        let baseline = Baseline::load(&path).unwrap();
        assert!(baseline.is_empty());
        assert!(
            baseline
                .frozen_for("any rule", &Edge::new("a", "b"))
                .is_none()
        );
    }

    #[test]
    fn an_entry_without_an_edge_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        std::fs::write(
            &path,
            r#"
            [config]
            version = 1

            [[violations]]
            rule = "broken"
            "#,
        )
        .unwrap();

        assert!(matches!(
            Baseline::load(&path),
            Err(BaselineError::Parse(_, _))
        ));
    }

    #[test]
    fn an_entry_matched_with_the_symbols_it_names_is_not_stale() {
        let stored = entry("a rule", "a", "b", symbols(&["One", "Two"]));
        let (_tmp, baseline) = baseline_of(std::slice::from_ref(&stored));
        assert!(baseline.unmatched(&[stored]).is_empty());
    }

    #[test]
    fn an_edge_the_run_no_longer_finds_is_gone() {
        let hit = entry("a rule", "a", "b", symbols(&["One"]));
        let fixed = entry("a rule", "c", "d", symbols(&["Two"]));
        let (_tmp, baseline) = baseline_of(&[hit.clone(), fixed.clone()]);
        assert_eq!(baseline.unmatched(&[hit]), vec![StaleEntry::Gone(fixed)]);
    }

    #[test]
    fn an_entry_the_edge_outgrew_downward_is_too_wide() {
        let stored = entry("a rule", "a", "b", symbols(&["One", "Two"]));
        let (_tmp, baseline) = baseline_of(std::slice::from_ref(&stored));
        let observed = entry("a rule", "a", "b", symbols(&["One"]));
        assert_eq!(
            baseline.unmatched(&[observed]),
            vec![StaleEntry::TooWide {
                entry: stored,
                surplus: symbols(&["Two"]),
            }]
        );
    }

    #[test]
    fn a_hit_under_another_rule_name_does_not_confirm_the_entry() {
        let stored = entry("a rule", "a", "b", symbols(&["One"]));
        let (_tmp, baseline) = baseline_of(std::slice::from_ref(&stored));
        let elsewhere = entry("some other rule", "a", "b", symbols(&["One"]));
        assert_eq!(
            baseline.unmatched(&[elsewhere]),
            vec![StaleEntry::Gone(stored)]
        );
    }

    #[test]
    fn unmatched_order_is_independent_of_storage_order() {
        let a = entry("a rule", "x", "y", symbols(&["One"]));
        let b = entry("b rule", "m::a", "m::b", symbols(&["Two"]));
        let (_tmp_1, one_way) = baseline_of(&[a.clone(), b.clone()]);
        let (_tmp_2, other_way) = baseline_of(&[b, a]);
        assert_eq!(one_way.unmatched(&[]), other_way.unmatched(&[]));
    }
}
