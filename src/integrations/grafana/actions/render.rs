use std::{collections::BTreeMap, str::FromStr};

use chrono::DateTime;
use chrono_tz::Tz;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{InvalidArguments, valid_dashboard_uid};

pub const DEFAULT_DASHBOARD_WIDTH: u16 = 1600;
pub const DEFAULT_DASHBOARD_HEIGHT: u16 = 1200;
pub const DEFAULT_PANEL_WIDTH: u16 = 1000;
pub const DEFAULT_PANEL_HEIGHT: u16 = 500;
pub const DEFAULT_RENDER_SCALE: u8 = 1;
pub const DEFAULT_RENDER_TIMEZONE: &str = "UTC";
pub const DEFAULT_RENDER_FROM: &str = "now-6h";
pub const DEFAULT_RENDER_TO: &str = "now";

const MIN_RENDER_WIDTH: u16 = 320;
const MAX_RENDER_WIDTH: u16 = 2000;
const MIN_RENDER_HEIGHT: u16 = 200;
const MAX_RENDER_HEIGHT: u16 = 2000;
const MAX_EFFECTIVE_PIXELS: u64 = 4_000_000;
const MAX_VARIABLES: usize = 20;
const MAX_VARIABLE_NAME_BYTES: usize = 64;
const MAX_VARIABLE_VALUE_BYTES: usize = 1024;
pub const MAX_RENDER_RANGE_BYTES: usize = 64;
pub const MAX_RENDER_TIMEZONE_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RenderTheme {
    Light,
    Dark,
}

