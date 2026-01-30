//! Git history analysis for code volatility measurement.
//!
//! Analyzes git log to determine how frequently Rust source files change.
//! Supports configurable thresholds and time periods.
//! Optimized for large repositories using streaming and git-level path filtering.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
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
    pub fn from_count(count: usize, config: &VolatilityConfig) -> Self {
        if count <= config.low_threshold {
            Self::Low
        } else if count <= config.high_threshold {
            Self::Medium
        } else {
            Self::High
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
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

/// Volatility analyzer using git history.
pub struct VolatilityAnalyzer {
    config: VolatilityConfig,
    file_changes: HashMap<String, usize>,
}

impl VolatilityAnalyzer {
    /// Create a new analyzer with the given configuration.
    pub fn new(config: VolatilityConfig) -> Self {
        Self {
            config,
            file_changes: HashMap::new(),
        }
    }

    /// Analyze git history for a repository.
    ///
    /// Streams `git log` output to count per-file change frequency for `.rs` files.
    /// Uses `--diff-filter=AMRC` to skip deleted files and a 64KB buffered reader.
    pub fn analyze(&mut self, repo_path: &Path) -> Result<(), VolatilityError> {
        // Check if it's a git repo
        let git_check = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(repo_path)
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .status()?;

        if !git_check.success() {
            return Err(VolatilityError::NotGitRepo);
        }

        // Stream git log output
        let mut child = Command::new("git")
            .args([
                "log",
                "--pretty=format:",
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

            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => continue,
                };

                let trimmed = line.trim();
                if !trimmed.is_empty() && trimmed.ends_with(".rs") {
                    *self.file_changes.entry(trimmed.to_string()).or_insert(0) += 1;
                }
            }
        }

        child.wait()?;

        Ok(())
    }

    /// Get the volatility level for a file path.
    pub fn get_volatility(&self, file_path: &str) -> Volatility {
        let count = self.file_changes.get(file_path).copied().unwrap_or(0);
        Volatility::from_count(count, &self.config)
    }

    /// Get the raw change count for a file path.
    pub fn get_change_count(&self, file_path: &str) -> usize {
        self.file_changes.get(file_path).copied().unwrap_or(0)
    }

    /// Normalized volatility scores (0.0 = no changes, 1.0 = most-changed file).
    ///
    /// Each file's score is `change_count / max_change_count`. Returns an empty
    /// map when no file changes have been recorded.
    pub fn normalized_scores(&self) -> HashMap<String, f64> {
        let max = self.file_changes.values().max().copied().unwrap_or(0);
        if max == 0 {
            return HashMap::new();
        }
        self.file_changes
            .iter()
            .map(|(path, &count)| (path.clone(), count as f64 / max as f64))
            .collect()
    }

    /// Format a human-readable volatility report.
    pub fn format_report(&self) -> String {
        if self.file_changes.is_empty() {
            return format!(
                "No .rs file changes in the last {} months.\n",
                self.config.months
            );
        }

        let stats = self.statistics();
        let mut out = String::new();

        // Header
        out.push_str(&format!(
            "Volatility (last {} months, {} files):\n",
            self.config.months, stats.total_files
        ));

        // Distribution summary
        out.push_str(&format!(
            "  High: {}  Medium: {}  Low: {}\n",
            stats.high_volatility_count, stats.medium_volatility_count, stats.low_volatility_count
        ));
        out.push('\n');

        // Sorted file list (descending by change count)
        let scores = self.normalized_scores();
        let mut files: Vec<_> = self.file_changes.iter().collect();
        files.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

        for (path, count) in &files {
            let level = Volatility::from_count(**count, &self.config);
            let score = scores.get(*path).copied().unwrap_or(0.0);
            out.push_str(&format!(
                "  {path}  {}  {:.2}  {}\n",
                count,
                score,
                level.label()
            ));
        }

        out
    }

    /// Compute aggregate statistics across all tracked files.
    pub fn statistics(&self) -> VolatilityStats {
        if self.file_changes.is_empty() {
            return VolatilityStats::default();
        }

        let counts: Vec<usize> = self.file_changes.values().copied().collect();
        let total_changes: usize = counts.iter().sum();
        let max_changes = counts.iter().max().copied().unwrap_or(0);
        let min_changes = counts.iter().min().copied().unwrap_or(0);
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
        assert!(analyzer.file_changes.is_empty());
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
        assert!(!analyzer.file_changes.is_empty());
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
        analyzer.file_changes.insert("low.rs".into(), 1);
        analyzer.file_changes.insert("med.rs".into(), 5);
        analyzer.file_changes.insert("high.rs".into(), 15);

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
        analyzer.file_changes.insert("known.rs".into(), 42);

        assert_eq!(analyzer.get_change_count("known.rs"), 42);
        assert_eq!(analyzer.get_change_count("unknown.rs"), 0);
    }

    #[test]
    fn test_statistics_with_data() {
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.file_changes.insert("a.rs".into(), 1); // Low
        analyzer.file_changes.insert("b.rs".into(), 5); // Medium
        analyzer.file_changes.insert("c.rs".into(), 15); // High

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
        analyzer.file_changes.insert("src/hot.rs".into(), 15);
        analyzer.file_changes.insert("src/warm.rs".into(), 5);
        analyzer.file_changes.insert("src/cold.rs".into(), 1);

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
        analyzer.file_changes.insert("a.rs".into(), 10);
        analyzer.file_changes.insert("b.rs".into(), 5);
        analyzer.file_changes.insert("c.rs".into(), 0);

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
}
