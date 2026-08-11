use schemars::JsonSchema;
use serde::Deserialize;

use super::{InvalidArguments, valid_dashboard_uid};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDashboardInput {
    /// Grafana dashboard UID.
    pub uid: String,
}

pub struct GetDashboardQuery {
    pub(crate) uid: String,
}

impl GetDashboardInput {
    pub fn validate(self) -> Result<GetDashboardQuery, InvalidArguments> {
        if !valid_dashboard_uid(&self.uid) {
            return Err(InvalidArguments);
        }
        Ok(GetDashboardQuery { uid: self.uid })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_uid_grammar_and_bounds() {
        for uid in ["a", "_", "-", "a.b-C_1", &"x".repeat(40)] {
            let query = GetDashboardInput {
                uid: uid.to_owned(),
            }
            .validate()
            .unwrap();
            assert_eq!(query.uid, uid);
        }
    }

    #[test]
    fn rejects_unsafe_uids_without_trimming() {
        for uid in [
            "",
            ".hidden",
            " dashboard",
            "dashboard ",
            "dash/board",
            "dash%2Fboard",
            "dash?x=1",
            "dash#panel",
            "dash\nboard",
            &"x".repeat(41),
            "é",
        ] {
            assert!(
                GetDashboardInput {
                    uid: uid.to_owned()
                }
                .validate()
                .is_err()
            );
        }
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(
            serde_json::from_value::<GetDashboardInput>(serde_json::json!({
                "uid": "valid", "slug": "controlled"
            }))
            .is_err()
        );
    }
}
