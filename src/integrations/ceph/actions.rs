use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_LIMIT: u16 = 100;
const MAX_DEVICE_ID_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationError;

impl std::fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid Ceph arguments")
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryInput {
    ClusterList {},
    StatusGet {
        cluster: String,
    },
    MetricsSummary {
        cluster: String,
    },
    OsdList {
        cluster: String,
        limit: Option<u16>,
    },
    OsdGet {
        cluster: String,
        osd_id: u32,
    },
    OsdSafeToDestroy {
        cluster: String,
        osd_id: u32,
    },
    DeviceList {
        cluster: String,
        osd_id: u32,
        limit: Option<u16>,
    },
    DeviceGet {
        cluster: String,
        osd_id: u32,
        device_id: String,
    },
    FlagsGet {
        cluster: String,
    },
    TaskList {
        cluster: String,
        limit: Option<u16>,
    },
}

macro_rules! cluster_input {
    ($name:ident, $variant:ident) => {
        #[derive(Debug, Clone, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub cluster: String,
        }
        impl $name {
            pub fn validate(self) -> Result<QueryCommand, ValidationError> {
                QueryInput::$variant {
                    cluster: self.cluster,
                }
                .validate()
            }
        }
    };
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClusterListInput {}
impl ClusterListInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        QueryInput::ClusterList {}.validate()
    }
}

cluster_input!(StatusGetInput, StatusGet);
cluster_input!(MetricsSummaryInput, MetricsSummary);
cluster_input!(FlagsGetInput, FlagsGet);

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OsdListInput {
    pub cluster: String,
    pub limit: Option<u16>,
}
impl OsdListInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        QueryInput::OsdList {
            cluster: self.cluster,
            limit: self.limit,
        }
        .validate()
    }
}

macro_rules! osd_input {
    ($name:ident, $variant:ident) => {
        #[derive(Debug, Clone, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub cluster: String,
            pub osd_id: u32,
        }
        impl $name {
            pub fn validate(self) -> Result<QueryCommand, ValidationError> {
                QueryInput::$variant {
                    cluster: self.cluster,
                    osd_id: self.osd_id,
                }
                .validate()
            }
        }
    };
}

