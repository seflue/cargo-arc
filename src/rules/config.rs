//! Config parsing for arc-rules.toml

use serde::Deserialize;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

/// Name of the implicit cycle rule. It reaches the user in every report block
/// it raises and keys the baseline entries it freezes, so it reads as the
/// assertion it makes, like the names in a rules file.
const IMPLICIT_RULE_NAME: &str = "no cycles";

/// The `[config].version` this build accepts, documented in `docs/RULES.md`. A
/// file naming a different number is refused rather than read as this one.
const FORMAT_VERSION: u32 = 1;

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
    /// `<name> = [entries]`: a definition is its entries and nothing else, so
    /// a name maps to the list directly instead of a table with one key.
    #[serde(default, rename = "dependency-patterns")]
    dependency_patterns: HashMap<String, Vec<AllowEntry>>,
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

/// The `unlayered-node` diagnostic: its level plus the nodes that are
/// deliberately outside the architecture (build tooling, examples). A name here
/// is a qualified node name and takes the modules below it with it, the way a
/// pattern does; `except` on a rule does not reach diagnostics.
///
/// Written either as a bare level (`"warn"`) or as a table
/// (`{ level = "warn", except = ["xtask"] }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlayeredNode {
    pub level: DiagnosticLevel,
    pub except: Vec<String>,
}

impl UnlayeredNode {
    /// A node an exhaustive rule leaves unsorted has the failure shape
    /// `unmatched-pattern` denies for: its edges are skipped without a word and
    /// the run stays green. The level only ever reaches someone who wrote
    /// `exhaustive = true`.
    fn default_level() -> DiagnosticLevel {
        DiagnosticLevel::Deny
    }
}

impl Default for UnlayeredNode {
    fn default() -> Self {
        Self {
            level: Self::default_level(),
            except: Vec::new(),
        }
    }
}

impl<'de> Deserialize<'de> for UnlayeredNode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `#[serde(untagged)]` would only report "data did not match any
        // variant". Deserializing the table arm as its own
        // `deny_unknown_fields` struct names the offending key instead.
        struct UnlayeredNodeVisitor;

        impl<'de> serde::de::Visitor<'de> for UnlayeredNodeVisitor {
            type Value = UnlayeredNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a diagnostic level string, or a table with level/except")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let level =
                    DiagnosticLevel::deserialize(serde::de::value::StrDeserializer::new(v))?;
                Ok(UnlayeredNode {
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
                    #[serde(default = "UnlayeredNode::default_level")]
                    level: DiagnosticLevel,
                    #[serde(default)]
                    except: Vec<String>,
                }
                let table = Table::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(UnlayeredNode {
                    level: table.level,
                    except: table.except,
                })
            }
        }

        deserializer.deserialize_any(UnlayeredNodeVisitor)
    }
}

/// A mistyped name is rejected rather than ignored: a diagnostic that silently
/// stays off is the state this section exists to end.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawDiagnostics")]
pub struct Diagnostics {
    pub unlayered_node: UnlayeredNode,
    pub unmatched_baseline_entry: DiagnosticLevel,
    pub unmatched_allow: DiagnosticLevel,
    pub unmatched_pattern: DiagnosticLevel,
    pub contradictory_allow: DiagnosticLevel,
}

/// `Diagnostics` as written, with the retired name declared so it reaches a
/// message naming its replacement instead of "unknown field".
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct RawDiagnostics {
    #[serde(default)]
    unlayered_node: UnlayeredNode,
    #[serde(default)]
    unmatched_baseline_entry: DiagnosticLevel,
    #[serde(default)]
    unmatched_allow: DiagnosticLevel,
    #[serde(default = "Diagnostics::unmatched_pattern_default")]
    unmatched_pattern: DiagnosticLevel,
    #[serde(default = "Diagnostics::contradictory_allow_default")]
    contradictory_allow: DiagnosticLevel,
    #[serde(default)]
    unmatched_except: Option<serde::de::IgnoredAny>,
}

impl TryFrom<RawDiagnostics> for Diagnostics {
    type Error = String;

    fn try_from(raw: RawDiagnostics) -> Result<Self, Self::Error> {
        if raw.unmatched_except.is_some() {
            return Err("`unmatched-except` no longer exists: write `unmatched-allow`".to_owned());
        }
        Ok(Self {
            unlayered_node: raw.unlayered_node,
            unmatched_baseline_entry: raw.unmatched_baseline_entry,
            unmatched_allow: raw.unmatched_allow,
            unmatched_pattern: raw.unmatched_pattern,
            contradictory_allow: raw.contradictory_allow,
        })
    }
}

impl Diagnostics {
    /// A rule pattern that matches nothing constrains nothing, and the run
    /// stays green over it; a dead `allow` only allows too much and shows up
    /// as a violation. That asymmetry is why this one denies where the others
    /// warn.
    fn unmatched_pattern_default() -> DiagnosticLevel {
        DiagnosticLevel::Deny
    }

    /// Two entries that put a pair above each other declare no order at all,
    /// and the rule would silently tolerate the cycle between them.
    fn contradictory_allow_default() -> DiagnosticLevel {
        DiagnosticLevel::Deny
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            unlayered_node: UnlayeredNode::default(),
            unmatched_baseline_entry: DiagnosticLevel::default(),
            unmatched_allow: DiagnosticLevel::default(),
            unmatched_pattern: Self::unmatched_pattern_default(),
            contradictory_allow: Self::contradictory_allow_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    TopDown,
    BottomUp,
}

/// Where an `allow` entry's `to` points: a module path pattern of its own, or
/// a place relative to the node `from` matched. A relative target that
/// resolves to nothing (the crate node has no `super`) matches nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowTarget {
    Path(String),
    /// `super`, `super::super`, ...: the ancestor that many levels up.
    Ancestor(NonZeroUsize),
    /// `crate`: the root of the crate `from` lies in.
    Crate,
    /// `self::*`: the direct children of `from`.
    Children,
    /// `self::**`: every module under `from`.
    Descendants,
}

impl AllowTarget {
    /// The module path pattern of an absolute target; `None` for a relative
    /// one, which is resolved per matched `from` node and has no pattern to
    /// check on its own.
    #[must_use]
    pub fn pattern(&self) -> Option<&str> {
        match self {
            Self::Path(pattern) => Some(pattern),
            _ => None,
        }
    }
}

