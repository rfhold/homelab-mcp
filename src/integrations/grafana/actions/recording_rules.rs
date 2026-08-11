use schemars::JsonSchema;
use serde::Deserialize;

use super::InvalidArguments;

pub const DEFAULT_RECORDING_RULE_LIMIT: u16 = 50;
pub const MAX_RECORDING_RULE_LIMIT: u16 = 100;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordingRulesInput {
    /// Maximum returned recording-rule summaries, from 1 through 100.
    pub limit: Option<u16>,
}

pub struct RecordingRulesQuery {
    pub(crate) limit: u16,
}

impl RecordingRulesInput {
    pub fn validate(self) -> Result<RecordingRulesQuery, InvalidArguments> {
        let limit = self.limit.unwrap_or(DEFAULT_RECORDING_RULE_LIMIT);
        if !(1..=MAX_RECORDING_RULE_LIMIT).contains(&limit) {
            return Err(InvalidArguments);
        }
        Ok(RecordingRulesQuery { limit })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_default_and_limit_bounds() {
        assert_eq!(
            RecordingRulesInput { limit: None }
                .validate()
                .unwrap()
                .limit,
            DEFAULT_RECORDING_RULE_LIMIT
        );
        assert!(RecordingRulesInput { limit: Some(1) }.validate().is_ok());
        assert!(
            RecordingRulesInput {
                limit: Some(MAX_RECORDING_RULE_LIMIT)
            }
            .validate()
            .is_ok()
        );
        assert!(RecordingRulesInput { limit: Some(0) }.validate().is_err());
        assert!(
            RecordingRulesInput {
                limit: Some(MAX_RECORDING_RULE_LIMIT + 1)
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(
            serde_json::from_value::<RecordingRulesInput>(
                serde_json::json!({"limit": 1, "url": "x"})
            )
            .is_err()
        );
    }
}