impl RenderTheme {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderDashboardInput {
    /// Grafana dashboard UID.
    pub uid: String,
    /// Render range start: now-Ns|m|h|d or RFC3339.
    #[schemars(length(max = 64))]
    pub from: Option<String>,
    /// Render range end: now or RFC3339.
    #[schemars(length(max = 64))]
    pub to: Option<String>,
    /// Image width in pixels, from 320 through 2000.
    pub width: Option<u16>,
    /// Image height in pixels, from 200 through 2000.
    pub height: Option<u16>,
    /// Device scale factor, from 1 through 2.
    pub scale: Option<u8>,
    /// Render theme, defaulting to dark.
    pub theme: Option<RenderTheme>,
    /// IANA timezone name, defaulting to UTC.
    #[schemars(length(max = 32))]
    pub timezone: Option<String>,
    /// Dashboard template variables, up to 20 values.
    pub variables: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderPanelInput {
    /// Grafana dashboard UID.
    pub uid: String,
    /// Grafana panel identifier.
    pub panel_id: String,
    /// Render range start: now-Ns|m|h|d or RFC3339.
    #[schemars(length(max = 64))]
    pub from: Option<String>,
    /// Render range end: now or RFC3339.
    #[schemars(length(max = 64))]
    pub to: Option<String>,
    /// Image width in pixels, from 320 through 2000.
    pub width: Option<u16>,
    /// Image height in pixels, from 200 through 2000.
    pub height: Option<u16>,
    /// Device scale factor, from 1 through 2.
    pub scale: Option<u8>,
    /// Render theme, defaulting to dark.
    pub theme: Option<RenderTheme>,
    /// IANA timezone name, defaulting to UTC.
    #[schemars(length(max = 32))]
    pub timezone: Option<String>,
    /// Dashboard template variables, up to 20 values.
    pub variables: Option<BTreeMap<String, String>>,
}

pub struct RenderDashboardRequest {
    pub(crate) uid: String,
    pub(crate) options: RenderOptions,
}

pub struct RenderPanelRequest {
    pub(crate) uid: String,
    pub(crate) panel_id: String,
    pub(crate) options: RenderOptions,
}

pub struct RenderOptions {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) scale: u8,
    pub(crate) theme: RenderTheme,
    pub(crate) timezone: String,
    pub(crate) variables: BTreeMap<String, String>,
}

struct RenderControls {
    from: Option<String>,
    to: Option<String>,
    width: Option<u16>,
    height: Option<u16>,
    scale: Option<u8>,
    theme: Option<RenderTheme>,
    timezone: Option<String>,
    variables: Option<BTreeMap<String, String>>,
}

impl RenderDashboardInput {
    pub fn validate(self) -> Result<RenderDashboardRequest, InvalidArguments> {
        if !valid_dashboard_uid(&self.uid) {
            return Err(InvalidArguments);
        }
        let options = validate_controls(
            RenderControls {
                from: self.from,
                to: self.to,
                width: self.width,
                height: self.height,
                scale: self.scale,
                theme: self.theme,
                timezone: self.timezone,
                variables: self.variables,
            },
            DEFAULT_DASHBOARD_WIDTH,
            DEFAULT_DASHBOARD_HEIGHT,
        )?;
        Ok(RenderDashboardRequest {
            uid: self.uid,
            options,
        })
    }
}

impl RenderPanelInput {
    pub fn validate(self) -> Result<RenderPanelRequest, InvalidArguments> {
        if !valid_dashboard_uid(&self.uid) || !valid_panel_id(&self.panel_id) {
            return Err(InvalidArguments);
        }
        let options = validate_controls(
            RenderControls {
                from: self.from,
                to: self.to,
                width: self.width,
                height: self.height,
                scale: self.scale,
                theme: self.theme,
                timezone: self.timezone,
                variables: self.variables,
            },
            DEFAULT_PANEL_WIDTH,
            DEFAULT_PANEL_HEIGHT,
        )?;
        Ok(RenderPanelRequest {
            uid: self.uid,
            panel_id: self.panel_id,
            options,
        })
    }
}

fn validate_controls(
    controls: RenderControls,
    default_width: u16,
    default_height: u16,
) -> Result<RenderOptions, InvalidArguments> {
    let (from, to) = validate_range(controls.from, controls.to)?;
    let width = controls.width.unwrap_or(default_width);
    let height = controls.height.unwrap_or(default_height);
    let scale = controls.scale.unwrap_or(DEFAULT_RENDER_SCALE);
    if !(MIN_RENDER_WIDTH..=MAX_RENDER_WIDTH).contains(&width)
        || !(MIN_RENDER_HEIGHT..=MAX_RENDER_HEIGHT).contains(&height)
        || !(1..=2).contains(&scale)
        || u64::from(width) * u64::from(height) * u64::from(scale).pow(2) > MAX_EFFECTIVE_PIXELS
    {
        return Err(InvalidArguments);
    }

    let timezone = controls
        .timezone
        .unwrap_or_else(|| DEFAULT_RENDER_TIMEZONE.to_owned());
    if timezone.len() > MAX_RENDER_TIMEZONE_BYTES {
        return Err(InvalidArguments);
    }
    let timezone = Tz::from_str(&timezone).map_err(|_| InvalidArguments)?;
    let variables = controls.variables.unwrap_or_default();
    if variables.len() > MAX_VARIABLES
        || variables.iter().any(|(name, value)| {
            !valid_variable_name(name)
                || value.len() > MAX_VARIABLE_VALUE_BYTES
                || value.chars().any(char::is_control)
        })
    {
        return Err(InvalidArguments);
    }

    Ok(RenderOptions {
        from,
        to,
        width,
        height,
        scale,
        theme: controls.theme.unwrap_or(RenderTheme::Dark),
        timezone: timezone.name().to_owned(),
        variables,
    })
}

fn validate_range(
    from: Option<String>,
    to: Option<String>,
) -> Result<(String, String), InvalidArguments> {
    if from
        .as_ref()
        .is_some_and(|value| value.len() > MAX_RENDER_RANGE_BYTES)
        || to
            .as_ref()
            .is_some_and(|value| value.len() > MAX_RENDER_RANGE_BYTES)
    {
        return Err(InvalidArguments);
    }
    match (from, to) {
        (None, None) => Ok((DEFAULT_RENDER_FROM.to_owned(), DEFAULT_RENDER_TO.to_owned())),
        (Some(from), Some(to)) if to == "now" => {
            let Some(relative) = from.strip_prefix("now-") else {
                return Err(InvalidArguments);
            };
            let Some((amount, unit)) = relative.split_at_checked(relative.len().saturating_sub(1))
            else {
                return Err(InvalidArguments);
            };
            if amount.is_empty() || !amount.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(InvalidArguments);
            }
            let amount = amount.parse::<u64>().map_err(|_| InvalidArguments)?;
            let seconds_per_unit = match unit {
                "s" => 1,
                "m" => 60,
                "h" => 60 * 60,
                "d" => 24 * 60 * 60,
                _ => return Err(InvalidArguments),
            };
            amount
                .checked_mul(seconds_per_unit)
                .filter(|seconds| (1..=24 * 60 * 60).contains(seconds))
                .ok_or(InvalidArguments)?;
            Ok((format!("now-{amount}{unit}"), "now".to_owned()))
        }
        (Some(from), Some(to)) => {
            let from = DateTime::parse_from_rfc3339(&from).map_err(|_| InvalidArguments)?;
            let to = DateTime::parse_from_rfc3339(&to).map_err(|_| InvalidArguments)?;
            let duration = to.signed_duration_since(from);
            if duration < chrono::Duration::zero() || duration > chrono::Duration::hours(24) {
                return Err(InvalidArguments);
            }
            Ok((
                from.timestamp_millis().to_string(),
                to.timestamp_millis().to_string(),
            ))
        }
        _ => Err(InvalidArguments),
    }
}