impl std::str::FromStr for AllowTarget {
    type Err = String;

    /// The relative forms are the reserved path keywords of Rust, and a
    /// pattern never starts with one, so a string starting with `self`,
    /// `super` or `crate` is either one of the offered forms or an error.
    fn from_str(target: &str) -> Result<Self, Self::Err> {
        let is_keyword_path =
            |keyword: &str| target == keyword || target.starts_with(&format!("{keyword}::"));
        if target == "crate" {
            return Ok(Self::Crate);
        }
        if target == "self::*" {
            return Ok(Self::Children);
        }
        if target == "self::**" {
            return Ok(Self::Descendants);
        }
        if is_keyword_path("super") && target.split("::").all(|segment| segment == "super") {
            let levels = target.split("::").count();
            return Ok(Self::Ancestor(
                NonZeroUsize::new(levels).expect("a split yields at least one segment"),
            ));
        }
        if is_keyword_path("self") || is_keyword_path("super") || is_keyword_path("crate") {
            return Err(format!(
                "`to` is {target:?}; a relative target is `crate`, `super`, `super::super`, \
                 `self::*` or `self::**`"
            ));
        }
        Ok(Self::Path(target.to_owned()))
    }
}

/// The target as written in the file, the inverse of `FromStr`.
impl std::fmt::Display for AllowTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(pattern) => f.write_str(pattern),
            Self::Ancestor(levels) => f.write_str(&vec!["super"; levels.get()].join("::")),
            Self::Crate => f.write_str("crate"),
            Self::Children => f.write_str("self::*"),
            Self::Descendants => f.write_str("self::**"),
        }
    }
}

impl<'de> Deserialize<'de> for AllowTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// One item of an `allow` list as written: a reference to a
/// `[dependency-patterns]` definition, or an edge of its own.
#[derive(Debug)]
enum AllowEntry {
    DependencyPattern(String),
    Edge {
        from: String,
        to: AllowTarget,
        reason: Option<String>,
    },
}

impl<'de> Deserialize<'de> for AllowEntry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // One table with every key optional, checked by hand: `untagged` would
        // only say "data did not match any variant" for a mixed entry.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Table {
            pattern: Option<String>,
            from: Option<String>,
            to: Option<AllowTarget>,
            reason: Option<String>,
        }
        let table = Table::deserialize(deserializer)?;
        match table {
            Table {
                pattern: Some(pattern),
                from: None,
                to: None,
                reason: None,
            } => Ok(Self::DependencyPattern(pattern)),
            Table {
                pattern: None,
                from: Some(from),
                to: Some(to),
                reason,
            } => Ok(Self::Edge { from, to, reason }),
            _ => Err(serde::de::Error::custom(
                "an allow entry is either `{ pattern = \"<name>\" }` or \
                 `{ from = \"<pattern>\", to = \"<pattern>\", reason = \"...\" }`",
            )),
        }
    }
}

/// A permanently allowed edge for the rule it is declared on, with its
/// `[dependency-patterns]` reference already expanded.
///
/// Scoped to its rule rather than a shared section: the allowance lives and
/// dies with the rule it applies to. An entry names the odd edge of a
/// codebase's order, the one against the direction dependencies run by
/// default, so the entries of a rule together declare that order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedEdge {
    pub from: String,
    pub to: AllowTarget,
    /// Documentation only (cargo-deny style), never evaluated.
    pub reason: Option<String>,
    /// The `[dependency-patterns]` definition the entry came from; `None` for
    /// an entry written on the rule itself.
    pub dependency_pattern: Option<String>,
}

impl AllowedEdge {
    /// The entry as written, for messages that point back at the file.
    #[must_use]
    pub fn written(&self) -> String {
        format!("{} -> {}", self.from, self.to)
    }
}

/// `flatten` hands every key this struct does not declare — `type` and the
/// rule parameters — to `RuleKind`, whose variants reject the unknown ones.
/// The retired key `except` is declared here so it reaches a message naming
/// its replacement instead of `RuleKind`'s "unknown field".
#[derive(Debug, Deserialize)]
struct RawRule {
    name: String,
    #[serde(default, rename = "severity")]
    severity: Option<Severity>,
    #[serde(default)]
    allow: Vec<AllowEntry>,
    #[serde(default)]
    except: Option<serde::de::IgnoredAny>,
    #[serde(flatten)]
    kind: RuleKind,
}

impl RawRule {
    /// The first retired key the rule still uses, with the `allow` form that
    /// replaces it.
    fn retired_key(&self) -> Option<(&'static str, &'static str)> {
        if self.except.is_some() {
            return Some(("except", "allow = [{ from = \"...\", to = \"...\" }]"));
        }
        None
    }
}

#[derive(Debug)]
pub struct Rule {
    pub name: String,
    pub severity: Severity,
    pub allow: Vec<AllowedEdge>,
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

/// One rank in a `layers` rule: either the patterns whose nodes share that
/// position, or the catch-all that stands for whatever the rule's other
/// positions do not match.
///
/// Several patterns per ordinary rank exist because a rank is not always one
/// crate. Written either as a bare pattern (`"domain"`) or as a list
/// (`["adapter_a", "adapter_b"]`); listing equals separately would make the
/// list order assert a ranking between them. The catch-all is written as the
/// bare string `"*"` or the single-element list `["*"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layer {
    Patterns(Vec<String>),
    CatchAll,
}

impl Layer {
    /// The patterns of an ordinary position; `None` for the catch-all, which
    /// carries none of its own to check for emptiness the ordinary way.
    #[must_use]
    pub fn patterns(&self) -> Option<&[String]> {
        match self {
            Self::Patterns(patterns) => Some(patterns),
            Self::CatchAll => None,
        }
    }

    #[must_use]
    pub fn is_catch_all(&self) -> bool {
        matches!(self, Self::CatchAll)
    }
}

impl From<&str> for Layer {
    fn from(pattern: &str) -> Self {
        if pattern == "*" {
            Self::CatchAll
        } else {
            Self::Patterns(vec![pattern.to_owned()])
        }
    }
}

