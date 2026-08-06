//! Config parsing for arc-rules.toml

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ArcConfig {
    pub rules: Vec<Rule>,
    pub diagnostics: Diagnostics,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    config: Option<ConfigMeta>,
    #[serde(default)]
    rules: Vec<RawRule>,
    #[serde(default)]
    diagnostics: Diagnostics,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigMeta {
    pub version: u32,
    #[serde(default)]
    pub default_severity: Severity,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Warn,
    Ignore,
}

/// Separate from `Severity` because the two qualify different objects: a rule
/// name is an intent, and `severity` says how bad breaking it is; a diagnostic
/// name is a state, and this says whether that state is allowed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Allow,
    #[default]
    Warn,
    Deny,
}

/// The `unlayered-crate` diagnostic: its level plus the crates that are
/// deliberately outside the architecture (build tooling, examples). The list
/// is the only way to tell *forgotten* from *deliberately outside*; `except`
/// on a rule does not reach diagnostics.
///
/// Written either as a bare level (`"warn"`) or as a table
/// (`{ level = "warn", except = ["xtask"] }`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UnlayeredCrate {
    pub level: DiagnosticLevel,
    pub except: Vec<String>,
}

impl<'de> Deserialize<'de> for UnlayeredCrate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `#[serde(untagged)]` would only report "data did not match any
        // variant". Deserializing the table arm as its own
        // `deny_unknown_fields` struct names the offending key instead.
        struct UnlayeredCrateVisitor;

        impl<'de> serde::de::Visitor<'de> for UnlayeredCrateVisitor {
            type Value = UnlayeredCrate;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a diagnostic level string, or a table with level/except")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let level =
                    DiagnosticLevel::deserialize(serde::de::value::StrDeserializer::new(v))?;
                Ok(UnlayeredCrate {
                    level,
                    except: Vec::new(),
                })
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Table {
                    #[serde(default)]
                    level: DiagnosticLevel,
                    #[serde(default)]
                    except: Vec<String>,
                }
                let table = Table::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(UnlayeredCrate {
                    level: table.level,
                    except: table.except,
                })
            }
        }

        deserializer.deserialize_any(UnlayeredCrateVisitor)
    }
}

/// A mistyped name is rejected rather than ignored: a diagnostic that silently
/// stays off is the state this section exists to end.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Diagnostics {
    #[serde(default)]
    pub unlayered_crate: UnlayeredCrate,
    #[serde(default)]
    pub unmatched_baseline_entry: DiagnosticLevel,
    #[serde(default)]
    pub unmatched_except: DiagnosticLevel,
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
#[serde(deny_unknown_fields)]
pub struct Except {
    pub from: String,
    pub to: String,
    // Documentation only (cargo-deny style), never evaluated.
    pub reason: Option<String>,
}

/// `flatten` hands every key this struct does not declare — `type` and the
/// rule parameters — to `RuleKind`, whose variants reject the unknown ones.
#[derive(Debug, Deserialize)]
struct RawRule {
    name: String,
    #[serde(default, rename = "severity")]
    severity: Option<Severity>,
    #[serde(default)]
    except: Vec<Except>,
    #[serde(flatten)]
    kind: RuleKind,
}

#[derive(Debug)]
pub struct Rule {
    pub name: String,
    pub severity: Severity,
    pub except: Vec<Except>,
    pub kind: RuleKind,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForbiddenDependencyRule {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoCyclesRule {
    pub scope: String,
}

/// One rank in a `layers` rule: the patterns whose nodes share a position.
///
/// Several patterns per rank exist because a rank is not always one crate.
/// Written either as a bare pattern (`"domain"`) or as a list
/// (`["adapter_a", "adapter_b"]`); listing equals separately would make the
/// list order assert a ranking between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer(Vec<String>);

impl Layer {
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.0
    }
}

impl From<&str> for Layer {
    fn from(pattern: &str) -> Self {
        Self(vec![pattern.to_owned()])
    }
}

impl FromIterator<String> for Layer {
    fn from_iter<I: IntoIterator<Item = String>>(patterns: I) -> Self {
        Self(patterns.into_iter().collect())
    }
}

impl<'de> Deserialize<'de> for Layer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LayerVisitor;

