//! Git history analysis for code volatility measurement.
//!
//! Analyzes git log to determine how frequently Rust source files change.
//! Supports configurable thresholds and time periods.
//! Optimized for large repositories using streaming and git-level path filtering.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

/// Errors that can occur during volatility analysis.
#[derive(Error, Debug)]
pub enum VolatilityError {
    #[error("Not a git repository")]
    NotGitRepo,
    #[error("Failed to execute git command: {0}")]
    GitCommand(#[from] std::io::Error),
    #[error("Failed to parse git output: {0}")]
    Parse(String),
}

/// Volatility classification based on change frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Volatility {
    Low,
    Medium,
    High,
}

impl Volatility {
    /// Classify a change count into a volatility level using the given config.
    #[must_use]
    pub fn from_count(count: usize, config: &VolatilityConfig) -> Self {
        if count <= config.low_threshold {
            Self::Low
        } else if count <= config.high_threshold {
            Self::Medium
        } else {
            Self::High
        }
    }
}

impl std::fmt::Display for Volatility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        };
        f.write_str(label)
    }
}

/// Configuration for volatility analysis.
pub struct VolatilityConfig {
    /// Analysis period in months.
    pub months: usize,
    /// Maximum change count still considered low volatility.
    pub low_threshold: usize,
    /// Maximum change count still considered medium volatility.
    pub high_threshold: usize,
}

impl Default for VolatilityConfig {
    fn default() -> Self {
        Self {
            months: 6,
            low_threshold: 2,
            high_threshold: 10,
        }
    }
}

/// A single commit that touched a file: its hash and commit time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    /// Commit time as a Unix timestamp (`%ct` in `git log`).
    pub timestamp: i64,
}

impl PartialOrd for Commit {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Commit {
    /// Orders by time, so a `BTreeSet<Commit>` iterates oldest first; falls
    /// back to the hash to keep the order total when two commits share a
    /// timestamp.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.timestamp
            .cmp(&other.timestamp)
            .then_with(|| self.hash.cmp(&other.hash))
    }
}

/// Volatility analyzer using git history.
///
/// Holds, per file, the dated set of commits that touched it, keyed by
/// absolute path so a workspace nested inside the repository still resolves.
/// `get_change_count`, `get_volatility` and `format_report` are read views
/// over this table.
pub struct VolatilityAnalyzer {
    config: VolatilityConfig,
    repo_root: PathBuf,
    commits: HashMap<PathBuf, BTreeSet<Commit>>,
}

impl VolatilityAnalyzer {
    /// Create a new analyzer with the given configuration.
    #[must_use]
    pub fn new(config: VolatilityConfig) -> Self {
        Self {
            config,
            repo_root: PathBuf::new(),
            commits: HashMap::new(),
        }
    }

    /// Analyze git history for a repository.
    ///
    /// Streams `git log` output once, combining commit hash, commit time and
    /// changed `.rs` files, and resolves `repo_path` to the repository's
    /// top-level directory via `git rev-parse --show-toplevel`. Uses
    /// `--diff-filter=AMRC` to skip deleted files and a 64KB buffered reader.
    #[allow(clippy::missing_errors_doc)]
    pub fn analyze(&mut self, repo_path: &Path) -> Result<(), VolatilityError> {
        let toplevel = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(repo_path)
            .stderr(Stdio::null())
            .output()?;

        if !toplevel.status.success() {
            return Err(VolatilityError::NotGitRepo);
        }
        self.repo_root = PathBuf::from(String::from_utf8_lossy(&toplevel.stdout).trim());
        self.commits.clear();

        // Stream git log output; a null-prefixed header line starts each commit,
        // carrying its hash and commit time ahead of --name-only's file list.
        let mut child = Command::new("git")
            .args([
                "log",
                "--pretty=format:%x00%H%x09%ct",
                "--name-only",
                "--diff-filter=AMRC",
                &format!("--since={} months ago", self.config.months),
                "--",
                "*.rs",
            ])
            .current_dir(repo_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::with_capacity(64 * 1024, stdout);
            let mut current: Option<Commit> = None;

            for line in reader.lines() {
                let Ok(line) = line else { continue };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if let Some(header) = trimmed.strip_prefix('\0') {
                    current = header.split_once('\t').and_then(|(hash, ts)| {
                        ts.parse().ok().map(|timestamp| Commit {
                            hash: hash.to_string(),
                            timestamp,
                        })
                    });
                    continue;
                }

                let Some(commit) = &current else { continue };
                if Path::new(trimmed)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
                {
                    let path = self.repo_root.join(trimmed);
                    self.commits.entry(path).or_default().insert(commit.clone());
                }
            }
        }

        child.wait()?;

        Ok(())
    }

