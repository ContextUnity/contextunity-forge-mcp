use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_PAGE_SIZE: usize = 100;
pub const MIN_OUTPUT_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    #[default]
    Compact,
    Full,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct SourceContextPolicy {
    pub enabled_by_default: bool,
    pub leading_lines: usize,
    pub max_body_lines: usize,
}

impl Default for SourceContextPolicy {
    fn default() -> Self {
        Self {
            enabled_by_default: false,
            leading_lines: 5,
            max_body_lines: 35,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ResponsePolicy {
    pub detail: Detail,
    pub page_size: usize,
    pub max_output_bytes: usize,
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
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=MAX_PAGE_SIZE).contains(&self.page_size),
            "response.page_size must be 1..=100"
        );
        anyhow::ensure!(
            (MIN_OUTPUT_BYTES..=MAX_OUTPUT_BYTES).contains(&self.max_output_bytes),
            "response.max_output_bytes must be 1024..=65536"
        );
        SourceOptions::resolve(self, None, None, None, 0)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct QueryOptions {
    pub limit: usize,
    pub offset: usize,
    pub detail: Detail,
    pub generation: Option<String>,
}

impl QueryOptions {
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

#[derive(Debug, Clone, Copy)]
pub struct SourceOptions {
    pub enabled: bool,
    pub leading_lines: usize,
    pub max_body_lines: usize,
    pub offset: usize,
}

impl SourceOptions {
    pub fn resolve(
        policy: &ResponsePolicy,
        enabled: Option<bool>,
        leading_lines: Option<usize>,
        max_body_lines: Option<usize>,
        offset: usize,
    ) -> anyhow::Result<Self> {
        let defaults = &policy.source_context;
        let leading_lines = leading_lines.unwrap_or(defaults.leading_lines);
        let max_body_lines = max_body_lines.unwrap_or(defaults.max_body_lines);
        anyhow::ensure!(leading_lines <= 20, "source leading_lines must be 0..=20");
        anyhow::ensure!(
            (1..=100).contains(&max_body_lines),
            "source max_body_lines must be 1..=100"
        );
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
}