        impl<'de> serde::de::Visitor<'de> for LayerVisitor {
            type Value = Layer;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a module path pattern, or a list of them")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Layer::from(v))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut patterns = Vec::new();
                while let Some(pattern) = seq.next_element::<String>()? {
                    patterns.push(pattern);
                }
                Ok(patterns.into_iter().collect())
            }
        }

        deserializer.deserialize_any(LayerVisitor)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayersRule {
    pub layers: Vec<Layer>,
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
    DuplicateRuleName { path: PathBuf, name: String },
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
            Self::DuplicateRuleName { path, name } => write!(
                f,
                "duplicate rule name {name:?} in {}: rule names must be unique",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl ArcConfig {
    /// # Errors
    /// Returns `ConfigError::FileNotFound` if the path does not exist,
    /// `ConfigError::IoError` for other I/O failures,
    /// `ConfigError::ParseError` for invalid TOML, or
    /// `ConfigError::DuplicateRuleName` if two rules share a name.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::FileNotFound(path.to_path_buf())
            } else {
                ConfigError::IoError(path.to_path_buf(), e)
            }
        })?;
        let config = Self::from_toml(&content)
            .map_err(|e| ConfigError::ParseError(path.to_path_buf(), e))?;
        config.check_unique_rule_names(path)?;
        Ok(config)
    }

    /// Rule names must be unique across all rule types: a baseline entry
    /// carries the rule name and no type, so a duplicate makes it ambiguous
    /// which rule the frozen violation belongs to.
    fn check_unique_rule_names(&self, path: &Path) -> Result<(), ConfigError> {
        let mut seen = std::collections::HashSet::new();
        for rule in &self.rules {
            if !seen.insert(rule.name.as_str()) {
                return Err(ConfigError::DuplicateRuleName {
                    path: path.to_path_buf(),
                    name: rule.name.clone(),
                });
            }
        }
        Ok(())
    }

    /// Fills in `config.default_severity` for rules that left `severity` unset.
    fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        let raw: RawConfig = toml::from_str(content)?;
        let default = raw
            .config
            .map(|meta| meta.default_severity)
            .unwrap_or_default();
        let rules = raw
            .rules
            .into_iter()
            .map(|rule| Rule {
                name: rule.name,
                severity: rule.severity.unwrap_or(default),
                except: rule.except,
                kind: rule.kind,
            })
            .collect();
        Ok(Self {
            rules,
            diagnostics: raw.diagnostics,
        })
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
        let config = ArcConfig::from_toml(toml).unwrap();
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
        let config = ArcConfig::from_toml(toml).unwrap();
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
        let config = ArcConfig::from_toml(toml).unwrap();
        assert_eq!(config.rules[0].name, "architecture layers");
        let RuleKind::Layers(LayersRule { layers, direction }) = &config.rules[0].kind else {
            panic!("expected Layers, got {:?}", config.rules[0].kind);
        };
        let patterns: Vec<&[String]> = layers.iter().map(Layer::patterns).collect();
        assert_eq!(
            patterns,
            [
                &["domain".to_string()][..],
                &["application".to_string()][..],
                &["infra".to_string()][..],
            ]
        );
        assert_eq!(*direction, Direction::TopDown);
    }

    /// Crates of equal rank share one entry. Without this they need one entry
    /// each, and the list is ordered, so the order asserts a ranking the
    /// architecture does not have.
    #[test]
    fn test_parse_layers_with_several_patterns_in_one_entry() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", ["adapter_a", "adapter_b"], "runtime"]
            direction = "bottom-up"
        "#;
        let config = ArcConfig::from_toml(toml).unwrap();
        let RuleKind::Layers(LayersRule { layers, .. }) = &config.rules[0].kind else {
            panic!("expected Layers, got {:?}", config.rules[0].kind);
        };
        let patterns: Vec<&[String]> = layers.iter().map(Layer::patterns).collect();
        assert_eq!(
            patterns,
            [
                &["domain".to_string()][..],
                &["adapter_a".to_string(), "adapter_b".to_string()][..],
                &["runtime".to_string()][..],
            ]
        );
    }

    #[test]
    fn test_severity_defaults_to_error() {
        assert_eq!(Severity::default(), Severity::Error);
    }

    #[test]
    fn test_parse_severity_default() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "test"
            scope = "**"
        "#;
        let config = ArcConfig::from_toml(toml).unwrap();
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
        let config = ArcConfig::from_toml(toml).unwrap();
        assert_eq!(config.rules[0].severity, Severity::Warn);
    }

    #[test]
    fn test_config_meta_without_default_severity_falls_back_to_error() {
        let toml = r#"
            [config]
            version = 1

            [[rules]]
            type = "no-cycles"
            name = "test"
            scope = "**"
        "#;
        let config = ArcConfig::from_toml(toml).unwrap();
        assert_eq!(config.rules[0].severity, Severity::Error);
    }

    #[test]
    fn test_config_default_severity_does_not_override_explicit_severity() {
        let toml = r#"
            [config]
            version = 1
            default_severity = "warn"

            [[rules]]
            type = "no-cycles"
            name = "test"
            scope = "**"
            severity = "error"
        "#;
        let config = ArcConfig::from_toml(toml).unwrap();
        assert_eq!(config.rules[0].severity, Severity::Error);
    }

    #[test]
    fn test_parse_unknown_type() {
        let toml = r#"
            [[rules]]
            type = "unknown-rule"
            name = "test"
        "#;
        let result = ArcConfig::from_toml(toml);
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
        let config = ArcConfig::from_toml(toml).unwrap();
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
        let config = ArcConfig::from_toml(toml).unwrap();
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
        let config = ArcConfig::from_toml(toml).unwrap();
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
        let config = ArcConfig::from_toml(toml).unwrap();
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
        let config = ArcConfig::from_toml(toml).unwrap();
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
        let config = ArcConfig::from_toml(toml).unwrap();
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
    fn test_load_rejects_duplicate_rule_name_across_types() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "shared name"
            from = "domain::**"
            to = "infra::**"

            [[rules]]
            type = "no-cycles"
            name = "shared name"
            scope = "domain::**"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::DuplicateRuleName { path: err_path, name }
                if err_path == &path && name == "shared name"),
            "expected DuplicateRuleName, got {error:?}"
        );
        assert!(error.to_string().contains("shared name"));
    }

    #[test]
    fn test_load_accepts_distinct_rule_names() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"

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
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let config = ArcConfig::load(&path).unwrap();
        assert_eq!(config.rules.len(), 3);
    }

    #[test]
    fn test_diagnostics_missing_section_defaults_to_warn() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "test"
            scope = "**"
        "#;
        let config = ArcConfig::from_toml(toml).unwrap();
        let diagnostics = &config.diagnostics;
        assert_eq!(diagnostics.unlayered_crate.level, DiagnosticLevel::Warn);
        assert!(diagnostics.unlayered_crate.except.is_empty());
        assert_eq!(diagnostics.unmatched_baseline_entry, DiagnosticLevel::Warn);
        assert_eq!(diagnostics.unmatched_except, DiagnosticLevel::Warn);
    }

    #[test]
    fn test_diagnostics_bare_level_strings() {
        let toml = r#"
            [diagnostics]
            unlayered-crate = "deny"
            unmatched-baseline-entry = "allow"
            unmatched-except = "warn"
        "#;
        let config = ArcConfig::from_toml(toml).unwrap();
        let diagnostics = &config.diagnostics;
        assert_eq!(diagnostics.unlayered_crate.level, DiagnosticLevel::Deny);
        assert!(
            diagnostics.unlayered_crate.except.is_empty(),
            "the bare form names a level and nothing else"
        );
        assert_eq!(diagnostics.unmatched_baseline_entry, DiagnosticLevel::Allow);
        assert_eq!(diagnostics.unmatched_except, DiagnosticLevel::Warn);
    }

    #[test]
    fn test_diagnostics_unlayered_crate_table_form() {
        let toml = r#"
            [diagnostics]
            unlayered-crate = { level = "deny", except = ["xtask", "benches"] }
        "#;
        let config = ArcConfig::from_toml(toml).unwrap();
        let unlayered = &config.diagnostics.unlayered_crate;
        assert_eq!(unlayered.level, DiagnosticLevel::Deny);
        assert_eq!(unlayered.except, ["xtask", "benches"]);
    }

    #[test]
    fn test_diagnostics_table_without_level_keeps_the_default() {
        let toml = r#"
            [diagnostics]
            unlayered-crate = { except = ["xtask"] }
        "#;
        let config = ArcConfig::from_toml(toml).unwrap();
        let unlayered = &config.diagnostics.unlayered_crate;
        assert_eq!(unlayered.level, DiagnosticLevel::Warn);
        assert_eq!(unlayered.except, ["xtask"]);
    }

    #[test]
    fn test_diagnostics_reject_unknown_name() {
        let toml = r#"
            [diagnostics]
            unlayered-crates = "warn"
        "#;
        let result = ArcConfig::from_toml(toml);
        assert!(
            result.is_err(),
            "a mistyped diagnostic name would otherwise switch nothing on"
        );
    }

    #[test]
    fn test_diagnostics_reject_unknown_level() {
        let toml = r#"
            [diagnostics]
            unmatched-except = "loud"
        "#;
        let result = ArcConfig::from_toml(toml);
        assert!(result.is_err(), "an unknown level is a config error");
    }

    /// Asserts that loading `toml` fails and that the error names `key`. An
    /// error without the key leaves the reader as stuck as the silent load did.
    fn assert_rejects_key(toml: &str, key: &str) {
        let error = ArcConfig::from_toml(toml).unwrap_err().to_string();
        assert!(
            error.contains(key),
            "expected {key} to be named, got: {error}"
        );
    }

    #[test]
    fn test_reject_unknown_top_level_section() {
        assert_rejects_key(
            r#"
            [diagnostic]
            unlayered-crate = "deny"
        "#,
            "diagnostic",
        );
    }

    #[test]
    fn test_reject_unknown_rule_key() {
        assert_rejects_key(
            r#"
            [[rules]]
            type = "no-cycles"
            name = "domain acyclic"
            scope = "domain::**"
            scpoe = "domain::**"
        "#,
            "scpoe",
        );
    }

    #[test]
    fn test_reject_unknown_config_meta_key() {
        assert_rejects_key(
            r#"
            [config]
            version = 1
            defualt_severity = "warn"
        "#,
            "defualt_severity",
        );
    }

    #[test]
    fn test_reject_unknown_except_key() {
        assert_rejects_key(
            r#"
            [[rules]]
            type = "no-cycles"
            name = "app acyclic"
            scope = "app::**"
            except = [
              { from = "app::router", to = "app::screens::**", resaon = "router mediates" },
            ]
        "#,
            "resaon",
        );
    }

    #[test]
    fn test_reject_unknown_key_in_unlayered_crate_table() {
        assert_rejects_key(
            r#"
            [diagnostics]
            unlayered-crate = { level = "deny", excpet = ["xtask"] }
        "#,
            "excpet",
        );
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
