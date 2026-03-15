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

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Rule {
    ForbiddenDependency {
        name: String,
        from: String,
        to: String,
        #[serde(default = "default_severity")]
        severity: Severity,
    },
    NoCycles {
        name: String,
        scope: String,
        #[serde(default = "default_severity")]
        severity: Severity,
    },
    Layers {
        name: String,
        layers: Vec<String>,
        direction: Direction,
        #[serde(default = "default_severity")]
        severity: Severity,
    },
}

impl Rule {
    fn severity_mut(&mut self) -> &mut Severity {
        match self {
            Rule::ForbiddenDependency { severity, .. }
            | Rule::NoCycles { severity, .. }
            | Rule::Layers { severity, .. } => severity,
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
            let sev = rule.severity_mut();
            if *sev == Severity::Error && default != Severity::Error {
                *sev = default;
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
        assert!(matches!(
            &config.rules[0],
            Rule::ForbiddenDependency { name, from, to, .. }
            if name == "no infra in domain" && from == "domain::**" && to == "infra::**"
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
        assert!(matches!(
            &config.rules[0],
            Rule::NoCycles { name, scope, .. }
            if name == "domain acyclic" && scope == "domain::**"
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
        assert!(matches!(
            &config.rules[0],
            Rule::Layers { name, layers, direction, .. }
            if name == "architecture layers"
                && layers == &["domain", "application", "infra"]
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
        match &config.rules[0] {
            Rule::NoCycles { severity, .. } => assert_eq!(*severity, Severity::Error),
            other => panic!("expected NoCycles, got {other:?}"),
        }
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
        match &config.rules[0] {
            Rule::NoCycles { severity, .. } => assert_eq!(*severity, Severity::Warn),
            other => panic!("expected NoCycles, got {other:?}"),
        }
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
        assert!(matches!(&config.rules[0], Rule::ForbiddenDependency { .. }));
        assert!(matches!(&config.rules[1], Rule::NoCycles { .. }));
        assert!(matches!(&config.rules[2], Rule::Layers { .. }));
    }
}