    /// The commits that touched `file`, keyed by its absolute path.
    ///
    /// Returns an empty set for a file with no recorded commits.
    #[must_use]
    pub fn commits(&self, file: &Path) -> &BTreeSet<Commit> {
        static EMPTY: BTreeSet<Commit> = BTreeSet::new();
        self.commits.get(file).unwrap_or(&EMPTY)
    }

    /// The display path for a tracked file: `path` with the repository root
    /// stripped, as git printed it. Unchanged if `path` does not start with
    /// the root.
    fn display_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.repo_root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// Get the volatility level for a file path.
    ///
    /// `file_path` is interpreted relative to the repository root.
    #[must_use]
    pub fn get_volatility(&self, file_path: &str) -> Volatility {
        Volatility::from_count(self.get_change_count(file_path), &self.config)
    }

    /// Get the raw change count for a file path.
    ///
    /// `file_path` is interpreted relative to the repository root.
    #[must_use]
    pub fn get_change_count(&self, file_path: &str) -> usize {
        self.commits
            .get(&self.repo_root.join(file_path))
            .map_or(0, BTreeSet::len)
    }

    /// Each tracked file's display path and its change count.
    fn path_counts(&self) -> Vec<(String, usize)> {
        self.commits
            .iter()
            .map(|(path, set)| (self.display_path(path), set.len()))
            .collect()
    }

    /// Normalized volatility scores (0.0 = no changes, 1.0 = most-changed file).
    ///
    /// Each file's score is `change_count / max_change_count`. Returns an empty
    /// map when no file changes have been recorded.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // file change counts stay well below 2^52
    pub fn normalized_scores(&self) -> HashMap<String, f64> {
        let max = self.commits.values().map(BTreeSet::len).max().unwrap_or(0);
        if max == 0 {
            return HashMap::new();
        }
        self.path_counts()
            .into_iter()
            .map(|(path, count)| (path, count as f64 / max as f64))
            .collect()
    }

    /// Format a human-readable volatility report.
    #[must_use]
    pub fn format_report(&self) -> String {
        if self.commits.is_empty() {
            return format!(
                "No .rs file changes in the last {} months.\n",
                self.config.months
            );
        }

        let stats = self.statistics();
        let mut out = String::new();

        // Header
        let _ = writeln!(
            out,
            "Volatility (last {} months, {} files):",
            self.config.months, stats.total_files
        );

        // Distribution summary
        let _ = writeln!(
            out,
            "  High: {}  Medium: {}  Low: {}",
            stats.high_volatility_count, stats.medium_volatility_count, stats.low_volatility_count
        );
        out.push('\n');

        // Sorted file list (descending by change count)
        let scores = self.normalized_scores();
        let mut files = self.path_counts();
        files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        for (path, count) in &files {
            let level = Volatility::from_count(*count, &self.config);
            let score = scores.get(path).copied().unwrap_or(0.0);
            let _ = writeln!(out, "  {path}  {count}  {score:.2}  {level}");
        }

        out
    }

