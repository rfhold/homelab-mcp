use schemars::JsonSchema;
use serde::Deserialize;

use super::{InvalidArguments, LabelMatcher, validate_matchers};

pub const DEFAULT_ALERT_INSTANCE_LIMIT: u16 = 50;
pub const MAX_ALERT_INSTANCE_LIMIT: u16 = 100;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlertInstancesInput {
    /// Label matchers used to filter current alerts, up to 20.
    #[serde(default)]
    pub matchers: Vec<LabelMatcher>,
    /// Maximum returned alert instances, from 1 through 100.
    pub limit: Option<u16>,
}

pub struct AlertInstancesQuery {
    pub(crate) matchers: Vec<LabelMatcher>,
    pub(crate) limit: u16,
}

impl AlertInstancesInput {
    pub fn validate(self) -> Result<AlertInstancesQuery, InvalidArguments> {
        validate_matchers(&self.matchers, true)?;
        let limit = self.limit.unwrap_or(DEFAULT_ALERT_INSTANCE_LIMIT);
        if !(1..=MAX_ALERT_INSTANCE_LIMIT).contains(&limit) {
            return Err(InvalidArguments);
        }
        Ok(AlertInstancesQuery {
            matchers: self.matchers,
            limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::grafana::actions::MatcherOperator;

    #[test]
    fn validates_defaults_matchers_and_bounds() {
        assert_eq!(
            AlertInstancesInput {
                matchers: Vec::new(),
                limit: None,
            }
            .validate()
            .unwrap()
            .limit,
            DEFAULT_ALERT_INSTANCE_LIMIT
        );
        assert!(
            AlertInstancesInput {
                matchers: vec![LabelMatcher {
                    name: "severity".to_owned(),
                    operator: MatcherOperator::Equal,
                    value: "critical".to_owned(),
                }],
                limit: Some(MAX_ALERT_INSTANCE_LIMIT),
            }
            .validate()
            .is_ok()
        );
        assert!(
            AlertInstancesInput {
                matchers: Vec::new(),
                limit: Some(0),
            }
            .validate()
            .is_err()
        );
        assert!(
            AlertInstancesInput {
                matchers: (0..21)
                    .map(|_| LabelMatcher {
                        name: "job".to_owned(),
                        operator: MatcherOperator::Equal,
                        value: "api".to_owned(),
                    })
                    .collect(),
                limit: None,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(
            serde_json::from_value::<AlertInstancesInput>(serde_json::json!({"receiver": "all"}))
                .is_err()
        );
    }
}
