//! Frozen findings from `arc-baseline.toml`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Identifies a finding independent of the rule wording that produced it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingKey {
    /// `forbidden-dependency` and `layers` report this shape.
    Edge { from: String, to: String },
    /// `no-cycles`: the ring's members in traversal order, canonically
    /// rotated to start at the smallest name.
    Ring(Vec<String>),
}

impl FindingKey {
    #[must_use]
    pub fn edge(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self::Edge {
            from: from.into(),
            to: to.into(),
        }
    }

    /// Rotates to start at the lexicographically smallest name, preserving
    /// direction: rotation of the same ring keeps the key, the reverse
    /// traversal over the same members does not.
    #[must_use]
    pub fn ring(members: impl IntoIterator<Item = String>) -> Self {
        let mut members: Vec<String> = members.into_iter().collect();
        if let Some(min_pos) = (0..members.len()).min_by_key(|&i| members[i].as_str()) {
            members.rotate_left(min_pos);
        }
        Self::Ring(members)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineEntry {
    pub rule: String,
    pub key: FindingKey,
}

/// Keyed by rule name so a lookup borrows both parts instead of rebuilding
/// them: the rule name is the foreign key, the set holds that rule's findings.
#[derive(Debug, Default)]
pub struct Baseline {
    entries: HashMap<String, HashSet<FindingKey>>,
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
    /// Returns `BaselineError::Io` for I/O failures other than a missing
    /// file, `BaselineError::Parse` for invalid TOML, or
    /// `BaselineError::Malformed` if a finding has neither a full
    /// `from`/`to` pair nor a `ring` (or both).
    pub fn load(path: &Path) -> Result<Self, BaselineError> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(BaselineError::Io(path.to_path_buf(), e)),
        };
        let on_disk: OnDiskBaseline =
            toml::from_str(&content).map_err(|e| BaselineError::Parse(path.to_path_buf(), e))?;
        let mut entries: HashMap<String, HashSet<FindingKey>> = HashMap::new();
        for finding in on_disk.findings {
            let entry = finding.into_entry(path)?;
            entries.entry(entry.rule).or_default().insert(entry.key);
        }
        Ok(Self { entries })
    }

    #[must_use]
    pub fn covers(&self, rule: &str, key: &FindingKey) -> bool {
        self.entries
            .get(rule)
            .is_some_and(|keys| keys.contains(key))
    }

    /// Entries that no finding in `hits` matched, sorted so a report over them
    /// reads the same on every run. Whether such an entry is worth reporting is
    /// the caller's call: the baseline does not know which rules a run skipped.
    #[must_use]
    pub fn unmatched(&self, hits: &[BaselineEntry]) -> Vec<BaselineEntry> {
        let hit: HashSet<(&str, &FindingKey)> = hits
            .iter()
            .map(|entry| (entry.rule.as_str(), &entry.key))
            .collect();
        let mut left: Vec<BaselineEntry> = self
            .entries
            .iter()
            .flat_map(|(rule, keys)| keys.iter().map(move |key| (rule, key)))
            .filter(|(rule, key)| !hit.contains(&(rule.as_str(), *key)))
            .map(|(rule, key)| BaselineEntry {
                rule: rule.clone(),
                key: key.clone(),
            })
            .collect();
        left.sort_by(|a, b| (&a.rule, &a.key).cmp(&(&b.rule, &b.key)));
        left
    }

