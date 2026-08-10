mod alert_instances;
mod alert_rules;
mod create_silence;
mod logql;
mod profiles;
mod promql;
mod traceql;

#[cfg(test)]
pub(crate) use logql::Direction;
pub use logql::{LogqlInput, Query};
#[cfg(test)]
pub(crate) use profiles::{DEFAULT_MAX_NODES, DEFAULT_PROFILE_TYPE};
pub use profiles::{ProfilesInput, ProfilesQuery};
pub use promql::{PromqlInput, PromqlQuery};
pub use traceql::{TraceqlInput, TraceqlQuery};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidArguments;

const MAX_MATCHERS: usize = 20;
const MAX_MATCHER_NAME_BYTES: usize = 128;
const MAX_MATCHER_VALUE_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum MatcherOperator {
    #[serde(rename = "=")]
    Equal,
    #[serde(rename = "!=")]
    NotEqual,
    #[serde(rename = "=~")]
    RegexEqual,
    #[serde(rename = "!~")]
    RegexNotEqual,
}

impl MatcherOperator {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::NotEqual => "!=",
            Self::RegexEqual => "=~",
            Self::RegexNotEqual => "!~",
        }
    }

    pub(crate) const fn is_equal(self) -> bool {
        matches!(self, Self::Equal | Self::RegexEqual)
    }

    pub(crate) const fn is_regex(self) -> bool {
        matches!(self, Self::RegexEqual | Self::RegexNotEqual)
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LabelMatcher {
    /// Prometheus label name.
    pub name: String,
    /// Label comparison operator.
    pub operator: MatcherOperator,
    /// Label value or regular expression.
    pub value: String,
}

fn validate_matchers(matchers: &[LabelMatcher], allow_empty: bool) -> Result<(), InvalidArguments> {
    if matchers.len() > MAX_MATCHERS || (!allow_empty && matchers.is_empty()) {
        return Err(InvalidArguments);
    }
    for matcher in matchers {
        let mut characters = matcher.name.bytes();
        if matcher.name.len() > MAX_MATCHER_NAME_BYTES
            || !characters
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            || !characters.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || matcher.value.len() > MAX_MATCHER_VALUE_BYTES
        {
            return Err(InvalidArguments);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Instant,
    Range,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Instant => "instant",
            Self::Range => "range",
        }
    }
}

fn parse_timestamp(value: Option<String>) -> Result<Option<DateTime<Utc>>, InvalidArguments> {
    value
        .map(|value| DateTime::parse_from_rfc3339(&value).map(|time| time.to_utc()))
        .transpose()
        .map_err(|_| InvalidArguments)
}

fn valid_range(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    maximum: chrono::Duration,
) -> Result<chrono::Duration, InvalidArguments> {
    let range = end.signed_duration_since(start);
    if range < chrono::Duration::zero() || range > maximum {
        return Err(InvalidArguments);
    }
    Ok(range)
}
pub use alert_instances::{AlertInstancesInput, AlertInstancesQuery};
pub use alert_rules::{AlertRulesInput, AlertRulesQuery};
pub use create_silence::{CreateSilenceCommand, CreateSilenceInput};