pub(crate) fn valid_panel_id(panel_id: &str) -> bool {
    (1..=64).contains(&panel_id.len())
        && panel_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn valid_variable_name(name: &str) -> bool {
    let mut characters = name.bytes();
    (1..=MAX_VARIABLE_NAME_BYTES).contains(&name.len())
        && characters
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && characters.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dashboard_input() -> RenderDashboardInput {
        RenderDashboardInput {
            uid: "dashboard-1".to_owned(),
            from: None,
            to: None,
            width: None,
            height: None,
            scale: None,
            theme: None,
            timezone: None,
            variables: None,
        }
    }

    fn panel_input() -> RenderPanelInput {
        RenderPanelInput {
            uid: "dashboard-1".to_owned(),
            panel_id: "panel-13".to_owned(),
            from: None,
            to: None,
            width: None,
            height: None,
            scale: None,
            theme: None,
            timezone: None,
            variables: None,
        }
    }

    #[test]
    fn applies_exact_dashboard_and_panel_defaults() {
        let dashboard = dashboard_input().validate().unwrap();
        assert_eq!(dashboard.uid, "dashboard-1");
        assert_eq!(dashboard.options.from, DEFAULT_RENDER_FROM);
        assert_eq!(dashboard.options.to, DEFAULT_RENDER_TO);
        assert_eq!(dashboard.options.width, DEFAULT_DASHBOARD_WIDTH);
        assert_eq!(dashboard.options.height, DEFAULT_DASHBOARD_HEIGHT);
        assert_eq!(dashboard.options.scale, DEFAULT_RENDER_SCALE);
        assert_eq!(dashboard.options.theme, RenderTheme::Dark);
        assert_eq!(dashboard.options.theme.as_str(), "dark");
        assert_eq!(dashboard.options.timezone, DEFAULT_RENDER_TIMEZONE);
        assert!(dashboard.options.variables.is_empty());

        let panel = panel_input().validate().unwrap();
        assert_eq!(panel.uid, "dashboard-1");
        assert_eq!(panel.panel_id, "panel-13");
        assert_eq!(panel.options.width, DEFAULT_PANEL_WIDTH);
        assert_eq!(panel.options.height, DEFAULT_PANEL_HEIGHT);
    }

    #[test]
    fn validates_dimensions_scale_and_effective_pixel_budget() {
        let mut minimum = panel_input();
        minimum.width = Some(MIN_RENDER_WIDTH);
        minimum.height = Some(MIN_RENDER_HEIGHT);
        minimum.scale = Some(2);
        assert!(minimum.validate().is_ok());

        let mut maximum = panel_input();
        maximum.width = Some(MAX_RENDER_WIDTH);
        maximum.height = Some(500);
        maximum.scale = Some(2);
        assert!(maximum.validate().is_ok());

        for (width, height, scale) in [
            (MIN_RENDER_WIDTH - 1, MIN_RENDER_HEIGHT, 1),
            (MAX_RENDER_WIDTH + 1, MIN_RENDER_HEIGHT, 1),
            (MIN_RENDER_WIDTH, MIN_RENDER_HEIGHT - 1, 1),
            (MIN_RENDER_WIDTH, MAX_RENDER_HEIGHT + 1, 1),
            (MIN_RENDER_WIDTH, MIN_RENDER_HEIGHT, 0),
            (MIN_RENDER_WIDTH, MIN_RENDER_HEIGHT, 3),
            (MAX_RENDER_WIDTH, MAX_RENDER_HEIGHT, 2),
        ] {
            let mut invalid = panel_input();
            invalid.width = Some(width);
            invalid.height = Some(height);
            invalid.scale = Some(scale);
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn accepts_panel_string_grammar_and_rejects_unsafe_ids() {
        for panel_id in ["1", "panel-13", "Panel_1.2", &"x".repeat(64)] {
            let mut input = panel_input();
            input.panel_id = panel_id.to_owned();
            assert_eq!(input.validate().unwrap().panel_id, panel_id);
        }
        for panel_id in [
            "",
            "panel/13",
            "panel%2F13",
            "panel?13",
            "panel 13",
            "panel\n13",
            &"x".repeat(65),
            "é",
        ] {
            let mut input = panel_input();
            input.panel_id = panel_id.to_owned();
            assert!(input.validate().is_err());
        }
    }

    #[test]
    fn normalizes_relative_and_absolute_ranges() {
        let mut relative = dashboard_input();
        relative.from = Some("now-060m".to_owned());
        relative.to = Some("now".to_owned());
        let relative = relative.validate().unwrap();
        assert_eq!(relative.options.from, "now-60m");
        assert_eq!(relative.options.to, "now");

        let mut maximum_relative = dashboard_input();
        maximum_relative.from = Some("now-86400s".to_owned());
        maximum_relative.to = Some("now".to_owned());
        assert!(maximum_relative.validate().is_ok());

        let mut absolute = dashboard_input();
        absolute.from = Some("2026-08-09T10:00:00+02:00".to_owned());
        absolute.to = Some("2026-08-09T09:30:00Z".to_owned());
        let absolute = absolute.validate().unwrap();
        assert_eq!(absolute.options.from, "1786262400000");
        assert_eq!(absolute.options.to, "1786267800000");

        let mut maximum_absolute = dashboard_input();
        maximum_absolute.from = Some("2026-08-09T10:00:00Z".to_owned());
        maximum_absolute.to = Some("2026-08-10T10:00:00Z".to_owned());
        assert!(maximum_absolute.validate().is_ok());
    }

    #[test]
    fn rejects_invalid_range_combinations_and_date_math() {
        let pairs = [
            (Some("now-1h"), None),
            (None, Some("now")),
            (Some("now-0h"), Some("now")),
            (Some("now-25h"), Some("now")),
            (Some("now-1w"), Some("now")),
            (Some("now/h"), Some("now")),
            (Some("2026-08-09T10:00:00Z"), Some("now")),
            (Some("now-1h"), Some("2026-08-09T10:00:00Z")),
            (Some("2026-08-10T10:00:00Z"), Some("2026-08-09T10:00:00Z")),
            (
                Some("2026-08-09T10:00:00Z"),
                Some("2026-08-10T10:00:00.001Z"),
            ),
        ];
        for (from, to) in pairs {
            let mut invalid = dashboard_input();
            invalid.from = from.map(str::to_owned);
            invalid.to = to.map(str::to_owned);
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn byte_bounds_ranges_and_timezone_before_parsing() {
        let maximum_rfc3339 = format!("2026-08-09T10:00:00.{}Z", "0".repeat(43));
        assert_eq!(maximum_rfc3339.len(), MAX_RENDER_RANGE_BYTES);
        let mut maximum = dashboard_input();
        maximum.from = Some(maximum_rfc3339.clone());
        maximum.to = Some(maximum_rfc3339);
        assert!(maximum.validate().is_ok());

        for (from, to) in [
            (
                Some("x".repeat(MAX_RENDER_RANGE_BYTES + 1)),
                Some("now".to_owned()),
            ),
            (
                Some("now-1h".to_owned()),
                Some("x".repeat(MAX_RENDER_RANGE_BYTES + 1)),
            ),
        ] {
            let mut invalid = dashboard_input();
            invalid.from = from;
            invalid.to = to;
            assert!(invalid.validate().is_err());
        }

        let mut maximum_timezone = dashboard_input();
        maximum_timezone.timezone = Some("America/Argentina/ComodRivadavia".to_owned());
        assert_eq!(
            maximum_timezone.timezone.as_ref().unwrap().len(),
            MAX_RENDER_TIMEZONE_BYTES
        );
        assert!(maximum_timezone.validate().is_ok());

        let mut overlong_timezone = dashboard_input();
        overlong_timezone.timezone = Some("x".repeat(MAX_RENDER_TIMEZONE_BYTES + 1));
        assert!(overlong_timezone.validate().is_err());
    }

    #[test]
    fn generated_schema_exposes_range_and_timezone_byte_bounds() {
        for schema in [
            serde_json::to_value(schemars::schema_for!(RenderDashboardInput)).unwrap(),
            serde_json::to_value(schemars::schema_for!(RenderPanelInput)).unwrap(),
        ] {
            for field in ["from", "to"] {
                assert!(contains_max_length(
                    &schema["properties"][field],
                    MAX_RENDER_RANGE_BYTES as u64
                ));
            }
            assert!(contains_max_length(
                &schema["properties"]["timezone"],
                MAX_RENDER_TIMEZONE_BYTES as u64
            ));
        }
    }

    fn contains_max_length(value: &serde_json::Value, expected: u64) -> bool {
        value.get("maxLength").and_then(serde_json::Value::as_u64) == Some(expected)
            || value.as_array().is_some_and(|values| {
                values
                    .iter()
                    .any(|value| contains_max_length(value, expected))
            })
            || value.as_object().is_some_and(|object| {
                object
                    .values()
                    .any(|value| contains_max_length(value, expected))
            })
    }

    #[test]
    fn validates_timezone_theme_and_variables() {
        let mut input = dashboard_input();
        input.theme = Some(RenderTheme::Light);
        input.timezone = Some("America/New_York".to_owned());
        input.variables = Some(BTreeMap::from([
            ("z-variable".to_owned(), String::new()),
            ("_a.variable".to_owned(), "value".repeat(204)),
        ]));
        let request = input.validate().unwrap();
        assert_eq!(request.options.theme.as_str(), "light");
        assert_eq!(request.options.timezone, "America/New_York");
        assert_eq!(
            request.options.variables.keys().collect::<Vec<_>>(),
            ["_a.variable", "z-variable"]
        );

        let maximum_variables = (0..MAX_VARIABLES)
            .map(|index| {
                let name = format!("v{index:02}{}", "x".repeat(MAX_VARIABLE_NAME_BYTES - 3));
                (name, "x".repeat(MAX_VARIABLE_VALUE_BYTES))
            })
            .collect();
        let mut maximum_input = dashboard_input();
        maximum_input.variables = Some(maximum_variables);
        let maximum = maximum_input.validate().unwrap();
        assert_eq!(maximum.options.variables.len(), MAX_VARIABLES);
        assert!(maximum.options.variables.iter().all(|(name, value)| {
            name.len() == MAX_VARIABLE_NAME_BYTES && value.len() == MAX_VARIABLE_VALUE_BYTES
        }));

        let mut invalid_timezone = dashboard_input();
        invalid_timezone.timezone = Some("GMT+2".to_owned());
        assert!(invalid_timezone.validate().is_err());

        assert!(
            serde_json::from_value::<RenderDashboardInput>(serde_json::json!({
                "uid": "valid", "theme": "sepia"
            }))
            .is_err()
        );
    }

    #[test]
    fn rejects_variable_count_name_value_and_controls() {
        let invalid_variables = [
            BTreeMap::from([("1name".to_owned(), "value".to_owned())]),
            BTreeMap::from([(String::new(), "value".to_owned())]),
            BTreeMap::from([("x".repeat(MAX_VARIABLE_NAME_BYTES + 1), "value".to_owned())]),
            BTreeMap::from([("bad/name".to_owned(), "value".to_owned())]),
            BTreeMap::from([("name".to_owned(), "x".repeat(MAX_VARIABLE_VALUE_BYTES + 1))]),
            BTreeMap::from([("name".to_owned(), "bad\nvalue".to_owned())]),
            (0..=MAX_VARIABLES)
                .map(|index| (format!("name{index}"), "value".to_owned()))
                .collect(),
        ];
        for variables in invalid_variables {
            let mut invalid = panel_input();
            invalid.variables = Some(variables);
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn rejects_invalid_dashboard_uid_and_unknown_fields() {
        let mut invalid_uid = dashboard_input();
        invalid_uid.uid = "bad/uid".to_owned();
        assert!(invalid_uid.validate().is_err());

        assert!(
            serde_json::from_value::<RenderDashboardInput>(serde_json::json!({
                "uid": "valid", "orgId": 2
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RenderPanelInput>(serde_json::json!({
                "uid": "valid", "panel_id": "panel-13", "url": "https://example.com"
            }))
            .is_err()
        );
    }
}
