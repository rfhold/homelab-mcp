use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{InvalidArguments, parse_timestamp, valid_range};

const MAX_RANGE: chrono::Duration = chrono::Duration::hours(24);
pub const DEFAULT_TRACE_LIMIT: u16 = 20;
pub const MAX_TRACE_LIMIT: u16 = 100;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceqlInput {
    /// TraceQL query to execute.
    pub query: String,
    /// Inclusive range start as an RFC3339 timestamp.
    pub start: Option<String>,
    /// Inclusive range end as an RFC3339 timestamp.
    pub end: Option<String>,
    /// Maximum returned traces, from 1 through 100.
    pub limit: Option<u16>,
}

pub struct TraceqlQuery {
    pub(crate) query: String,
    pub(crate) start: Option<DateTime<Utc>>,
    pub(crate) end: Option<DateTime<Utc>>,
    pub(crate) limit: u16,
}

impl TraceqlInput {
    pub fn validate(self) -> Result<TraceqlQuery, InvalidArguments> {
        if self.query.trim().is_empty() {
            return Err(InvalidArguments);
        }
        let limit = self.limit.unwrap_or(DEFAULT_TRACE_LIMIT);
        if !(1..=MAX_TRACE_LIMIT).contains(&limit) {
            return Err(InvalidArguments);
        }
        let start = parse_timestamp(self.start)?;
        let end = parse_timestamp(self.end)?;
        match (start, end) {
            (Some(start), Some(end)) => {
                valid_range(start, end, MAX_RANGE)?;
                Ok(TraceqlQuery {
                    query: self.query,
                    start: Some(start),
                    end: Some(end),
                    limit,
                })
            }
            (None, None) => Ok(TraceqlQuery {
                query: self.query,
                start: None,
                end: None,
                limit,
            }),
            _ => Err(InvalidArguments),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> TraceqlInput {
        TraceqlInput {
            query: "{ true }".to_owned(),
            start: None,
            end: None,
            limit: None,
        }
    }

    #[test]
    fn validates_defaults_and_bounds() {
        assert_eq!(input().validate().unwrap().limit, DEFAULT_TRACE_LIMIT);
        let mut range = input();
        range.start = Some("2026-08-09T10:00:00Z".to_owned());
        range.end = Some("2026-08-10T10:00:00Z".to_owned());
        range.limit = Some(MAX_TRACE_LIMIT);
        assert!(range.validate().is_ok());
        let mut invalid = input();
        invalid.limit = Some(MAX_TRACE_LIMIT + 1);
        assert!(invalid.validate().is_err());
        let mut incomplete = input();
        incomplete.start = Some("2026-08-09T10:00:00Z".to_owned());
        assert!(incomplete.validate().is_err());
    }
}