    /// Number of frozen findings across all rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.values().map(HashSet::len).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Writes the file from scratch, entries sorted deterministically.
    ///
    /// # Errors
    /// Returns `BaselineError::Serialize` if encoding fails, or
    /// `BaselineError::Io` if the file cannot be written.
    pub fn write(path: &Path, entries: &[BaselineEntry]) -> Result<(), BaselineError> {
        let mut sorted: Vec<&BaselineEntry> = entries.iter().collect();
        sorted.sort_by(|a, b| (&a.rule, &a.key).cmp(&(&b.rule, &b.key)));
        let on_disk = OnDiskBaseline {
            config: OnDiskConfig { version: 1 },
            findings: sorted.into_iter().map(OnDiskFinding::from_entry).collect(),
        };
        let content = toml::to_string_pretty(&on_disk).map_err(BaselineError::Serialize)?;
        std::fs::write(path, content).map_err(|e| BaselineError::Io(path.to_path_buf(), e))
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct OnDiskConfig {
    version: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct OnDiskBaseline {
    config: OnDiskConfig,
    #[serde(default)]
    findings: Vec<OnDiskFinding>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OnDiskFinding {
    rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ring: Option<Vec<String>>,
}

impl OnDiskFinding {
    fn from_entry(entry: &BaselineEntry) -> Self {
        match &entry.key {
            FindingKey::Edge { from, to } => Self {
                rule: entry.rule.clone(),
                from: Some(from.clone()),
                to: Some(to.clone()),
                ring: None,
            },
            FindingKey::Ring(members) => Self {
                rule: entry.rule.clone(),
                from: None,
                to: None,
                ring: Some(members.clone()),
            },
        }
    }

    fn into_entry(self, path: &Path) -> Result<BaselineEntry, BaselineError> {
        match (self.from, self.to, self.ring) {
            (Some(from), Some(to), None) => Ok(BaselineEntry {
                rule: self.rule,
                key: FindingKey::Edge { from, to },
            }),
            (None, None, Some(ring)) => Ok(BaselineEntry {
                rule: self.rule,
                key: FindingKey::ring(ring),
            }),
            _ => Err(BaselineError::Malformed(
                path.to_path_buf(),
                format!(
                    "finding for rule {:?} needs either from/to or ring, not both or neither",
                    self.rule
                ),
            )),
        }
    }
}

#[derive(Debug)]
pub enum BaselineError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    Malformed(PathBuf, String),
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
            Self::Malformed(path, msg) => {
                write!(f, "malformed baseline file {}: {msg}", path.display())
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

    #[test]
    fn ring_key_is_independent_of_rotation() {
        let a = FindingKey::ring(["a".to_string(), "b".to_string(), "c".to_string()]);
        let b = FindingKey::ring(["c".to_string(), "a".to_string(), "b".to_string()]);
        assert_eq!(a, b);
    }

    #[test]
    fn ring_key_differs_by_membership_and_length() {
        let base = FindingKey::ring(["a".to_string(), "b".to_string(), "c".to_string()]);
        let different_member =
            FindingKey::ring(["a".to_string(), "b".to_string(), "d".to_string()]);
        let different_length = FindingKey::ring(["a".to_string(), "b".to_string()]);
        assert_ne!(base, different_member);
        assert_ne!(base, different_length);
    }

    #[test]
    fn ring_key_differs_by_traversal_direction() {
        let forward = FindingKey::ring(["a".to_string(), "b".to_string(), "c".to_string()]);
        let backward = FindingKey::ring(["a".to_string(), "c".to_string(), "b".to_string()]);
        assert_ne!(forward, backward);
    }

    #[test]
    fn covers_is_scoped_to_rule_name() {
        let entries = [BaselineEntry {
            rule: "no infra in domain".to_string(),
            key: FindingKey::edge("domain::legacy", "infra::db"),
        }];
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, &entries).unwrap();
        let baseline = Baseline::load(&path).unwrap();

        let key = FindingKey::edge("domain::legacy", "infra::db");
        assert!(baseline.covers("no infra in domain", &key));
        assert!(!baseline.covers("other rule", &key));
    }

    #[test]
    fn round_trip_covers_both_key_shapes_and_writes_findings_table() {
        let entries = [
            BaselineEntry {
                rule: "no infra in domain".to_string(),
                key: FindingKey::edge("domain::legacy", "infra::db"),
            },
            BaselineEntry {
                rule: "domain acyclic".to_string(),
                key: FindingKey::ring(vec!["domain::a".to_string(), "domain::b".to_string()]),
            },
        ];
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, &entries).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[[findings]]"));

        let baseline = Baseline::load(&path).unwrap();
        assert_eq!(baseline.len(), 2);
        assert!(baseline.covers(
            "no infra in domain",
            &FindingKey::edge("domain::legacy", "infra::db")
        ));
        assert!(baseline.covers(
            "domain acyclic",
            &FindingKey::ring(vec!["domain::a".to_string(), "domain::b".to_string()])
        ));
    }

    #[test]
    fn write_is_deterministic_regardless_of_input_order() {
        let a = BaselineEntry {
            rule: "no infra in domain".to_string(),
            key: FindingKey::edge("domain::legacy", "infra::db"),
        };
        let b = BaselineEntry {
            rule: "domain acyclic".to_string(),
            key: FindingKey::ring(vec!["domain::a".to_string(), "domain::b".to_string()]),
        };

        let tmp = TempDir::new().unwrap();
        let path_1 = tmp.path().join("order-1.toml");
        let path_2 = tmp.path().join("order-2.toml");
        Baseline::write(&path_1, &[a.clone(), b.clone()]).unwrap();
        Baseline::write(&path_2, &[b, a]).unwrap();

        let content_1 = std::fs::read_to_string(&path_1).unwrap();
        let content_2 = std::fs::read_to_string(&path_2).unwrap();
        assert_eq!(content_1, content_2);
    }

    #[test]
    fn missing_file_is_an_empty_baseline() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("does-not-exist.toml");
        let baseline = Baseline::load(&path).unwrap();
        assert!(baseline.is_empty());
        assert!(!baseline.covers("any rule", &FindingKey::edge("a", "b")));
    }

    #[test]
    fn malformed_entry_without_from_to_or_ring_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        std::fs::write(
            &path,
            r#"
            [config]
            version = 1

            [[findings]]
            rule = "broken"
            "#,
        )
        .unwrap();

        let result = Baseline::load(&path);
        assert!(matches!(result, Err(BaselineError::Malformed(_, _))));
    }

