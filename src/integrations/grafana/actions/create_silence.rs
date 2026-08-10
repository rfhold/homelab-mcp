use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;

use super::{InvalidArguments, LabelMatcher, validate_matchers};

pub const MAX_SILENCE_DURATION_SECONDS: u32 = 7 * 24 * 60 * 60;
pub const MAX_COMMENT_BYTES: usize = 512;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSilenceInput {
    /// Matchers selecting alerts to silence, from 1 through 20.
    pub matchers: Vec<LabelMatcher>,
    /// Positive silence duration in seconds, capped at seven days.
    pub duration_seconds: u32,
    /// Required operator comment, up to 512 UTF-8 bytes.
    pub comment: String,
}

pub struct CreateSilenceCommand {
    pub(crate) matchers: Vec<LabelMatcher>,
    pub(crate) duration: Duration,
    pub(crate) comment: String,
}

impl CreateSilenceInput {
    pub fn validate(self) -> Result<CreateSilenceCommand, InvalidArguments> {
        validate_matchers(&self.matchers, false)?;
        if !(1..=MAX_SILENCE_DURATION_SECONDS).contains(&self.duration_seconds)
            || self.comment.trim().is_empty()
            || self.comment.len() > MAX_COMMENT_BYTES
        {
            return Err(InvalidArguments);
        }
        Ok(CreateSilenceCommand {
            matchers: self.matchers,
            duration: Duration::from_secs(u64::from(self.duration_seconds)),
            comment: self.comment,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::grafana::actions::MatcherOperator;

    fn input() -> CreateSilenceInput {
        CreateSilenceInput {
            matchers: vec![LabelMatcher {
                name: "alertname".to_owned(),
                operator: MatcherOperator::RegexEqual,
                value: "API.*".to_owned(),
            }],
            duration_seconds: 3600,
            comment: "maintenance".to_owned(),
        }
    }

    #[test]
    fn validates_matchers_duration_and_comment() {
        assert_eq!(
            input().validate().unwrap().duration,
            Duration::from_secs(3600)
        );
        let mut zero = input();
        zero.duration_seconds = 0;
        assert!(zero.validate().is_err());
        let mut too_long = input();
        too_long.duration_seconds = MAX_SILENCE_DURATION_SECONDS + 1;
        assert!(too_long.validate().is_err());
        let mut empty_comment = input();
        empty_comment.comment = " \t".to_owned();
        assert!(empty_comment.validate().is_err());
        let mut oversized_comment = input();
        oversized_comment.comment = "x".repeat(MAX_COMMENT_BYTES + 1);
        assert!(oversized_comment.validate().is_err());
        let mut no_matchers = input();
        no_matchers.matchers.clear();
        assert!(no_matchers.validate().is_err());
    }

    #[test]
    fn rejects_invalid_matcher_names_and_values() {
        for matcher in [
            LabelMatcher {
                name: "bad-name".to_owned(),
                operator: MatcherOperator::Equal,
                value: "x".to_owned(),
            },
            LabelMatcher {
                name: "job".to_owned(),
                operator: MatcherOperator::Equal,
                value: "x".repeat(1025),
            },
        ] {
            let mut invalid = input();
            invalid.matchers = vec![matcher];
            assert!(invalid.validate().is_err());
        }
    }
}
