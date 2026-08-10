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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidArguments;

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
