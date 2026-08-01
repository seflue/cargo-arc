//! Config parsing for arc-rules.toml

use serde::Deserialize;
use std::path::{Path, PathBuf};

fn default_severity() -> Severity {
    Severity::Error
}

#[derive(Debug, Deserialize)]
pub struct ArcConfig {
    pub config: Option<ConfigMeta>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigMeta {
    pub version: u32,
    #[serde(default = "default_severity")]
    pub default_severity: Severity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    TopDown,
    BottomUp,
}

/// A permanently allowed edge for the rule it is declared on.
///
/// Scoped to its rule rather than a shared section: the exception lives and
/// dies with the rule it applies to.
#[derive(Debug, Deserialize)]
pub struct Except {
    pub from: String,
    pub to: String,
    // Documentation only (cargo-deny style), never evaluated.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    pub name: String,
    #[serde(default = "default_severity")]
    pub severity: Severity,
    #[serde(default)]
    pub except: Vec<Except>,
    #[serde(flatten)]
    pub kind: RuleKind,
}

#[derive(Debug, Deserialize)]
pub struct ForbiddenDependencyRule {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
pub struct NoCyclesRule {
    pub scope: String,
}

#[derive(Debug, Deserialize)]
pub struct LayersRule {
    pub layers: Vec<String>,
    pub direction: Direction,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RuleKind {
    ForbiddenDependency(ForbiddenDependencyRule),
    NoCycles(NoCyclesRule),
    Layers(LayersRule),
}

impl Rule {
    #[must_use]
    pub fn rule_type(&self) -> &'static str {
        match self.kind {
            RuleKind::ForbiddenDependency(_) => "forbidden-dependency",
            RuleKind::NoCycles(_) => "no-cycles",
            RuleKind::Layers(_) => "layers",
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    FileNotFound(PathBuf),
    IoError(PathBuf, std::io::Error),
    ParseError(PathBuf, toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotFound(path) => write!(f, "config file not found: {}", path.display()),
            Self::IoError(path, err) => {
                write!(f, "cannot read config file {}: {err}", path.display())
            }
            Self::ParseError(path, err) => {
                write!(f, "invalid config file {}: {err}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl ArcConfig {
    /// # Errors
    /// Returns `ConfigError::FileNotFound` if the path does not exist,
    /// `ConfigError::IoError` for other I/O failures, or
    /// `ConfigError::ParseError` for invalid TOML.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::FileNotFound(path.to_path_buf())
            } else {
                ConfigError::IoError(path.to_path_buf(), e)
            }
        })?;
        let mut config: Self =
            toml::from_str(&content).map_err(|e| ConfigError::ParseError(path.to_path_buf(), e))?;
        config.apply_defaults();
        Ok(config)
    }

    /// Replace serde-default severity values with `config.default_severity`
    /// when the rule's severity was not explicitly set.
    ///
    /// Since serde defaults fire at parse time, we use a post-deserialization
    /// pass: if severity == Error (the serde default) and `config.default_severity`
    /// differs, override it. Rules with explicit `severity = "error"` are
    /// indistinguishable but get the same value anyway.
    pub fn apply_defaults(&mut self) {
        let default = self
            .config
            .as_ref()
            .map_or(Severity::Error, |c| c.default_severity);
        for rule in &mut self.rules {
            if rule.severity == Severity::Error && default != Severity::Error {
                rule.severity = default;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_forbidden_dependency() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].name, "no infra in domain");
        assert!(matches!(
            &config.rules[0].kind,
            RuleKind::ForbiddenDependency(ForbiddenDependencyRule { from, to })
            if from == "domain::**" && to == "infra::**"
        ));
    }

    #[test]
    fn test_parse_no_cycles() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "domain acyclic"
            scope = "domain::**"
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.rules[0].name, "domain acyclic");
        assert!(matches!(
            &config.rules[0].kind,
            RuleKind::NoCycles(NoCyclesRule { scope })
            if scope == "domain::**"
        ));
    }

