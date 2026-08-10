use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{InvalidArguments, Mode};

pub const DEFAULT_LIMIT: u16 = 1000;
pub const MAX_LIMIT: u16 = 5000;

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Forward,
    Backward,
}

impl Direction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Backward => "backward",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogqlInput {
    /// LogQL query to execute.
    pub query: String,
    /// Inclusive range start as an RFC3339 timestamp.
    pub start: Option<String>,
    /// Inclusive range end as an RFC3339 timestamp.
    pub end: Option<String>,
    /// Instant query time as an RFC3339 timestamp.
    pub time: Option<String>,
    /// Range traversal direction, defaulting to backward.
    pub direction: Option<Direction>,
    /// Maximum returned entries or samples, from 1 through 5000.
    pub limit: Option<u16>,
}

pub struct Query {
    pub(crate) query: String,
    pub(crate) mode: Mode,
    pub(crate) start: Option<DateTime<Utc>>,
    pub(crate) end: Option<DateTime<Utc>>,
    pub(crate) time: Option<DateTime<Utc>>,
    pub(crate) direction: Option<Direction>,
    pub(crate) limit: u16,
}

impl LogqlInput {
    pub fn validate(self) -> Result<Query, InvalidArguments> {
        if self.query.trim().is_empty() {
            return Err(InvalidArguments);
        }
        let limit = self.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(InvalidArguments);
        }
        let parse = |value: Option<String>| {
            value
                .map(|value| DateTime::parse_from_rfc3339(&value).map(|time| time.to_utc()))
                .transpose()
                .map_err(|_| InvalidArguments)
        };
        let start = parse(self.start)?;
        let end = parse(self.end)?;
        let time = parse(self.time)?;

        match (start, end) {
            (Some(start), Some(end)) => {
                if time.is_some()
                    || start > end
                    || end.signed_duration_since(start) > chrono::Duration::hours(24)
                {
                    return Err(InvalidArguments);
                }
                Ok(Query {
                    query: self.query,
                    mode: Mode::Range,
                    start: Some(start),
                    end: Some(end),
                    time: None,
                    direction: Some(self.direction.unwrap_or(Direction::Backward)),
                    limit,
                })
            }
            (None, None) => {
                if self.direction.is_some() {
                    return Err(InvalidArguments);
                }
                Ok(Query {
                    query: self.query,
                    mode: Mode::Instant,
                    start: None,
                    end: None,
                    time,
                    direction: None,
                    limit,
                })
            }
            _ => Err(InvalidArguments),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> LogqlInput {
        LogqlInput {
            query: "{job=\"test\"}".to_owned(),
            start: None,
            end: None,
            time: None,
            direction: None,
            limit: None,
        }
    }

    #[test]
    fn validates_modes_defaults_and_bounds() {
        let instant = input().validate().unwrap();
        assert_eq!(instant.mode, Mode::Instant);
        assert_eq!(instant.limit, DEFAULT_LIMIT);

        let mut range = input();
        range.start = Some("2026-08-09T10:00:00Z".to_owned());
        range.end = Some("2026-08-09T10:00:00Z".to_owned());
        let range = range.validate().unwrap();
        assert_eq!(range.mode, Mode::Range);
        assert_eq!(range.direction, Some(Direction::Backward));
    }

    #[test]
    fn rejects_all_invalid_combinations() {
        let invalid = [
            LogqlInput {
                query: " ".to_owned(),
                ..input()
            },
            LogqlInput {
                start: Some("2026-08-09T10:00:00Z".to_owned()),
                ..input()
            },
            LogqlInput {
                direction: Some(Direction::Forward),
                ..input()
            },
            LogqlInput {
                limit: Some(0),
                ..input()
            },
            LogqlInput {
                time: Some("not-time".to_owned()),
                ..input()
            },
            LogqlInput {
                start: Some("2026-08-10T11:00:00Z".to_owned()),
                end: Some("2026-08-09T10:00:00Z".to_owned()),
                ..input()
            },
            LogqlInput {
                start: Some("2026-08-09T10:00:00Z".to_owned()),
                end: Some("2026-08-10T10:00:00.001Z".to_owned()),
                ..input()
            },
            LogqlInput {
                start: Some("2026-08-09T10:00:00Z".to_owned()),
                end: Some("2026-08-09T11:00:00Z".to_owned()),
                time: Some("2026-08-09T10:30:00Z".to_owned()),
                ..input()
            },
        ];
        assert!(invalid.into_iter().all(|input| input.validate().is_err()));
    }

    #[test]
    fn rejects_unknown_input_fields() {
        assert!(
            serde_json::from_value::<LogqlInput>(serde_json::json!({
                "query": "{}", "datasource": "other"
            }))
            .is_err()
        );
    }
}