    /// Compute aggregate statistics across all tracked files.
    #[must_use]
    pub fn statistics(&self) -> VolatilityStats {
        if self.commits.is_empty() {
            return VolatilityStats::default();
        }

        let counts: Vec<usize> = self.commits.values().map(BTreeSet::len).collect();
        let total_changes: usize = counts.iter().sum();
        let max_changes = counts.iter().max().copied().unwrap_or(0);
        let min_changes = counts.iter().min().copied().unwrap_or(0);
        #[allow(clippy::cast_precision_loss)] // file change counts stay well below 2^52
        let avg_changes = total_changes as f64 / counts.len() as f64;

        let low_count = counts
            .iter()
            .filter(|&&c| c <= self.config.low_threshold)
            .count();
        let high_count = counts
            .iter()
            .filter(|&&c| c > self.config.high_threshold)
            .count();
        let medium_count = counts.len() - low_count - high_count;

        VolatilityStats {
            total_files: counts.len(),
            total_changes,
            max_changes,
            min_changes,
            avg_changes,
            low_volatility_count: low_count,
            medium_volatility_count: medium_count,
            high_volatility_count: high_count,
        }
    }
}

/// Aggregate statistics about volatility across a project.
#[derive(Debug, Default)]
pub struct VolatilityStats {
    pub total_files: usize,
    pub total_changes: usize,
    pub max_changes: usize,
    pub min_changes: usize,
    pub avg_changes: f64,
    pub low_volatility_count: usize,
    pub medium_volatility_count: usize,
    pub high_volatility_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a git subcommand in `dir`, failing the test on a non-zero exit.
    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git should be on PATH");
        assert!(status.success(), "git {args:?} failed in {}", dir.display());
    }

    /// A fresh repository with a local identity, so commits succeed without
    /// relying on the environment's global git config.
    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    /// Writes `relative` under `repo_dir` and commits it, returning the new commit's hash.
    fn commit_file(repo_dir: &Path, relative: &str, contents: &str) -> String {
        let path = repo_dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, contents).unwrap();
        git(repo_dir, &["add", relative]);
        git(repo_dir, &["commit", "-q", "-m", "test commit"]);
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_dir)
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// `n` distinct commits with fabricated hashes, for tests that only care
    /// about the count.
    fn commits_of(n: usize) -> BTreeSet<Commit> {
        (0..n)
            .map(|i| Commit {
                hash: format!("commit{i}"),
                timestamp: i64::try_from(i).unwrap(),
            })
            .collect()
    }

    #[test]
    fn test_commits_known_file_has_hash_and_timestamp() {
        let repo = init_repo();
        let hash = commit_file(repo.path(), "src/lib.rs", "fn main() {}");

        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.analyze(repo.path()).unwrap();

        let file = repo.path().canonicalize().unwrap().join("src/lib.rs");
        let commits: Vec<&Commit> = analyzer.commits(&file).iter().collect();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, hash);
        assert!(commits[0].timestamp > 0);
    }

    #[test]
    fn test_commits_unknown_file_is_empty() {
        let repo = init_repo();
        commit_file(repo.path(), "src/lib.rs", "fn main() {}");

        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.analyze(repo.path()).unwrap();

        let file = repo
            .path()
            .canonicalize()
            .unwrap()
            .join("src/never_committed.rs");
        assert!(analyzer.commits(&file).is_empty());
    }

    #[test]
    fn test_commits_for_workspace_in_subdirectory() {
        let repo = init_repo();
        commit_file(repo.path(), "workspace/src/lib.rs", "fn main() {}");

        let workspace_root = repo.path().join("workspace");
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.analyze(&workspace_root).unwrap();

        let file = repo
            .path()
            .canonicalize()
            .unwrap()
            .join("workspace/src/lib.rs");
        assert_eq!(analyzer.commits(&file).len(), 1);
    }

    #[test]
    fn test_volatility_config_default() {
        let config = VolatilityConfig::default();
        assert_eq!(config.months, 6);
        assert_eq!(config.low_threshold, 2);
        assert_eq!(config.high_threshold, 10);
    }

    #[test]
    fn test_volatility_from_count_default_thresholds() {
        let config = VolatilityConfig::default();
        // Boundary: 0-2 = Low
        assert_eq!(Volatility::from_count(0, &config), Volatility::Low);
        assert_eq!(Volatility::from_count(1, &config), Volatility::Low);
        assert_eq!(Volatility::from_count(2, &config), Volatility::Low);
        // Boundary: 3-10 = Medium
        assert_eq!(Volatility::from_count(3, &config), Volatility::Medium);
        assert_eq!(Volatility::from_count(10, &config), Volatility::Medium);
        // Boundary: 11+ = High
        assert_eq!(Volatility::from_count(11, &config), Volatility::High);
        assert_eq!(Volatility::from_count(100, &config), Volatility::High);
    }

    #[test]
    fn test_volatility_from_count_custom_thresholds() {
        let config = VolatilityConfig {
            months: 3,
            low_threshold: 1,
            high_threshold: 5,
        };
        assert_eq!(Volatility::from_count(0, &config), Volatility::Low);
        assert_eq!(Volatility::from_count(1, &config), Volatility::Low);
        assert_eq!(Volatility::from_count(2, &config), Volatility::Medium);
        assert_eq!(Volatility::from_count(5, &config), Volatility::Medium);
        assert_eq!(Volatility::from_count(6, &config), Volatility::High);
    }

    #[test]
    fn test_analyzer_new() {
        let analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        assert!(analyzer.commits.is_empty());
        assert_eq!(analyzer.config.months, 6);
    }

    #[test]
    fn test_volatility_error_display() {
        let err = VolatilityError::NotGitRepo;
        assert_eq!(err.to_string(), "Not a git repository");
    }

    #[test]
    fn test_volatility_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "git not found");
        let vol_err: VolatilityError = io_err.into();
        assert!(matches!(vol_err, VolatilityError::GitCommand(_)));
        assert!(vol_err.to_string().contains("git not found"));
    }

    #[test]
    fn test_analyze_real_repo() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        let result = analyzer.analyze(repo);
        assert!(result.is_ok());
        assert!(!analyzer.commits.is_empty());
    }

    #[test]
    fn test_analyze_not_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        let result = analyzer.analyze(tmp.path());
        assert!(matches!(result, Err(VolatilityError::NotGitRepo)));
    }

    #[test]
    fn test_analyze_empty_history() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let config = VolatilityConfig {
            months: 0,
            ..VolatilityConfig::default()
        };
        let mut analyzer = VolatilityAnalyzer::new(config);
        let result = analyzer.analyze(repo);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_volatility_known_file() {
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.repo_root = PathBuf::from("/repo");
        analyzer
            .commits
            .insert(analyzer.repo_root.join("low.rs"), commits_of(1));
        analyzer
            .commits
            .insert(analyzer.repo_root.join("med.rs"), commits_of(5));
        analyzer
            .commits
            .insert(analyzer.repo_root.join("high.rs"), commits_of(15));

        assert_eq!(analyzer.get_volatility("low.rs"), Volatility::Low);
        assert_eq!(analyzer.get_volatility("med.rs"), Volatility::Medium);
        assert_eq!(analyzer.get_volatility("high.rs"), Volatility::High);
    }

    #[test]
    fn test_get_volatility_unknown_file() {
        let analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        // 0 commits → Low
        assert_eq!(analyzer.get_volatility("nonexistent.rs"), Volatility::Low);
    }

    #[test]
    fn test_get_change_count() {
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.repo_root = PathBuf::from("/repo");
        analyzer
            .commits
            .insert(analyzer.repo_root.join("known.rs"), commits_of(42));

        assert_eq!(analyzer.get_change_count("known.rs"), 42);
        assert_eq!(analyzer.get_change_count("unknown.rs"), 0);
    }

    #[test]
    fn test_statistics_with_data() {
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.repo_root = PathBuf::from("/repo");
        analyzer
            .commits
            .insert(analyzer.repo_root.join("a.rs"), commits_of(1)); // Low
        analyzer
            .commits
            .insert(analyzer.repo_root.join("b.rs"), commits_of(5)); // Medium
        analyzer
            .commits
            .insert(analyzer.repo_root.join("c.rs"), commits_of(15)); // High

        let stats = analyzer.statistics();
        assert_eq!(stats.total_files, 3);
        assert_eq!(stats.total_changes, 21);
        assert_eq!(stats.max_changes, 15);
        assert_eq!(stats.min_changes, 1);
        assert!((stats.avg_changes - 7.0).abs() < f64::EPSILON);
        assert_eq!(stats.low_volatility_count, 1);
        assert_eq!(stats.medium_volatility_count, 1);
        assert_eq!(stats.high_volatility_count, 1);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "the empty case returns Default, so 0.0 is a literal, not a quotient"
    )]
    fn test_statistics_empty() {
        let analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        let stats = analyzer.statistics();

        assert_eq!(stats.total_files, 0);
        assert_eq!(stats.total_changes, 0);
        assert_eq!(stats.max_changes, 0);
        assert_eq!(stats.min_changes, 0);
        assert_eq!(stats.avg_changes, 0.0);
        assert_eq!(stats.low_volatility_count, 0);
        assert_eq!(stats.medium_volatility_count, 0);
        assert_eq!(stats.high_volatility_count, 0);
    }

    #[test]
    fn test_format_report() {
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.repo_root = PathBuf::from("/repo");
        analyzer
            .commits
            .insert(analyzer.repo_root.join("src/hot.rs"), commits_of(15));
        analyzer
            .commits
            .insert(analyzer.repo_root.join("src/warm.rs"), commits_of(5));
        analyzer
            .commits
            .insert(analyzer.repo_root.join("src/cold.rs"), commits_of(1));

        let report = analyzer.format_report();

        assert!(report.contains("last 6 months, 3 files"));
        assert!(report.contains("High: 1"));
        assert!(report.contains("Medium: 1"));
        assert!(report.contains("Low: 1"));
        // First file listed should be the hottest
        let hot_pos = report.find("src/hot.rs").unwrap();
        let cold_pos = report.find("src/cold.rs").unwrap();
        assert!(hot_pos < cold_pos);
        assert!(report.contains("src/hot.rs  15  1.00  HIGH"));
        assert!(report.contains("src/warm.rs  5  0.33  MEDIUM"));
        assert!(report.contains("src/cold.rs  1  0.07  LOW"));
    }

    #[test]
    fn test_normalized_scores() {
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.repo_root = PathBuf::from("/repo");
        analyzer
            .commits
            .insert(analyzer.repo_root.join("a.rs"), commits_of(10));
        analyzer
            .commits
            .insert(analyzer.repo_root.join("b.rs"), commits_of(5));
        analyzer
            .commits
            .insert(analyzer.repo_root.join("c.rs"), commits_of(0));

        let scores = analyzer.normalized_scores();
        assert!((scores["a.rs"] - 1.0).abs() < f64::EPSILON);
        assert!((scores["b.rs"] - 0.5).abs() < f64::EPSILON);
        assert!((scores["c.rs"] - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_normalized_scores_empty() {
        let analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        assert!(analyzer.normalized_scores().is_empty());
    }

    #[test]
    fn test_format_report_empty() {
        let analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        let report = analyzer.format_report();
        assert_eq!(report, "No .rs file changes in the last 6 months.\n");
    }

    #[test]
    fn test_analyze_clears_commits_from_previous_repository() {
        let repo_a = init_repo();
        commit_file(repo_a.path(), "a.rs", "fn a() {}");
        let repo_b = init_repo();
        commit_file(repo_b.path(), "b.rs", "fn b() {}");

        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.analyze(repo_a.path()).unwrap();
        analyzer.analyze(repo_b.path()).unwrap();

        assert_eq!(analyzer.statistics().total_files, 1);
    }

    #[test]
    fn test_commits_are_ordered_by_time_not_hash() {
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.repo_root = PathBuf::from("/repo");
        let file = analyzer.repo_root.join("a.rs");
        analyzer.commits.insert(
            file.clone(),
            BTreeSet::from([
                Commit {
                    hash: "zzz".into(),
                    timestamp: 1,
                },
                Commit {
                    hash: "aaa".into(),
                    timestamp: 2,
                },
            ]),
        );

        let ordered: Vec<&str> = analyzer
            .commits(&file)
            .iter()
            .map(|c| c.hash.as_str())
            .collect();

        assert_eq!(ordered, vec!["zzz", "aaa"]);
    }
}
