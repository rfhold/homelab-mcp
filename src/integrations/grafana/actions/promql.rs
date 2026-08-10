use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{InvalidArguments, Mode, parse_timestamp, valid_range};

const MAX_RANGE: chrono::Duration = chrono::Duration::hours(24);
pub const MAX_PROMQL_POINTS: u64 = 11_000;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromqlInput {
    /// PromQL query to execute.
    pub query: String,
    /// Inclusive range start as an RFC3339 timestamp.
    pub start: Option<String>,
    /// Inclusive range end as an RFC3339 timestamp.
    pub end: Option<String>,
    /// Positive Prometheus duration used as the range query step.
    pub step: Option<String>,
    /// Instant query time as an RFC3339 timestamp.
    pub time: Option<String>,
}

pub struct PromqlQuery {
    pub(crate) query: String,
    pub(crate) mode: Mode,
    pub(crate) start: Option<DateTime<Utc>>,
    pub(crate) end: Option<DateTime<Utc>>,
    pub(crate) step: Option<String>,
    pub(crate) time: Option<DateTime<Utc>>,
}

impl PromqlInput {
    pub fn validate(self) -> Result<PromqlQuery, InvalidArguments> {
        if self.query.trim().is_empty() {
            return Err(InvalidArguments);
        }
        let start = parse_timestamp(self.start)?;
        let end = parse_timestamp(self.end)?;
        let time = parse_timestamp(self.time)?;
        match (start, end, self.step) {
            (Some(start), Some(end), Some(step)) => {
                let step_nanos = prometheus_duration_nanos(&step).ok_or(InvalidArguments)?;
                let range = valid_range(start, end, MAX_RANGE)?;
                let range_nanos = u64::try_from(range.num_nanoseconds().ok_or(InvalidArguments)?)
                    .map_err(|_| InvalidArguments)?;
                if range_nanos / step_nanos + 1 > MAX_PROMQL_POINTS || time.is_some() {
                    return Err(InvalidArguments);
                }
                Ok(PromqlQuery {
                    query: self.query,
                    mode: Mode::Range,
                    start: Some(start),
                    end: Some(end),
                    step: Some(step),
                    time: None,
                })
            }
            (None, None, None) => Ok(PromqlQuery {
                query: self.query,
                mode: Mode::Instant,
                start: None,
                end: None,
                step: None,
                time,
            }),
            _ => Err(InvalidArguments),
        }
    }
}

fn prometheus_duration_nanos(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    let mut offset = 0;
    let mut total = 0_u64;
    let mut previous_rank = u8::MAX;
    while offset < bytes.len() {
        let digits_start = offset;
        while offset < bytes.len() && bytes[offset].is_ascii_digit() {
            offset += 1;
        }
        if digits_start == offset {
            return None;
        }
        let amount = value[digits_start..offset].parse::<u64>().ok()?;
        let (unit_nanos, rank, unit_length) = if value[offset..].starts_with("ms") {
            (1_000_000_u64, 0, 2)
        } else {
            let unit = *bytes.get(offset)?;
            match unit {
                b's' => (1_000_000_000, 1, 1),
                b'm' => (60 * 1_000_000_000, 2, 1),
                b'h' => (60 * 60 * 1_000_000_000, 3, 1),
                b'd' => (24 * 60 * 60 * 1_000_000_000, 4, 1),
                b'w' => (7 * 24 * 60 * 60 * 1_000_000_000, 5, 1),
                b'y' => (365 * 24 * 60 * 60 * 1_000_000_000, 6, 1),
                _ => return None,
            }
        };
        if rank >= previous_rank {
            return None;
        }
        previous_rank = rank;
        offset += unit_length;
        total = total.checked_add(amount.checked_mul(unit_nanos)?)?;
    }
    (total > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> PromqlInput {
        PromqlInput {
            query: "up".to_owned(),
            start: None,
            end: None,
            step: None,
            time: None,
        }
    }

    #[test]
    fn validates_modes_durations_ranges_and_point_bound() {
        assert_eq!(input().validate().unwrap().mode, Mode::Instant);
        let mut range = input();
        range.start = Some("2026-08-09T10:00:00Z".to_owned());
        range.end = Some("2026-08-09T11:00:00Z".to_owned());
        range.step = Some("1m".to_owned());
        assert_eq!(range.validate().unwrap().mode, Mode::Range);
        assert_eq!(
            prometheus_duration_nanos("1h30m5s"),
            Some(5_405_000_000_000)
        );

        let mut zero_step = input();
        zero_step.start = Some("2026-08-09T10:00:00Z".to_owned());
        zero_step.end = Some("2026-08-09T11:00:00Z".to_owned());
        zero_step.step = Some("0s".to_owned());
        assert!(zero_step.validate().is_err());

        let mut too_many_points = input();
        too_many_points.start = Some("2026-08-09T10:00:00Z".to_owned());
        too_many_points.end = Some("2026-08-09T10:00:11Z".to_owned());
        too_many_points.step = Some("1ms".to_owned());
        assert!(too_many_points.validate().is_err());

        let mut exactly_max_points = input();
        exactly_max_points.start = Some("2026-08-09T10:00:00Z".to_owned());
        exactly_max_points.end = Some("2026-08-09T10:00:10.999Z".to_owned());
        exactly_max_points.step = Some("1ms".to_owned());
        assert!(exactly_max_points.validate().is_ok());

        let mut too_long = input();
        too_long.start = Some("2026-08-09T10:00:00Z".to_owned());
        too_long.end = Some("2026-08-10T10:00:00.001Z".to_owned());
        too_long.step = Some("1h".to_owned());
        assert!(too_long.validate().is_err());

        let mut incomplete = input();
        incomplete.start = Some("2026-08-09T10:00:00Z".to_owned());
        assert!(incomplete.validate().is_err());
        assert!(prometheus_duration_nanos("1m1h").is_none());
        assert!(prometheus_duration_nanos("1").is_none());
    }
}