    fn entry(rule: &str, key: FindingKey) -> BaselineEntry {
        BaselineEntry {
            rule: rule.to_string(),
            key,
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
    fn hit_entry_is_absent_from_unmatched() {
        let stored = entry(
            "no infra in domain",
            FindingKey::edge("domain::legacy", "infra::db"),
        );
        let (_tmp, baseline) = baseline_of(std::slice::from_ref(&stored));
        assert!(baseline.unmatched(&[stored]).is_empty());
    }

    #[test]
    fn entry_no_run_hit_is_reported() {
        let hit = entry(
            "no infra in domain",
            FindingKey::edge("domain::legacy", "infra::db"),
        );
        let fixed = entry(
            "no infra in domain",
            FindingKey::edge("domain::service", "infra::db"),
        );
        let (_tmp, baseline) = baseline_of(&[hit.clone(), fixed.clone()]);
        assert_eq!(baseline.unmatched(&[hit]), vec![fixed]);
    }

    #[test]
    fn hit_under_another_rule_name_does_not_cover_the_entry() {
        let stored = entry(
            "no infra in domain",
            FindingKey::edge("domain::legacy", "infra::db"),
        );
        let (_tmp, baseline) = baseline_of(std::slice::from_ref(&stored));
        let elsewhere = entry(
            "some other rule",
            FindingKey::edge("domain::legacy", "infra::db"),
        );
        assert_eq!(baseline.unmatched(&[elsewhere]), vec![stored]);
    }

    #[test]
    fn hit_in_another_rotation_covers_the_ring_entry() {
        let stored = entry(
            "domain acyclic",
            FindingKey::ring(vec![
                "domain::a".to_string(),
                "domain::b".to_string(),
                "domain::c".to_string(),
            ]),
        );
        let (_tmp, baseline) = baseline_of(&[stored]);
        let rotated = entry(
            "domain acyclic",
            FindingKey::ring(vec![
                "domain::c".to_string(),
                "domain::a".to_string(),
                "domain::b".to_string(),
            ]),
        );
        assert!(baseline.unmatched(&[rotated]).is_empty());
    }

    #[test]
    fn unmatched_order_is_independent_of_storage_order() {
        let a = entry("a rule", FindingKey::edge("x", "y"));
        let b = entry(
            "b rule",
            FindingKey::ring(vec!["m::a".to_string(), "m::b".to_string()]),
        );
        let (_tmp_1, one_way) = baseline_of(&[a.clone(), b.clone()]);
        let (_tmp_2, other_way) = baseline_of(&[b, a]);
        assert_eq!(one_way.unmatched(&[]), other_way.unmatched(&[]));
    }

    #[test]
    fn hand_written_rotated_ring_is_still_covered() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        std::fs::write(
            &path,
            r#"
            [config]
            version = 1

            [[findings]]
            rule = "domain acyclic"
            ring = ["domain::b", "domain::c", "domain::a"]
            "#,
        )
        .unwrap();

        let baseline = Baseline::load(&path).unwrap();
        assert!(baseline.covers(
            "domain acyclic",
            &FindingKey::ring(vec![
                "domain::a".to_string(),
                "domain::b".to_string(),
                "domain::c".to_string(),
            ])
        ));
    }
}
