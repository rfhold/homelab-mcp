use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{InvalidArguments, valid_range};

const MAX_PROFILE_RANGE: chrono::Duration = chrono::Duration::hours(1);
pub const DEFAULT_PROFILE_TYPE: &str = "process_cpu:cpu:nanoseconds:cpu:nanoseconds";
pub const DEFAULT_MAX_NODES: u16 = 256;
pub const MAX_MAX_NODES: u16 = 1000;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfilesInput {
    /// Pyroscope label selector to query.
    pub selector: String,
    /// Inclusive range start as an RFC3339 timestamp.
    pub start: String,
    /// Inclusive range end as an RFC3339 timestamp.
    pub end: String,
    /// Pyroscope profile type, defaulting to process CPU.
    pub profile_type: Option<String>,
    /// Maximum returned flame graph nodes, from 1 through 1000.
    pub max_nodes: Option<u16>,
}

pub struct ProfilesQuery {
    pub(crate) selector: String,
    pub(crate) start: DateTime<Utc>,
    pub(crate) end: DateTime<Utc>,
    pub(crate) profile_type: String,
    pub(crate) max_nodes: u16,
}

impl ProfilesInput {
    pub fn validate(self) -> Result<ProfilesQuery, InvalidArguments> {
        if self.selector.trim().is_empty() {
            return Err(InvalidArguments);
        }
        let profile_type = self
            .profile_type
            .unwrap_or_else(|| DEFAULT_PROFILE_TYPE.to_owned());
        if profile_type.trim().is_empty() {
            return Err(InvalidArguments);
        }
        let max_nodes = self.max_nodes.unwrap_or(DEFAULT_MAX_NODES);
        if !(1..=MAX_MAX_NODES).contains(&max_nodes) {
            return Err(InvalidArguments);
        }
        let start = DateTime::parse_from_rfc3339(&self.start)
            .map_err(|_| InvalidArguments)?
            .to_utc();
        let end = DateTime::parse_from_rfc3339(&self.end)
            .map_err(|_| InvalidArguments)?
            .to_utc();
        valid_range(start, end, MAX_PROFILE_RANGE)?;
        Ok(ProfilesQuery {
            selector: self.selector,
            start,
            end,
            profile_type,
            max_nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ProfilesInput {
        ProfilesInput {
            selector: "{service_name=\"api\"}".to_owned(),
            start: "2026-08-09T10:00:00Z".to_owned(),
            end: "2026-08-09T11:00:00Z".to_owned(),
            profile_type: None,
            max_nodes: None,
        }
    }

    #[test]
    fn validates_defaults_and_bounds() {
        let profile = input().validate().unwrap();
        assert_eq!(profile.profile_type, DEFAULT_PROFILE_TYPE);
        assert_eq!(profile.max_nodes, DEFAULT_MAX_NODES);
        let mut invalid_range = input();
        invalid_range.end = "2026-08-09T11:00:00.001Z".to_owned();
        assert!(invalid_range.validate().is_err());
        let mut invalid_nodes = input();
        invalid_nodes.max_nodes = Some(MAX_MAX_NODES + 1);
        assert!(invalid_nodes.validate().is_err());
        let mut empty_selector = input();
        empty_selector.selector = " ".to_owned();
        assert!(empty_selector.validate().is_err());
    }
}
