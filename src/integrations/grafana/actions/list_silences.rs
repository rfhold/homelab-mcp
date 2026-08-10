use schemars::JsonSchema;
use serde::Deserialize;

use super::InvalidArguments;

pub const DEFAULT_SILENCE_LIMIT: u16 = 50;
pub const MAX_SILENCE_LIMIT: u16 = 100;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSilencesInput {
    /// Optional silence state to return.
    pub state: Option<SilenceState>,
    /// Maximum returned silences, from 1 through 100.
    pub limit: Option<u16>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SilenceState {
    Active,
    Pending,
    Expired,
}

impl SilenceState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Pending => "pending",
            Self::Expired => "expired",
        }
    }
}

pub struct ListSilencesQuery {
    pub(crate) state: Option<SilenceState>,
    pub(crate) limit: u16,
}

impl ListSilencesInput {
    pub fn validate(self) -> Result<ListSilencesQuery, InvalidArguments> {
        let limit = self.limit.unwrap_or(DEFAULT_SILENCE_LIMIT);
        if !(1..=MAX_SILENCE_LIMIT).contains(&limit) {
            return Err(InvalidArguments);
        }
        Ok(ListSilencesQuery {
            state: self.state,
            limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_defaults_state_and_bounds() {
        let default = ListSilencesInput {
            state: None,
            limit: None,
        }
        .validate()
        .unwrap();
        assert_eq!(default.limit, DEFAULT_SILENCE_LIMIT);
        assert_eq!(default.state, None);

        let filtered = ListSilencesInput {
            state: Some(SilenceState::Active),
            limit: Some(MAX_SILENCE_LIMIT),
        }
        .validate()
        .unwrap();
        assert_eq!(filtered.state, Some(SilenceState::Active));

        assert!(
            ListSilencesInput {
                state: None,
                limit: Some(0),
            }
            .validate()
            .is_err()
        );
        assert!(
            ListSilencesInput {
                state: None,
                limit: Some(MAX_SILENCE_LIMIT + 1),
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields_and_states() {
        assert!(
            serde_json::from_value::<ListSilencesInput>(serde_json::json!({"status":"active"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<ListSilencesInput>(serde_json::json!({"state":"unknown"}))
                .is_err()
        );
    }
}