impl FromIterator<String> for Layer {
    fn from_iter<I: IntoIterator<Item = String>>(patterns: I) -> Self {
        let patterns: Vec<String> = patterns.into_iter().collect();
        if patterns == ["*"] {
            Self::CatchAll
        } else {
            Self::Patterns(patterns)
        }
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
    #[serde(default)]
    pub exhaustive: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RuleKind {
    ForbiddenDependency(ForbiddenDependencyRule),
    NoCycles(NoCyclesRule),
    Layers(LayersRule),
}

impl RuleKind {
    /// Every module path pattern the rule itself is written with. `allow`
    /// patterns are not among them: those state an allowance, not the reach of
    /// the rule, and have their own diagnostic.
    #[must_use]
    pub fn patterns(&self) -> Vec<&str> {
        match self {
            Self::ForbiddenDependency(params) => {
                vec![params.from.as_str(), params.to.as_str()]
            }
            Self::NoCycles(params) => vec![params.scope.as_str()],
            Self::Layers(params) => params
                .layers
                .iter()
                .filter_map(Layer::patterns)
                .flatten()
                .map(String::as_str)
                .collect(),
        }
    }
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
    DuplicateRuleName {
        path: PathBuf,
        name: String,
    },
    ReservedRuleName {
        path: PathBuf,
        name: String,
    },
    CatchAllNotAlone {
        path: PathBuf,
        name: String,
    },
    MultipleCatchAllPositions {
        path: PathBuf,
        name: String,
    },
    ExhaustiveWithCatchAll {
        path: PathBuf,
        name: String,
    },
    RetiredKey {
        path: PathBuf,
        name: String,
        key: &'static str,
        replacement: &'static str,
    },
    UnknownDependencyPattern {
        path: PathBuf,
        name: String,
        pattern: String,
    },
    ReferenceInDependencyPattern {
        path: PathBuf,
        pattern: String,
    },
    EmptyPosition {
        path: PathBuf,
        name: String,
    },
    TooFewPositions {
        path: PathBuf,
        name: String,
    },
    UnsupportedVersion {
        path: PathBuf,
        found: u32,
    },
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
            Self::ReservedRuleName { path, name } => write!(
                f,
                "rule name {name:?} in {} belongs to the implicit cycle rule: \
                 rename it, or state a no-cycles rule of your own",
                path.display()
            ),
            Self::CatchAllNotAlone { path, name } => write!(
                f,
                "rule {name:?} in {}: the catch-all layer `*` must stand alone in its position",
                path.display()
            ),
            Self::MultipleCatchAllPositions { path, name } => write!(
                f,
                "rule {name:?} in {}: only one position may be the catch-all layer `*`",
                path.display()
            ),
            Self::ExhaustiveWithCatchAll { path, name } => write!(
                f,
                "rule {name:?} in {} claims to be exhaustive beside the catch-all layer `*`, \
                 which already holds every node its other positions leave: drop \
                 `exhaustive = true`, or drop the catch-all",
                path.display()
            ),
            Self::RetiredKey {
                path,
                name,
                key,
                replacement,
            } => write!(
                f,
                "rule {name:?} in {} uses `{key}`, which no longer exists: write {replacement}",
                path.display()
            ),
            Self::UnknownDependencyPattern {
                path,
                name,
                pattern,
            } => write!(
                f,
                "rule {name:?} in {} refers to dependency pattern {pattern:?}, and the file \
                 defines no `{pattern}` under `[dependency-patterns]`",
                path.display()
            ),
            Self::ReferenceInDependencyPattern { path, pattern } => write!(
                f,
                "dependency pattern `{pattern}` in {} refers to another one: a definition \
                 holds entries only",
                path.display()
            ),
            Self::EmptyPosition { path, name } => write!(
                f,
                "rule {name:?} in {}: every position must hold at least one pattern, \
                 and one holds none",
                path.display()
            ),
            Self::TooFewPositions { path, name } => write!(
                f,
                "rule {name:?} in {}: a layers rule orders its positions against each \
                 other and needs at least two",
                path.display()
            ),
            Self::UnsupportedVersion { path, found } => write!(
                f,
                "unsupported config file {}: format version {found}, this cargo-arc \
                 supports version {FORMAT_VERSION}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl ArcConfig {
    /// # Errors
    /// Returns `ConfigError::FileNotFound` if the path does not exist,
    /// `ConfigError::IoError` for other I/O failures, or
    /// `ConfigError::ParseError` for invalid TOML. Once the file parses, every
    /// other variant reports a load-time check that failed on its content.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::FileNotFound(path.to_path_buf())
            } else {
                ConfigError::IoError(path.to_path_buf(), e)
            }
        })?;
        let (mut config, version) = Self::from_toml(&content, path)?;
        if let Some(found) = version
            && found != FORMAT_VERSION
        {
            return Err(ConfigError::UnsupportedVersion {
                path: path.to_path_buf(),
                found,
            });
        }
        config.check_unique_rule_names(path)?;
        config.check_catch_all_layers(path)?;
        config.check_layers_arity(path)?;
        config.check_exhaustive_layers(path)?;
        config.add_implicit_rule(path)?;
        Ok(config)
    }

    /// The configuration of a run that finds no rules file: the implicit cycle
    /// rule and nothing else.
    #[must_use]
    pub fn implicit() -> Self {
        Self {
            rules: vec![Self::implicit_rule()],
            diagnostics: Diagnostics::default(),
        }
    }

    /// Names of the rules at `Severity::Ignore`: a run skips them, but a
    /// baseline may still hold entries frozen under one from before it was
    /// turned off.
    pub fn ignored_rules(&self) -> impl Iterator<Item = &str> {
        self.rules
            .iter()
            .filter(|rule| rule.severity == Severity::Ignore)
            .map(|rule| rule.name.as_str())
    }

    fn implicit_rule() -> Rule {
        Rule {
            name: IMPLICIT_RULE_NAME.to_owned(),
            severity: Severity::Error,
            allow: Vec::new(),
            kind: RuleKind::NoCycles(NoCyclesRule {
                scope: "**".to_owned(),
            }),
        }
    }

    /// Add the implicit cycle rule unless the file states a `no-cycles` rule of
    /// its own. Beside a narrower scope the implicit one would still forbid the
    /// cycles that rule deliberately allows, so the written rule replaces it
    /// rather than joining it.
    ///
    /// # Errors
    /// `ConfigError::ReservedRuleName` if a rule already carries the implicit
    /// rule's name.
    fn add_implicit_rule(&mut self, path: &Path) -> Result<(), ConfigError> {
        if self
            .rules
            .iter()
            .any(|rule| matches!(rule.kind, RuleKind::NoCycles(_)))
        {
            return Ok(());
        }
        if self
            .rules
            .iter()
            .any(|rule| rule.name == IMPLICIT_RULE_NAME)
        {
            return Err(ConfigError::ReservedRuleName {
                path: path.to_path_buf(),
                name: IMPLICIT_RULE_NAME.to_owned(),
            });
        }
        self.rules.push(Self::implicit_rule());
        Ok(())
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

    /// Reject a `layers` rule whose catch-all does not stand alone in its
    /// position, or that carries more than one catch-all position. Neither is
    /// catchable from inside `Layer`'s `Deserialize`, which has no rule name
    /// to report against.
    fn check_catch_all_layers(&self, path: &Path) -> Result<(), ConfigError> {
        for rule in &self.rules {
            let RuleKind::Layers(params) = &rule.kind else {
                continue;
            };
            let mut catch_alls = 0;
            for layer in &params.layers {
                match layer {
                    Layer::CatchAll => catch_alls += 1,
                    Layer::Patterns(patterns) => {
                        if patterns.iter().any(|pattern| pattern == "*") {
                            return Err(ConfigError::CatchAllNotAlone {
                                path: path.to_path_buf(),
                                name: rule.name.clone(),
                            });
                        }
                    }
                }
            }
            if catch_alls > 1 {
                return Err(ConfigError::MultipleCatchAllPositions {
                    path: path.to_path_buf(),
                    name: rule.name.clone(),
                });
            }
        }
        Ok(())
    }

    /// Reject a `layers` rule with fewer than two positions, or with a
    /// position holding no patterns. One position orders nothing against
    /// another, and a position without a pattern holds no node to place.
    fn check_layers_arity(&self, path: &Path) -> Result<(), ConfigError> {
        for rule in &self.rules {
            let RuleKind::Layers(params) = &rule.kind else {
                continue;
            };
            for layer in &params.layers {
                if layer.patterns().is_some_and(<[String]>::is_empty) {
                    return Err(ConfigError::EmptyPosition {
                        path: path.to_path_buf(),
                        name: rule.name.clone(),
                    });
                }
            }
            if params.layers.len() < 2 {
                return Err(ConfigError::TooFewPositions {
                    path: path.to_path_buf(),
                    name: rule.name.clone(),
                });
            }
        }
        Ok(())
    }

    /// Reject a `layers` rule that declares itself exhaustive beside a catch-all
    /// layer. `layer_rest` hands the catch-all every non-external node the other
    /// positions leave, so the claim holds by construction and checks nothing.
    fn check_exhaustive_layers(&self, path: &Path) -> Result<(), ConfigError> {
        for rule in &self.rules {
            let RuleKind::Layers(params) = &rule.kind else {
                continue;
            };
            if params.exhaustive && params.layers.iter().any(Layer::is_catch_all) {
                return Err(ConfigError::ExhaustiveWithCatchAll {
                    path: path.to_path_buf(),
                    name: rule.name.clone(),
                });
            }
        }
        Ok(())
    }

    /// Fills in `config.default_severity` for rules that left `severity`
    /// unset and expands every `{ pattern = ... }` reference into the entries
    /// of its definition. The second element is the `[config].version` the
    /// file named, or `None` for a file without a `[config]` section.
    ///
    /// # Errors
    /// `ConfigError::ParseError` for invalid TOML; `RetiredKey`,
    /// `UnknownDependencyPattern` and `ReferenceInDependencyPattern` for
    /// content the syntax no longer has or a reference that resolves to no
    /// definition.
    fn from_toml(content: &str, path: &Path) -> Result<(Self, Option<u32>), ConfigError> {
        let raw: RawConfig =
            toml::from_str(content).map_err(|e| ConfigError::ParseError(path.to_path_buf(), e))?;
        let version = raw.config.as_ref().map(|meta| meta.version);
        let default = raw
            .config
            .map(|meta| meta.default_severity)
            .unwrap_or_default();

        let mut patterns: HashMap<String, Vec<AllowedEdge>> = HashMap::new();
        for (name, entries) in raw.dependency_patterns {
            let mut edges = Vec::new();
            for entry in entries {
                match entry {
                    AllowEntry::DependencyPattern(_) => {
                        return Err(ConfigError::ReferenceInDependencyPattern {
                            path: path.to_path_buf(),
                            pattern: name,
                        });
                    }
                    AllowEntry::Edge { from, to, reason } => edges.push(AllowedEdge {
                        from,
                        to,
                        reason,
                        dependency_pattern: Some(name.clone()),
                    }),
                }
            }
            patterns.insert(name, edges);
        }

        let mut rules = Vec::new();
        for rule in raw.rules {
            if let Some((key, replacement)) = rule.retired_key() {
                return Err(ConfigError::RetiredKey {
                    path: path.to_path_buf(),
                    name: rule.name,
                    key,
                    replacement,
                });
            }
            let mut allow = Vec::new();
            for entry in rule.allow {
                match entry {
                    AllowEntry::DependencyPattern(pattern) => {
                        let Some(edges) = patterns.get(&pattern) else {
                            return Err(ConfigError::UnknownDependencyPattern {
                                path: path.to_path_buf(),
                                name: rule.name,
                                pattern,
                            });
                        };
                        allow.extend(edges.iter().cloned());
                    }
                    AllowEntry::Edge { from, to, reason } => allow.push(AllowedEdge {
                        from,
                        to,
                        reason,
                        dependency_pattern: None,
                    }),
                }
            }
            rules.push(Rule {
                name: rule.name,
                severity: rule.severity.unwrap_or(default),
                allow,
                kind: rule.kind,
            });
        }
        Ok((
            Self {
                rules,
                diagnostics: raw.diagnostics,
            },
            version,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Result<(ArcConfig, Option<u32>), ConfigError> {
        ArcConfig::from_toml(toml, Path::new("arc-rules.toml"))
    }

    #[test]
    fn test_parse_forbidden_dependency() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
        "#;
        let (config, _) = parse(toml).unwrap();
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
        let (config, _) = parse(toml).unwrap();
        assert_eq!(config.rules[0].name, "domain acyclic");
        assert!(matches!(
            &config.rules[0].kind,
            RuleKind::NoCycles(NoCyclesRule { scope, .. })
            if scope == "domain::**"
        ));
    }

    fn load(toml: &str) -> Result<ArcConfig, ConfigError> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();
        ArcConfig::load(&path)
    }

    fn relative_target(to: &str) -> AllowTarget {
        let toml = format!(
            r#"
            [[rules]]
            type = "no-cycles"
            name = "no cycles"
            scope = "**"
            allow = [{{ from = "**", to = "{to}" }}]
        "#
        );
        let (config, _) = parse(&toml).unwrap();
        config.rules[0].allow[0].to.clone()
    }

    #[test]
    fn test_parse_relative_targets() {
        assert_eq!(
            relative_target("super::super"),
            AllowTarget::Ancestor(NonZeroUsize::new(2).unwrap())
        );
        assert_eq!(relative_target("crate"), AllowTarget::Crate);
        assert_eq!(relative_target("self::*"), AllowTarget::Children);
        assert_eq!(relative_target("self::**"), AllowTarget::Descendants);
    }

    #[test]
    fn test_a_relative_target_outside_the_offered_forms_fails_to_parse() {
        for to in ["super::sibling", "self", "crate::core", "super::*"] {
            let toml = format!(
                r#"
                [[rules]]
                type = "no-cycles"
                name = "no cycles"
                scope = "**"
                allow = [{{ from = "**", to = "{to}" }}]
            "#
            );
            let error = parse(&toml).unwrap_err();
            let message = error.to_string();
            assert!(message.contains(to), "{to}: got: {message}");
            assert!(message.contains("self::**"), "{to}: got: {message}");
        }
    }

    #[test]
    fn test_an_entry_mixing_pattern_and_edge_fails_to_parse() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "no cycles"
            scope = "**"
            allow = [{ pattern = "vocabulary-parent", from = "**", to = "super" }]
        "#;
        let error = parse(toml).unwrap_err();
        assert!(error.to_string().contains("either"), "got: {error}");
    }

    #[test]
    fn test_a_reference_to_an_undefined_pattern_fails_to_load() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "no cycles"
            scope = "**"
            allow = [{ pattern = "vocabulary-parent" }]
        "#;
        let error = load(toml).unwrap_err();
        assert!(
            matches!(&error, ConfigError::UnknownDependencyPattern { name, pattern, .. }
                if name == "no cycles" && pattern == "vocabulary-parent"),
            "got: {error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("`vocabulary-parent` under `[dependency-patterns]`"),
            "got: {error}"
        );
    }

    #[test]
    fn test_a_pattern_definition_referring_to_a_pattern_fails_to_load() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "no cycles"
            scope = "**"

            [dependency-patterns]
            outer = [{ pattern = "inner" }]
            inner = [{ from = "**", to = "super" }]
        "#;
        let error = load(toml).unwrap_err();
        assert!(
            matches!(&error, ConfigError::ReferenceInDependencyPattern { pattern, .. } if pattern == "outer"),
            "got: {error:?}"
        );
    }

    #[test]
    fn test_a_pattern_definition_written_as_a_table_fails_to_parse() {
        let toml = r#"
            [dependency-patterns.vocabulary-parent]
            allow = [{ from = "**", to = "super" }]
        "#;
        let error = parse(toml).unwrap_err();
        assert!(error.to_string().contains("sequence"), "got: {error}");
    }

    #[test]
    fn test_the_retired_except_key_fails_to_load_naming_allow() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "no cycles"
            scope = "**"
            except = [{ from = "core::a", to = "core::b" }]
        "#;
        let error = load(toml).unwrap_err();
        assert!(
            matches!(&error, ConfigError::RetiredKey { name, key: "except", .. }
                if name == "no cycles"),
            "got: {error:?}"
        );
        let message = error.to_string();
        assert!(message.contains("allow = ["), "got: {message}");
    }

    #[test]
    fn test_the_retired_diagnostic_name_fails_to_parse_naming_the_replacement() {
        let toml = r#"
            [diagnostics]
            unmatched-except = "warn"
        "#;
        let error = parse(toml).unwrap_err();
        assert!(
            matches!(error, ConfigError::ParseError(..)),
            "got: {error:?}"
        );
        let message = error.to_string();
        assert!(message.contains("unmatched-allow"), "got: {message}");
    }

    #[test]
    fn test_a_retired_key_is_refused_on_every_rule_type() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "infra"]
            direction = "top-down"
            except = [{ from = "infra::bridge", to = "domain::events" }]
        "#;
        let error = load(toml).unwrap_err();
        assert!(
            matches!(&error, ConfigError::RetiredKey { key: "except", .. }),
            "got: {error:?}"
        );
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
        let (config, _) = parse(toml).unwrap();
        assert_eq!(config.rules[0].name, "architecture layers");
        let RuleKind::Layers(LayersRule {
            layers, direction, ..
        }) = &config.rules[0].kind
        else {
            panic!("expected Layers, got {:?}", config.rules[0].kind);
        };
        assert_eq!(
            layers,
            &[
                Layer::Patterns(vec!["domain".to_string()]),
                Layer::Patterns(vec!["application".to_string()]),
                Layer::Patterns(vec!["infra".to_string()]),
            ]
        );
        assert_eq!(*direction, Direction::TopDown);
    }

    #[test]
    fn test_layers_exhaustive_defaults_to_false() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "infra"]
            direction = "top-down"
        "#;
        let (config, _) = parse(toml).unwrap();
        let RuleKind::Layers(LayersRule { exhaustive, .. }) = &config.rules[0].kind else {
            panic!("expected Layers, got {:?}", config.rules[0].kind);
        };
        assert!(!exhaustive);
    }

    #[test]
    fn test_layers_exhaustive_parses() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "infra"]
            direction = "top-down"
            exhaustive = true
        "#;
        let (config, _) = parse(toml).unwrap();
        let RuleKind::Layers(LayersRule { exhaustive, .. }) = &config.rules[0].kind else {
            panic!("expected Layers, got {:?}", config.rules[0].kind);
        };
        assert!(exhaustive);
    }

    #[test]
    fn test_reject_misspelled_exhaustive() {
        assert_rejects_key(
            r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "infra"]
            direction = "top-down"
            exhuastive = true
        "#,
            "exhuastive",
        );
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
        let (config, _) = parse(toml).unwrap();
        let RuleKind::Layers(LayersRule { layers, .. }) = &config.rules[0].kind else {
            panic!("expected Layers, got {:?}", config.rules[0].kind);
        };
        assert_eq!(
            layers,
            &[
                Layer::Patterns(vec!["domain".to_string()]),
                Layer::Patterns(vec!["adapter_a".to_string(), "adapter_b".to_string()]),
                Layer::Patterns(vec!["runtime".to_string()]),
            ]
        );
    }

    #[test]
    fn test_rule_patterns_per_kind() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
            allow = [
              { from = "domain::legacy", to = "infra::db" },
            ]

            [[rules]]
            type = "no-cycles"
            name = "domain acyclic"
            scope = "domain::**"

            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", ["adapter_a", "adapter_b"]]
            direction = "top-down"
        "#;
        let (config, _) = parse(toml).unwrap();
        let patterns: Vec<Vec<&str>> = config
            .rules
            .iter()
            .map(|rule| rule.kind.patterns())
            .collect();
        assert_eq!(
            patterns,
            [
                // The `allow` patterns are the other diagnostic's business.
                vec!["domain::**", "infra::**"],
                vec!["domain::**"],
                vec!["domain", "adapter_a", "adapter_b"],
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
        let (config, _) = parse(toml).unwrap();
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
        let (config, _) = parse(toml).unwrap();
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
        let (config, _) = parse(toml).unwrap();
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
        let (config, _) = parse(toml).unwrap();
        assert_eq!(config.rules[0].severity, Severity::Error);
    }

    #[test]
    fn test_parse_unknown_type() {
        let toml = r#"
            [[rules]]
            type = "unknown-rule"
            name = "test"
        "#;
        let result = parse(toml);
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
    fn test_parse_allow_expands_a_pattern_reference_and_reads_a_relative_target() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "no cycles"
            scope = "**"
            allow = [
              { pattern = "vocabulary-parent" },
              { from = "core::keywords", to = "core::writer", reason = "table generated from the writer" },
            ]

            [dependency-patterns]
            vocabulary-parent = [{ from = "**", to = "super", reason = "children read the parent's vocabulary" }]
        "#;
        let (config, _) = parse(toml).unwrap();
        let allow = &config.rules[0].allow;
        assert_eq!(allow.len(), 2);
        assert_eq!(allow[0].from, "**");
        assert_eq!(
            allow[0].to,
            AllowTarget::Ancestor(NonZeroUsize::new(1).unwrap())
        );
        assert_eq!(
            allow[0].dependency_pattern.as_deref(),
            Some("vocabulary-parent")
        );
        assert_eq!(allow[1].from, "core::keywords");
        assert_eq!(allow[1].to, AllowTarget::Path("core::writer".into()));
        assert_eq!(allow[1].dependency_pattern, None);
        assert_eq!(
            allow[1].reason.as_deref(),
            Some("table generated from the writer")
        );
    }

    #[test]
    fn test_parse_allow_without_reason() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "app acyclic"
            scope = "app::**"
            allow = [
              { from = "app::router", to = "app::screens::**" },
            ]
        "#;
        let (config, _) = parse(toml).unwrap();
        let allow = &config.rules[0].allow;
        assert_eq!(allow.len(), 1);
        assert_eq!(allow[0].reason, None);
    }

    #[test]
    fn test_parse_allow_defaults_to_empty() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "app acyclic"
            scope = "app::**"
        "#;
        let (config, _) = parse(toml).unwrap();
        assert!(config.rules[0].allow.is_empty());
    }

    #[test]
    fn test_parse_allow_on_forbidden_dependency_and_layers() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
            allow = [
              { from = "domain::legacy", to = "infra::db", reason = "pending migration" },
            ]

            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "application", "infra"]
            direction = "top-down"
            allow = [
              { from = "infra::bridge", to = "domain::events" },
            ]
        "#;
        let (config, _) = parse(toml).unwrap();
        let allow = &config.rules[0].allow;
        assert_eq!(allow.len(), 1);
        assert_eq!(allow[0].from, "domain::legacy");
        assert_eq!(allow[0].to, AllowTarget::Path("infra::db".into()));
        let allow = &config.rules[1].allow;
        assert_eq!(allow.len(), 1);
        assert_eq!(allow[0].from, "infra::bridge");
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
        let (config, _) = parse(toml).unwrap();
        assert_eq!(config.rules.len(), 3);
        assert!(matches!(
            &config.rules[0].kind,
            RuleKind::ForbiddenDependency(_)
        ));
        assert!(matches!(&config.rules[1].kind, RuleKind::NoCycles(_)));
        assert!(matches!(&config.rules[2].kind, RuleKind::Layers(_)));
    }

    #[test]
    fn test_name_and_allow_readable_uniformly_across_rule_types() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
            allow = [
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
        let (config, _) = parse(toml).unwrap();
        let names: Vec<&str> = config.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "no infra in domain",
                "domain acyclic",
                "architecture layers"
            ]
        );
        let allow_lens: Vec<usize> = config.rules.iter().map(|r| r.allow.len()).collect();
        assert_eq!(allow_lens, [1, 0, 0]);
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
    fn test_diagnostics_missing_section_uses_the_per_diagnostic_defaults() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "test"
            scope = "**"
        "#;
        let (config, _) = parse(toml).unwrap();
        let diagnostics = &config.diagnostics;
        assert_eq!(diagnostics.unlayered_node.level, DiagnosticLevel::Deny);
        assert!(diagnostics.unlayered_node.except.is_empty());
        assert_eq!(diagnostics.unmatched_baseline_entry, DiagnosticLevel::Warn);
        assert_eq!(diagnostics.unmatched_allow, DiagnosticLevel::Warn);
        assert_eq!(diagnostics.unmatched_pattern, DiagnosticLevel::Deny);
        assert_eq!(diagnostics.contradictory_allow, DiagnosticLevel::Deny);
    }

    /// A section that sets other diagnostics must not pull this one down to the
    /// shared `warn` default along the way.
    #[test]
    fn test_unmatched_pattern_stays_denied_when_the_section_omits_it() {
        let toml = r#"
            [diagnostics]
            unmatched-allow = "allow"
        "#;
        let (config, _) = parse(toml).unwrap();
        assert_eq!(config.diagnostics.unmatched_pattern, DiagnosticLevel::Deny);
    }

    /// Same asymmetry as `unmatched-pattern`: a section that sets other
    /// diagnostics must not pull this one down to the shared `warn` default.
    #[test]
    fn test_unlayered_node_stays_denied_when_the_section_omits_it() {
        let toml = r#"
            [diagnostics]
            unmatched-allow = "allow"
        "#;
        let (config, _) = parse(toml).unwrap();
        assert_eq!(
            config.diagnostics.unlayered_node.level,
            DiagnosticLevel::Deny
        );
    }

    #[test]
    fn test_unmatched_pattern_is_configurable() {
        let toml = r#"
            [diagnostics]
            unmatched-pattern = "warn"
        "#;
        let (config, _) = parse(toml).unwrap();
        assert_eq!(config.diagnostics.unmatched_pattern, DiagnosticLevel::Warn);
    }

    #[test]
    fn test_diagnostics_bare_level_strings() {
        let toml = r#"
            [diagnostics]
            unlayered-node = "deny"
            unmatched-baseline-entry = "allow"
            unmatched-allow = "warn"
        "#;
        let (config, _) = parse(toml).unwrap();
        let diagnostics = &config.diagnostics;
        assert_eq!(diagnostics.unlayered_node.level, DiagnosticLevel::Deny);
        assert!(
            diagnostics.unlayered_node.except.is_empty(),
            "the bare form names a level and nothing else"
        );
        assert_eq!(diagnostics.unmatched_baseline_entry, DiagnosticLevel::Allow);
        assert_eq!(diagnostics.unmatched_allow, DiagnosticLevel::Warn);
    }

    #[test]
    fn test_diagnostics_unlayered_node_table_form() {
        let toml = r#"
            [diagnostics]
            unlayered-node = { level = "deny", except = ["xtask", "benches"] }
        "#;
        let (config, _) = parse(toml).unwrap();
        let unlayered = &config.diagnostics.unlayered_node;
        assert_eq!(unlayered.level, DiagnosticLevel::Deny);
        assert_eq!(unlayered.except, ["xtask", "benches"]);
    }

    #[test]
    fn test_diagnostics_table_without_level_keeps_the_default() {
        let toml = r#"
            [diagnostics]
            unlayered-node = { except = ["xtask"] }
        "#;
        let (config, _) = parse(toml).unwrap();
        let unlayered = &config.diagnostics.unlayered_node;
        assert_eq!(unlayered.level, DiagnosticLevel::Deny);
        assert_eq!(unlayered.except, ["xtask"]);
    }

    #[test]
    fn test_diagnostics_reject_unknown_name() {
        let toml = r#"
            [diagnostics]
            unlayered-nodes = "warn"
        "#;
        let result = parse(toml);
        assert!(
            result.is_err(),
            "a mistyped diagnostic name would otherwise switch nothing on"
        );
    }

    #[test]
    fn test_diagnostics_reject_unknown_level() {
        let toml = r#"
            [diagnostics]
            unmatched-allow = "loud"
        "#;
        let result = parse(toml);
        assert!(result.is_err(), "an unknown level is a config error");
    }

    /// Asserts that loading `toml` fails and that the error names `key`. An
    /// error without the key leaves the reader as stuck as the silent load did.
    fn assert_rejects_key(toml: &str, key: &str) {
        let error = parse(toml).unwrap_err().to_string();
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
            unlayered-node = "deny"
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
    fn test_reject_unknown_allow_key() {
        assert_rejects_key(
            r#"
            [[rules]]
            type = "no-cycles"
            name = "app acyclic"
            scope = "app::**"
            allow = [
              { from = "app::router", to = "app::screens::**", resaon = "router mediates" },
            ]
        "#,
            "resaon",
        );
    }

    #[test]
    fn test_reject_unknown_key_in_unlayered_node_table() {
        assert_rejects_key(
            r#"
            [diagnostics]
            unlayered-node = { level = "deny", excpet = ["xtask"] }
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
        assert!(config.rules.iter().all(|r| r.allow.is_empty()));
        match &config.rules[1].kind {
            RuleKind::Layers(LayersRule { direction, .. }) => {
                assert_eq!(*direction, Direction::TopDown);
            }
            other => panic!("expected Layers, got {other:?}"),
        }
    }

    /// The file `docs/RULES.md` quotes from and offers for copying. Loading it
    /// here keeps the documented format and the parsed one the same thing.
    #[test]
    fn test_parse_documented_example() {
        let path = Path::new("docs/arc-rules.example.toml");
        let config = ArcConfig::load(path).unwrap();
        let kinds: Vec<&str> = config.rules.iter().map(Rule::rule_type).collect();
        assert_eq!(kinds, ["layers", "forbidden-dependency", "no-cycles"]);
        assert!(
            config.rules.iter().all(|rule| rule.allow.is_empty()),
            "the example answers no finding yet, so it carries no allow entry"
        );
        assert_eq!(
            config.diagnostics,
            Diagnostics::default(),
            "the example answers no finding yet, so it configures no diagnostic"
        );
    }

    #[test]
    fn test_implicit_config_carries_one_cycle_rule_over_the_workspace() {
        let config = ArcConfig::implicit();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].name, "no cycles");
        assert_eq!(config.rules[0].severity, Severity::Error);
        assert!(matches!(
            &config.rules[0].kind,
            RuleKind::NoCycles(NoCyclesRule { scope, .. }) if scope == "**"
        ));
    }

    #[test]
    fn test_config_without_a_cycle_rule_gets_the_implicit_one() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no infra in domain"
            from = "domain::**"
            to = "infra::**"
        "#;
        let (mut config, _) = parse(toml).unwrap();
        config
            .add_implicit_rule(Path::new("arc-rules.toml"))
            .unwrap();
        assert_eq!(config.rules.len(), 2);
        assert_eq!(config.rules[1].name, "no cycles");
    }

    /// Without this the implicit scope `**` would sit next to a narrower one and
    /// forbid the cycles that rule deliberately allows.
    #[test]
    fn test_a_cycle_rule_in_the_file_replaces_the_implicit_one() {
        let toml = r#"
            [[rules]]
            type = "no-cycles"
            name = "domain acyclic"
            scope = "domain::**"
        "#;
        let (mut config, _) = parse(toml).unwrap();
        config
            .add_implicit_rule(Path::new("arc-rules.toml"))
            .unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].name, "domain acyclic");
    }

    /// A baseline entry names its rule and no type, so two rules of that name
    /// would make the frozen violations ambiguous.
    #[test]
    fn test_a_rule_named_like_the_implicit_one_is_rejected() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no cycles"
            from = "domain::**"
            to = "infra::**"
        "#;
        let (mut config, _) = parse(toml).unwrap();
        let err = config
            .add_implicit_rule(Path::new("arc-rules.toml"))
            .unwrap_err();
        assert!(
            matches!(&err, ConfigError::ReservedRuleName { name, .. } if name == "no cycles"),
            "got: {err:?}"
        );
    }

    /// The name is only reserved where the implicit rule would land: with a
    /// `no-cycles` rule in the file nothing is injected, so nothing collides.
    #[test]
    fn test_the_reserved_name_is_free_once_the_file_checks_cycles_itself() {
        let toml = r#"
            [[rules]]
            type = "forbidden-dependency"
            name = "no cycles"
            from = "domain::**"
            to = "infra::**"

            [[rules]]
            type = "no-cycles"
            name = "domain acyclic"
            scope = "domain::**"
        "#;
        let (mut config, _) = parse(toml).unwrap();
        config
            .add_implicit_rule(Path::new("arc-rules.toml"))
            .unwrap();
        assert_eq!(config.rules.len(), 2);
    }

    // ===== catch-all layer =====

    #[test]
    fn test_catch_all_bare_star_parses_as_catch_all_layer() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["*", "domain"]
            direction = "top-down"
        "#;
        let (config, _) = parse(toml).unwrap();
        let RuleKind::Layers(LayersRule { layers, .. }) = &config.rules[0].kind else {
            panic!("expected Layers, got {:?}", config.rules[0].kind);
        };
        assert_eq!(layers[0], Layer::CatchAll);
        assert_eq!(layers[1], Layer::Patterns(vec!["domain".to_string()]));
    }

    #[test]
    fn test_catch_all_single_element_list_parses_as_catch_all_layer() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = [["*"], "domain"]
            direction = "top-down"
        "#;
        let (config, _) = parse(toml).unwrap();
        let RuleKind::Layers(LayersRule { layers, .. }) = &config.rules[0].kind else {
            panic!("expected Layers, got {:?}", config.rules[0].kind);
        };
        assert_eq!(layers[0], Layer::CatchAll);
    }

    #[test]
    fn test_catch_all_alongside_other_patterns_fails_to_load() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = [["*", "core"], "domain"]
            direction = "top-down"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::CatchAllNotAlone { name, .. } if name == "architecture layers"),
            "got: {error:?}"
        );
        assert!(error.to_string().contains("architecture layers"));
    }

    #[test]
    fn test_two_catch_all_positions_fail_to_load() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["*", "*"]
            direction = "top-down"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::MultipleCatchAllPositions { name, .. } if name == "architecture layers"),
            "got: {error:?}"
        );
        assert!(error.to_string().contains("architecture layers"));
    }

    #[test]
    fn test_exhaustive_beside_a_catch_all_fails_to_load() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["*", "domain"]
            direction = "top-down"
            exhaustive = true
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::ExhaustiveWithCatchAll { name, .. } if name == "architecture layers"),
            "got: {error:?}"
        );
        let message = error.to_string();
        assert!(message.contains("architecture layers"));
        assert!(message.contains("exhaustive"));
    }

    #[test]
    fn test_an_exhaustive_rule_without_a_catch_all_loads() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain", "infra"]
            direction = "top-down"
            exhaustive = true
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let config = ArcConfig::load(&path).unwrap();
        assert_eq!(config.rules.len(), 2);
    }

    // ===== layers arity =====

    #[test]
    fn test_a_layer_position_without_patterns_fails_to_load() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = [[], "domain"]
            direction = "top-down"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::EmptyPosition { name, .. } if name == "architecture layers"),
            "got: {error:?}"
        );
        assert!(error.to_string().contains("architecture layers"));
    }

    #[test]
    fn test_a_single_layer_position_fails_to_load() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["domain"]
            direction = "top-down"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::TooFewPositions { name, .. } if name == "architecture layers"),
            "got: {error:?}"
        );
        assert!(error.to_string().contains("architecture layers"));
    }

    #[test]
    fn test_an_empty_layers_list_fails_to_load() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = []
            direction = "top-down"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::TooFewPositions { name, .. } if name == "architecture layers"),
            "got: {error:?}"
        );
    }

    #[test]
    fn test_a_lone_catch_all_position_fails_to_load() {
        let toml = r#"
            [[rules]]
            type = "layers"
            name = "architecture layers"
            layers = ["*"]
            direction = "top-down"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::TooFewPositions { name, .. } if name == "architecture layers"),
            "got: {error:?}"
        );
    }

    // ===== config format version =====

    #[test]
    fn load_rejects_unsupported_config_version() {
        let toml = r"
            [config]
            version = 2
        ";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arc-rules.toml");
        std::fs::write(&path, toml).unwrap();

        let error = ArcConfig::load(&path).unwrap_err();
        assert!(
            matches!(&error, ConfigError::UnsupportedVersion { found, .. } if *found == 2),
            "got: {error:?}"
        );
    }
}
