use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// A typed value retained inside one framework rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FrameworkManifestValue {
    /// Decoded string.
    String(String),
    /// Signed integer.
    Integer(i64),
    /// Floating-point number.
    Float(f64),
    /// Boolean.
    Boolean(bool),
    /// TOML date or time value.
    DateTime(toml::value::Datetime),
    /// Ordered array values.
    Array(Vec<FrameworkManifestValue>),
    /// Table fields in deterministic key order.
    Table(BTreeMap<String, FrameworkManifestValue>),
}

/// Framework rules separated by the linker-owned rule domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkManifest {
    /// Framework identifier, taken from the user manifest's filename stem.
    #[serde(skip)]
    pub name: String,
    /// Selects merging explicitly when true; merging is also the default.
    #[serde(default, skip_serializing)]
    extend: bool,
    /// Selects replacement of the bundled table when true.
    #[serde(default, skip_serializing)]
    replace: bool,
    /// Receiver rules consumed by the language linker.
    pub receivers: Vec<FrameworkManifestValue>,
    /// Builtin rules consumed by the language linker.
    pub builtins: Vec<FrameworkManifestValue>,
    /// Template-filter rules consumed by the language linker.
    pub filters: Vec<FrameworkManifestValue>,
    /// Route-pattern rules consumed by the language linker.
    pub routes: Vec<FrameworkManifestValue>,
    /// Members inherited from known external class hierarchies.
    #[serde(default)]
    pub class_hierarchy_receivers: Vec<ClassHierarchyReceiverRule>,
    /// Routes declared with framework decorators.
    #[serde(default)]
    pub route_decorators: Vec<RouteDecoratorRule>,
    /// Routes registered with framework calls.
    #[serde(default)]
    pub route_calls: Vec<RouteCallRule>,
    /// Provider references created by dependency-injection calls.
    #[serde(default)]
    pub di_calls: Vec<DiCallRule>,
}

/// External members supplied by a known base class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassHierarchyReceiverRule {
    /// Fully qualified base identities accepted by this rule.
    pub bases: Vec<String>,
    /// Provider identity recorded for inherited members.
    pub provider: String,
    /// Member paths supplied by the base.
    pub members: Vec<String>,
}

/// Route metadata extracted from a decorator call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDecoratorRule {
    /// Callee identity. A leading dot matches a qualified member suffix.
    pub callee: String,
    /// HTTP method represented by the decorator.
    pub method: String,
    /// Zero-based positional argument containing the route path.
    pub path_argument: usize,
    /// Optional receiver basenames required for member-call decorators.
    #[serde(default)]
    pub receiver_names: Vec<String>,
    /// Whether route paths may omit a leading slash or caret.
    #[serde(default)]
    pub allow_relative_path: bool,
}

/// Route metadata extracted from a registration call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCallRule {
    /// Callee identity.
    pub callee: String,
    /// HTTP method represented by the registration.
    pub method: String,
    /// Zero-based positional argument containing the route path.
    pub path_argument: usize,
    /// First handler position; later positional handlers are retained as middleware.
    #[serde(default)]
    pub handler_argument: Option<usize>,
    /// Optional receiver basenames required for member-call registrations.
    #[serde(default)]
    pub receiver_names: Vec<String>,
    /// Whether route paths may omit a leading slash or caret.
    #[serde(default)]
    pub allow_relative_path: bool,
}

/// A dependency-injection call argument that names a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiCallRule {
    /// Callee identity. A leading dot matches a qualified member suffix.
    pub callee: String,
    /// Zero-based positional argument containing the provider.
    pub argument: usize,
}

impl FrameworkManifest {
    fn merge_rules(
        project: &[FrameworkManifestValue],
        bundled: &[FrameworkManifestValue],
        identity: impl Fn(&str) -> String,
    ) -> Vec<FrameworkManifestValue> {
        let mut seen = BTreeSet::new();
        project
            .iter()
            .chain(bundled)
            .filter(|rule| match rule {
                FrameworkManifestValue::String(value) => seen.insert(identity(value)),
                _ => true,
            })
            .cloned()
            .collect()
    }

    /// Merges project rules before bundled rules, unless replacement is explicit.
    pub(crate) fn with_bundled_rules(self, bundled: &Self) -> Self {
        if self.replace {
            return Self {
                extend: false,
                replace: false,
                ..self
            };
        }

        let source_identity = |rule: &str| {
            rule.split_once('=')
                .map_or(rule, |(source, _)| source)
                .to_owned()
        };
        Self {
            name: self.name,
            extend: false,
            replace: false,
            receivers: Self::merge_rules(&self.receivers, &bundled.receivers, source_identity),
            builtins: Self::merge_rules(&self.builtins, &bundled.builtins, str::to_owned),
            filters: Self::merge_rules(&self.filters, &bundled.filters, str::to_owned),
            routes: Self::merge_rules(&self.routes, &bundled.routes, source_identity),
            class_hierarchy_receivers: merge_typed_rules(
                self.class_hierarchy_receivers,
                &bundled.class_hierarchy_receivers,
                |rule| rule.bases.join("\u{0}"),
            ),
            route_decorators: merge_typed_rules(
                self.route_decorators,
                &bundled.route_decorators,
                |rule| format!("{}\u{0}{}", rule.callee, rule.method),
            ),
            route_calls: merge_typed_rules(self.route_calls, &bundled.route_calls, |rule| {
                format!("{}\u{0}{}", rule.callee, rule.method)
            }),
            di_calls: merge_typed_rules(self.di_calls, &bundled.di_calls, |rule| {
                rule.callee.clone()
            }),
        }
    }

