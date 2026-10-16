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
        }
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
    /// or wrongly typed sections, or unknown top-level sections.
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
        manifest.name = name;
        Ok(manifest)
    }
}
