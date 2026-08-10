use schemars::JsonSchema;
use serde::Deserialize;

use super::InvalidArguments;

pub const DEFAULT_ALERT_RULE_LIMIT: u16 = 50;
pub const MAX_ALERT_RULE_LIMIT: u16 = 100;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlertRulesInput {
    /// Maximum returned alert-rule summaries, from 1 through 100.
    pub limit: Option<u16>,
}

pub struct AlertRulesQuery {
    pub(crate) limit: u16,
}

impl AlertRulesInput {
    pub fn validate(self) -> Result<AlertRulesQuery, InvalidArguments> {
        let limit = self.limit.unwrap_or(DEFAULT_ALERT_RULE_LIMIT);
        if !(1..=MAX_ALERT_RULE_LIMIT).contains(&limit) {
            return Err(InvalidArguments);
        }
        Ok(AlertRulesQuery { limit })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_default_and_limit_bounds() {
        assert_eq!(
            AlertRulesInput { limit: None }.validate().unwrap().limit,
            DEFAULT_ALERT_RULE_LIMIT
        );
        assert!(AlertRulesInput { limit: Some(1) }.validate().is_ok());
        assert!(
            AlertRulesInput {
                limit: Some(MAX_ALERT_RULE_LIMIT)
            }
            .validate()
            .is_ok()
        );
        assert!(AlertRulesInput { limit: Some(0) }.validate().is_err());
        assert!(
            AlertRulesInput {
                limit: Some(MAX_ALERT_RULE_LIMIT + 1)
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(
            serde_json::from_value::<AlertRulesInput>(serde_json::json!({"limit": 1, "url": "x"}))
                .is_err()
        );
    }
}