osd_input!(OsdGetInput, OsdGet);
osd_input!(OsdSafeToDestroyInput, OsdSafeToDestroy);

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceListInput {
    pub cluster: String,
    pub osd_id: u32,
    pub limit: Option<u16>,
}
impl DeviceListInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        QueryInput::DeviceList {
            cluster: self.cluster,
            osd_id: self.osd_id,
            limit: self.limit,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceGetInput {
    pub cluster: String,
    pub osd_id: u32,
    pub device_id: String,
}
impl DeviceGetInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        QueryInput::DeviceGet {
            cluster: self.cluster,
            osd_id: self.osd_id,
            device_id: self.device_id,
        }
        .validate()
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskListInput {
    pub cluster: String,
    pub limit: Option<u16>,
}
impl TaskListInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        QueryInput::TaskList {
            cluster: self.cluster,
            limit: self.limit,
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryCommand {
    ClusterList,
    StatusGet {
        cluster: String,
    },
    MetricsSummary {
        cluster: String,
    },
    OsdList {
        cluster: String,
        limit: u16,
    },
    OsdGet {
        cluster: String,
        osd_id: u32,
    },
    OsdSafeToDestroy {
        cluster: String,
        osd_id: u32,
    },
    DeviceList {
        cluster: String,
        osd_id: u32,
        limit: u16,
    },
    DeviceGet {
        cluster: String,
        osd_id: u32,
        device_id: String,
    },
    FlagsGet {
        cluster: String,
    },
    TaskList {
        cluster: String,
        limit: u16,
    },
}

impl QueryInput {
    pub fn validate(self) -> Result<QueryCommand, ValidationError> {
        Ok(match self {
            Self::ClusterList {} => QueryCommand::ClusterList,
            Self::StatusGet { cluster } if valid_cluster(&cluster) => {
                QueryCommand::StatusGet { cluster }
            }
            Self::MetricsSummary { cluster } if valid_cluster(&cluster) => {
                QueryCommand::MetricsSummary { cluster }
            }
            Self::OsdList { cluster, limit } if valid_cluster(&cluster) => QueryCommand::OsdList {
                cluster,
                limit: bounded_limit(limit)?,
            },
            Self::OsdGet { cluster, osd_id } if valid_cluster(&cluster) => {
                QueryCommand::OsdGet { cluster, osd_id }
            }
            Self::OsdSafeToDestroy { cluster, osd_id } if valid_cluster(&cluster) => {
                QueryCommand::OsdSafeToDestroy { cluster, osd_id }
            }
            Self::DeviceList {
                cluster,
                osd_id,
                limit,
            } if valid_cluster(&cluster) => QueryCommand::DeviceList {
                cluster,
                osd_id,
                limit: bounded_limit(limit)?,
            },
            Self::DeviceGet {
                cluster,
                osd_id,
                device_id,
            } if valid_cluster(&cluster) && valid_device_id(&device_id) => {
                QueryCommand::DeviceGet {
                    cluster,
                    osd_id,
                    device_id,
                }
            }
            Self::FlagsGet { cluster } if valid_cluster(&cluster) => {
                QueryCommand::FlagsGet { cluster }
            }
            Self::TaskList { cluster, limit } if valid_cluster(&cluster) => {
                QueryCommand::TaskList {
                    cluster,
                    limit: bounded_limit(limit)?,
                }
            }
            _ => return Err(ValidationError),
        })
    }
}

impl QueryCommand {
    pub fn cluster(&self) -> Option<&str> {
        match self {
            Self::ClusterList => None,
            Self::StatusGet { cluster }
            | Self::MetricsSummary { cluster }
            | Self::OsdList { cluster, .. }
            | Self::OsdGet { cluster, .. }
            | Self::OsdSafeToDestroy { cluster, .. }
            | Self::DeviceList { cluster, .. }
            | Self::DeviceGet { cluster, .. }
            | Self::FlagsGet { cluster }
            | Self::TaskList { cluster, .. } => Some(cluster),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarkState {
    In,
    Out,
    Down,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrubKind {
    Normal,
    Deep,
}

pub(crate) fn is_curated_flag(value: &str) -> bool {
    matches!(
        value,
        "noout"
            | "noin"
            | "noup"
            | "nodown"
            | "norebalance"
            | "norecover"
            | "nobackfill"
            | "noscrub"
            | "nodeep-scrub"
    )
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecInput {
    #[serde(rename = "osd_mark")]
    Mark {
        cluster: String,
        osd_id: u32,
        state: MarkState,
    },
    #[serde(rename = "osd_reweight")]
    Reweight {
        cluster: String,
        osd_id: u32,
        weight: f64,
    },
    #[serde(rename = "osd_scrub")]
    Scrub {
        cluster: String,
        osd_id: u32,
        kind: ScrubKind,
    },
    #[serde(rename = "osd_destroy")]
    Destroy {
        cluster: String,
        osd_id: u32,
        confirmation: String,
    },
    #[serde(rename = "osd_purge")]
    Purge {
        cluster: String,
        osd_id: u32,
        confirmation: String,
    },
}

macro_rules! exec_input {
    ($name:ident, $variant:ident, { $($field:ident : $type:ty),+ $(,)? }) => {
        #[derive(Debug, Clone, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(pub $field: $type),+ }
        impl $name {
            pub fn validate(self) -> Result<ExecCommand, ValidationError> {
                ExecInput::$variant { $($field: self.$field),+ }.validate()
            }
        }
    };
}

exec_input!(OsdMarkInput, Mark, { cluster: String, osd_id: u32, state: MarkState });
exec_input!(OsdReweightInput, Reweight, { cluster: String, osd_id: u32, weight: f64 });
exec_input!(OsdScrubInput, Scrub, { cluster: String, osd_id: u32, kind: ScrubKind });
exec_input!(OsdDestroyInput, Destroy, { cluster: String, osd_id: u32, confirmation: String });
exec_input!(OsdPurgeInput, Purge, { cluster: String, osd_id: u32, confirmation: String });

#[derive(Debug, Clone, PartialEq)]
pub enum ExecCommand {
    Mark {
        cluster: String,
        osd_id: u32,
        state: MarkState,
    },
    Reweight {
        cluster: String,
        osd_id: u32,
        weight: f64,
    },
    Scrub {
        cluster: String,
        osd_id: u32,
        kind: ScrubKind,
    },
    Destroy {
        cluster: String,
        osd_id: u32,
    },
    Purge {
        cluster: String,
        osd_id: u32,
    },
}

impl ExecInput {
    pub fn validate(self) -> Result<ExecCommand, ValidationError> {
        Ok(match self {
            Self::Mark {
                cluster,
                osd_id,
                state,
            } if valid_cluster(&cluster) => ExecCommand::Mark {
                cluster,
                osd_id,
                state,
            },
            Self::Reweight {
                cluster,
                osd_id,
                weight,
            } if valid_cluster(&cluster) && weight.is_finite() && (0.0..=1.0).contains(&weight) => {
                ExecCommand::Reweight {
                    cluster,
                    osd_id,
                    weight,
                }
            }
            Self::Scrub {
                cluster,
                osd_id,
                kind,
            } if valid_cluster(&cluster) => ExecCommand::Scrub {
                cluster,
                osd_id,
                kind,
            },
            Self::Destroy {
                cluster,
                osd_id,
                confirmation,
            } if valid_cluster(&cluster)
                && confirmation == destructive_confirmation("destroy", &cluster, osd_id) =>
            {
                ExecCommand::Destroy { cluster, osd_id }
            }
            Self::Purge {
                cluster,
                osd_id,
                confirmation,
            } if valid_cluster(&cluster)
                && confirmation == destructive_confirmation("purge", &cluster, osd_id) =>
            {
                ExecCommand::Purge { cluster, osd_id }
            }
            _ => return Err(ValidationError),
        })
    }
}

impl ExecCommand {
    pub fn cluster(&self) -> &str {
        match self {
            Self::Mark { cluster, .. }
            | Self::Reweight { cluster, .. }
            | Self::Scrub { cluster, .. }
            | Self::Destroy { cluster, .. }
            | Self::Purge { cluster, .. } => cluster,
        }
    }
}

pub fn destructive_confirmation(action: &str, cluster: &str, osd_id: u32) -> String {
    format!("{action} osd.{osd_id} on {cluster}")
}

pub(crate) fn valid_cluster(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn valid_device_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_DEVICE_ID_BYTES
        && !value.chars().any(char::is_control)
}

fn bounded_limit(value: Option<u16>) -> Result<u16, ValidationError> {
    let value = value.unwrap_or(50);
    (value > 0 && value <= MAX_LIMIT)
        .then_some(value)
        .ok_or(ValidationError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_reject_unknown_and_unapproved_inputs() {
        for payload in [
            r#"{"action":"osd_mark","cluster":"romulus","osd_id":1,"state":"lost"}"#,
            r#"{"action":"osd_mark","cluster":"romulus","osd_id":1,"state":"up"}"#,
            r#"{"action":"osd_destroy","cluster":"romulus","osd_id":1,"confirmation":"destroy osd.1 on romulus","force":true}"#,
            r#"{"action":"flags_set","cluster":"romulus","flag":"pause","state":"set"}"#,
            r#"{"action":"osd_scrub","cluster":"romulus","osd_id":1,"kind":"normal","path":"/api/other"}"#,
        ] {
            assert!(
                serde_json::from_str::<ExecInput>(payload).is_err(),
                "accepted {payload}"
            );
        }
        assert!(
            serde_json::from_str::<QueryInput>(
                r#"{"action":"status_get","cluster":"romulus","counter":"secret"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn validation_enforces_cluster_syntax_bounds_and_confirmation() {
        assert!(
            OsdReweightInput {
                cluster: "romulus".into(),
                osd_id: 1,
                weight: 1.0
            }
            .validate()
            .is_ok()
        );
        for weight in [-0.1, 1.1, f64::NAN] {
            assert!(
                OsdReweightInput {
                    cluster: "romulus".into(),
                    osd_id: 1,
                    weight
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            OsdListInput {
                cluster: "other".into(),
                limit: None
            }
            .validate()
            .is_ok()
        );
        for cluster in [
            "",
            "-cluster",
            "cluster-",
            "UPPER",
            "bad.name",
            &"a".repeat(64),
        ] {
            assert!(
                StatusGetInput {
                    cluster: cluster.into()
                }
                .validate()
                .is_err(),
                "accepted {cluster:?}"
            );
        }
        assert!(
            TaskListInput {
                cluster: "pantheon".into(),
                limit: Some(101)
            }
            .validate()
            .is_err()
        );
        assert!(
            OsdDestroyInput {
                cluster: "romulus".into(),
                osd_id: 7,
                confirmation: "destroy osd.7 on pantheon".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            OsdDestroyInput {
                cluster: "romulus".into(),
                osd_id: 7,
                confirmation: destructive_confirmation("destroy", "romulus", 7)
            }
            .validate()
            .is_ok()
        );
    }
}