    /// Checks typed rules against the shape needed by extraction.
    fn validate(&self) -> Result<()> {
        for rule in &self.class_hierarchy_receivers {
            anyhow::ensure!(
                !rule.bases.is_empty()
                    && rule.bases.iter().all(|base| !base.trim().is_empty())
                    && !rule.provider.trim().is_empty()
                    && !rule.members.is_empty()
                    && rule.members.iter().all(|member| !member.trim().is_empty()),
                "class hierarchy receiver rules need bases, a provider, and members"
            );
        }
        for rule in &self.route_decorators {
            validate_callee(&rule.callee)?;
            validate_method(&rule.method)?;
            validate_receiver_names(&rule.receiver_names)?;
        }
        for rule in &self.route_calls {
            validate_callee(&rule.callee)?;
            validate_method(&rule.method)?;
            validate_receiver_names(&rule.receiver_names)?;
            anyhow::ensure!(
                rule.handler_argument != Some(rule.path_argument),
                "route path and handler arguments must differ"
            );
        }
        for rule in &self.di_calls {
            validate_callee(&rule.callee)?;
        }
        Ok(())
    }

    fn contains_rule(rules: &[FrameworkManifestValue], rule: &str) -> bool {
        rules
            .iter()
            .any(|value| matches!(value, FrameworkManifestValue::String(value) if value == rule))
    }

    /// Returns whether a receiver rule is present in this framework table.
    pub(crate) fn has_receiver(&self, rule: &str) -> bool {
        Self::contains_rule(&self.receivers, rule)
    }

    /// Returns whether a builtin rule is present in this framework table.
    pub(crate) fn has_builtin(&self, rule: &str) -> bool {
        Self::contains_rule(&self.builtins, rule)
    }

    /// Returns whether a template-filter rule is present in this framework table.
    pub(crate) fn has_filter(&self, rule: &str) -> bool {
        Self::contains_rule(&self.filters, rule)
    }

    /// Resolves a receiver rule encoded as `source=target`.
    pub(crate) fn receiver_target(&self, source: &str) -> Option<&str> {
        Self::mapping_target(&self.receivers, source)
    }

    /// Resolves a route rule encoded as `source=target`.
    pub(crate) fn route_target(&self, source: &str) -> Option<&str> {
        Self::mapping_target(&self.routes, source)
    }

    fn mapping_target<'a>(rules: &'a [FrameworkManifestValue], source: &str) -> Option<&'a str> {
        rules.iter().find_map(|rule| {
            let FrameworkManifestValue::String(rule) = rule else {
                return None;
            };
            let (candidate, target) = rule.split_once('=')?;
            (candidate == source).then_some(target)
        })
    }

    /// Parses one framework manifest according to its filename extension.
    ///
    /// All four rule arrays are required by the type, so missing or non-array
    /// sections fail during deserialization. TOML semantic validation is
    /// delegated to the TOML parser and YAML shape validation to serde_yaml.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported extensions, malformed input, missing
    /// or wrongly typed sections, invalid typed rules, or unknown sections.
    pub(crate) fn parse(name: String, extension: &str, source: &str) -> Result<Self> {
        let mut manifest = match extension {
            "toml" => toml::from_str::<Self>(source)
                .with_context(|| format!("invalid TOML framework manifest {name}"))?,
            "yaml" | "yml" => serde_yaml::from_str::<Self>(source)
                .with_context(|| format!("invalid YAML framework manifest {name}"))?,
            other => bail!("unsupported framework manifest extension .{other}"),
        };
        if manifest.extend && manifest.replace {
            bail!("framework manifest cannot set both extend and replace to true");
        }
        manifest.validate()?;
        manifest.name = name;
        Ok(manifest)
    }
}

fn merge_typed_rules<T: Clone>(
    project: Vec<T>,
    bundled: &[T],
    identity: impl Fn(&T) -> String,
) -> Vec<T> {
    let mut seen = BTreeSet::new();
    project
        .iter()
        .chain(bundled)
        .filter(|rule| seen.insert(identity(rule)))
        .cloned()
        .collect()
}

fn validate_callee(callee: &str) -> Result<()> {
    anyhow::ensure!(
        !callee.trim().is_empty() && !callee.chars().any(char::is_whitespace),
        "framework rule callee must be a non-empty identifier path"
    );
    Ok(())
}

fn validate_method(method: &str) -> Result<()> {
    anyhow::ensure!(
        !method.trim().is_empty()
            && method.chars().all(|character| {
                character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
            }),
        "framework route method must be uppercase"
    );
    Ok(())
}

fn validate_receiver_names(names: &[String]) -> Result<()> {
    anyhow::ensure!(
        names.iter().all(|name| {
            !name.trim().is_empty()
                && name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        }),
        "route receiver names must be non-empty identifiers"
    );
    Ok(())
}