    #[test]
    fn test_parse_layers() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "application", "infra"]
            direction = "top-down"
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.rules[0].name, "architecture layers");
        assert!(matches!(
            &config.rules[0].kind,
            RuleKind::Layers(LayersRule { layers, direction })
            if layers == &["domain", "application", "infra"]
                && *direction == Direction::TopDown
        ));
    }

    #[test]
    fn test_parse_severity_default() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "test"
            scope = "**"
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.rules[0].severity, Severity::Error);
    }

    #[test]
    fn test_config_default_severity() {
        let toml = r#"
            [config]
            version = 1
            default_severity = "warn"

            [[rules]]
            type = "no-cycles"
            name = "test"
            scope = "**"
        "#;
        let mut config: ArcConfig = toml::from_str(toml).unwrap();
        config.apply_defaults();
        assert_eq!(config.rules[0].severity, Severity::Warn);
    }

    #[test]
    fn test_parse_unknown_type() {
        let toml = r#"
            [[rules]]
            type = "unknown-rule"
            name = "test"
        "#;
        let result: Result<ArcConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_missing_file() {
        let result = ArcConfig::load(Path::new("/nonexistent/arc-rules.toml"));
        assert!(matches!(result, Err(ConfigError::FileNotFound(_))));
    }

    #[test]
    fn test_load_io_error() {
        // /proc/1/mem exists but is not readable → IoError, not FileNotFound
        let result = ArcConfig::load(Path::new("/proc/1/mem"));
        assert!(
            matches!(result, Err(ConfigError::IoError(..))),
            "expected IoError, got {result:?}"
        );
    }

    #[test]
    fn test_parse_except() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "app acyclic"
            scope = "app::**"
            except = [
              { from = "app::router", to = "app::screens::**", reason = "router mediates" },
            ]
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        let except = &config.rules[0].except;
        assert_eq!(except.len(), 1);
        assert_eq!(except[0].from, "app::router");
        assert_eq!(except[0].to, "app::screens::**");
        assert_eq!(except[0].reason.as_deref(), Some("router mediates"));
    }

    #[test]
    fn test_parse_except_without_reason() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "app acyclic"
            scope = "app::**"
            except = [
              { from = "app::router", to = "app::screens::**" },
            ]
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        let except = &config.rules[0].except;
        assert_eq!(except.len(), 1);
        assert_eq!(except[0].reason, None);
    }

    #[test]
    fn test_parse_except_defaults_to_empty() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "app acyclic"
            scope = "app::**"
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        assert!(config.rules[0].except.is_empty());
    }

    #[test]
    fn test_parse_except_on_forbidden_dependency_and_layers() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
            except = [
              { from = "domain::legacy", to = "infra::db", reason = "pending migration" },
            ]

            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "application", "infra"]
            direction = "top-down"
            except = [
              { from = "infra::bridge", to = "domain::events" },
            ]
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        let except = &config.rules[0].except;
        assert_eq!(except.len(), 1);
        assert_eq!(except[0].from, "domain::legacy");
        assert_eq!(except[0].to, "infra::db");
        let except = &config.rules[1].except;
        assert_eq!(except.len(), 1);
        assert_eq!(except[0].from, "infra::bridge");
    }

    #[test]
    fn test_parse_full_config() {
        let toml = r#"
            [config]
            version = 1

            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
            severity = "error"

            [[rules]]
            type = "no-cycles"
            name = "domain acyclic"
            scope = "domain::**"
            severity = "warn"

            [[rules]]
            type = "layers"
            name = "architecture"
            layers = ["domain", "application", "infra"]
            direction = "top-down"
            severity = "error"
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.rules.len(), 3);
        assert!(matches!(
            &config.rules[0].kind,
            RuleKind::ForbiddenDependency(_)
        ));
        assert!(matches!(&config.rules[1].kind, RuleKind::NoCycles(_)));
        assert!(matches!(&config.rules[2].kind, RuleKind::Layers(_)));
    }

    #[test]
    fn test_name_and_except_readable_uniformly_across_rule_types() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
            except = [
              { from = "domain::legacy", to = "infra::db" },
            ]

            [[rules]]
            type = "no-cycles"
            name = "domain acyclic"
            scope = "domain::**"

            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "application", "infra"]
            direction = "top-down"
        "#;
        let config: ArcConfig = toml::from_str(toml).unwrap();
        let names: Vec<&str> = config.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "no infra in domain",
                "domain acyclic",
                "architecture layers"
            ]
        );
        let except_lens: Vec<usize> = config.rules.iter().map(|r| r.except.len()).collect();
        assert_eq!(except_lens, [1, 0, 0]);
    }

    #[test]
    fn test_parse_fixture_config() {
        let path = Path::new("tests/fixtures/arch_violation_workspace/arc-rules.toml");
        let config = ArcConfig::load(path).unwrap();
        assert_eq!(config.rules.len(), 3);
        assert_eq!(config.rules[0].name, "no infra in domain");
        assert_eq!(config.rules[1].name, "architecture layers");
        assert_eq!(config.rules[2].name, "no cycles in domain");
        assert!(config.rules.iter().all(|r| r.except.is_empty()));
        match &config.rules[1].kind {
            RuleKind::Layers(LayersRule { direction, .. }) => {
                assert_eq!(*direction, Direction::TopDown);
            }
            other => panic!("expected Layers, got {other:?}"),
        }
    }
}
