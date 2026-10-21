use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The max output bytes value.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;
/// The max page size value.
pub const MAX_PAGE_SIZE: usize = 100;
/// The min output bytes value.
pub const MIN_OUTPUT_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Response detail for paged collections.
pub enum Detail {
    #[default]
    /// Omit heavy node details.
    Compact,
    /// Include node details within the response byte budget.
    Full,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
/// Represents source context policy data.
pub struct SourceContextPolicy {
    /// Whether enabled by default applies.
    pub enabled_by_default: bool,
    /// The leading lines value.
    pub leading_lines: usize,
    /// Default source body lines, from 1 through 500.
    pub max_body_lines: usize,
}

impl Default for SourceContextPolicy {
    fn default() -> Self {
        Self {
            enabled_by_default: false,
            leading_lines: 5,
            max_body_lines: 200,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
/// Represents response policy data.
pub struct ResponsePolicy {
    /// The detail value.
    pub detail: Detail,
    /// The page size value.
    pub page_size: usize,
    /// The max output bytes value.
    pub max_output_bytes: usize,
    /// The source context value.
    pub source_context: SourceContextPolicy,
}

impl Default for ResponsePolicy {
    fn default() -> Self {
        Self {
            detail: Detail::Compact,
            page_size: 30,
            max_output_bytes: MAX_OUTPUT_BYTES,
            source_context: SourceContextPolicy::default(),
        }
    }
}

impl ResponsePolicy {
    /// Performs validate.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=MAX_PAGE_SIZE).contains(&self.page_size),
            "response.page_size must be 1..=100"
        );
        anyhow::ensure!(
            (MIN_OUTPUT_BYTES..=MAX_OUTPUT_BYTES).contains(&self.max_output_bytes),
            "response.max_output_bytes must be 1024..=65536"
        );
        anyhow::ensure!(
            self.source_context.leading_lines <= 20,
            "source leading_lines must be 0..=20"
        );
        anyhow::ensure!(
            (1..=500).contains(&self.source_context.max_body_lines),
            "source max_body_lines must be 1..=500"
        );
        Ok(())
    }
}

#[derive(Debug, Clone)]
/// Configures query operations.
pub struct QueryOptions {
    /// The limit value.
    pub limit: usize,
    /// The offset value.
    pub offset: usize,
    /// The detail value.
    pub detail: Detail,
    /// Optional generation value.
    pub generation: Option<String>,
}

impl QueryOptions {
    /// Performs resolve.
    pub fn resolve(
        policy: &ResponsePolicy,
        limit: Option<usize>,
        offset: usize,
        detail: Option<Detail>,
        generation: Option<String>,
    ) -> anyhow::Result<Self> {
        let limit = limit.unwrap_or(policy.page_size);
        anyhow::ensure!(
            (1..=MAX_PAGE_SIZE).contains(&limit),
            "limit must be 1..=100"
        );
        anyhow::ensure!(
            offset
                .checked_add(limit)
                .is_some_and(|end| end <= i64::MAX as usize),
            "offset is too large"
        );
        anyhow::ensure!(
            offset == 0 || generation.is_some(),
            "continuation requires generation from the previous page"
        );
        Ok(Self {
            limit,
            offset,
            detail: detail.unwrap_or(policy.detail),
            generation,
        })
    }
}

/// Controls whether symbol queries include paged resolution coverage.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoverageOptions {
    /// Whether include coverage applies.
    pub include_coverage: bool,
}

#[derive(Debug, Clone, Copy)]
/// Configures source operations.
pub struct SourceOptions {
    /// Whether enabled applies.
    pub enabled: bool,
    /// The leading lines value.
    pub leading_lines: usize,
    /// The max body lines value.
    pub max_body_lines: usize,
    /// The offset value.
    pub offset: usize,
}

impl SourceOptions {
    /// Performs resolve.
    pub fn resolve(
        policy: &ResponsePolicy,
        enabled: Option<bool>,
        leading_lines: Option<usize>,
        max_body_lines: Option<usize>,
        offset: usize,
    ) -> anyhow::Result<Self> {
        let defaults = &policy.source_context;
        let leading_lines = leading_lines.unwrap_or(defaults.leading_lines).min(20);
        let max_body_lines = max_body_lines
            .unwrap_or(defaults.max_body_lines)
            .clamp(1, 500);
        anyhow::ensure!(
            offset.checked_add(max_body_lines).is_some(),
            "source_offset is too large"
        );
        Ok(Self {
            enabled: enabled.unwrap_or(defaults.enabled_by_default),
            leading_lines,
            max_body_lines,
            offset,
        })
    }

    /// Configures a complete-symbol request whose response layer applies the byte bound.
    pub(crate) fn resolve_full_symbol(
        policy: &ResponsePolicy,
        leading_lines: Option<usize>,
        max_body_lines: Option<usize>,
        offset: usize,
    ) -> anyhow::Result<Self> {
        let defaults = &policy.source_context;
        let leading_lines = leading_lines.unwrap_or(defaults.leading_lines).min(20);
        let max_body_lines = max_body_lines
            .unwrap_or_else(|| usize::MAX.saturating_sub(offset))
            .max(1);
        anyhow::ensure!(
            offset.checked_add(max_body_lines).is_some(),
            "source_offset is too large"
        );
        Ok(Self {
            enabled: true,
            leading_lines,
            max_body_lines,
            offset,
        })
    }
}

#[cfg(test)]
#[test]
fn source_preview_line_budgets_are_clamped() {
    let mut failures = Vec::new();
    let policy = ResponsePolicy::default();
    for (requested, expected) in [
        (None, 200),
        (Some(0), 1),
        (Some(1), 1),
        (Some(200), 200),
        (Some(500), 500),
        (Some(501), 500),
        (Some(usize::MAX), 500),
    ] {
        match SourceOptions::resolve(&policy, Some(true), Some(30), requested, 0) {
            Ok(options) => {
                assert_eq!(options.leading_lines, 20);
                if options.max_body_lines != expected {
                    failures.push(format!(
                        "requested {requested:?}: expected {expected} body lines, got {}",
                        options.max_body_lines
                    ));
                }
            }
            Err(error) => failures.push(format!("requested {requested:?}: {error}")),
        }
    }
    for (configured, accepted) in [
        (0, false),
        (1, true),
        (200, true),
        (500, true),
        (501, false),
    ] {
        let mut policy = policy.clone();
        policy.source_context.max_body_lines = configured;
        if policy.validate().is_ok() != accepted {
            failures.push(format!(
                "policy body lines {configured}: expected accepted={accepted}"
            ));
        }
    }
    for (requested, offset, expected) in [
        (None, 0, usize::MAX),
        (None, 35, usize::MAX - 35),
        (Some(501), 0, 501),
        (Some(35), 0, 35),
    ] {
        match SourceOptions::resolve_full_symbol(&policy, None, requested, offset) {
            Ok(options) if options.max_body_lines == expected => {}
            Ok(options) => failures.push(format!(
                "full-symbol request {requested:?} at {offset}: expected {expected}, got {}",
                options.max_body_lines
            )),
            Err(error) => failures.push(format!(
                "full-symbol request {requested:?} at {offset}: {error}"
            )),
        }
    }
    if SourceOptions::resolve_full_symbol(&policy, None, Some(usize::MAX), 1).is_ok() {
        failures.push("overflowing full-symbol source offset was accepted".to_owned());
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
