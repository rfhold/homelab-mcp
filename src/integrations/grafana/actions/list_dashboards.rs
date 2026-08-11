use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::Deserialize;

use super::InvalidArguments;

pub const DEFAULT_DASHBOARD_PAGE: u32 = 1;
pub const MAX_DASHBOARD_PAGE: u32 = 10_000;
pub const DEFAULT_DASHBOARD_LIMIT: u16 = 50;
pub const MAX_DASHBOARD_LIMIT: u16 = 100;
pub const MAX_DASHBOARD_QUERY_BYTES: usize = 256;
pub const MAX_DASHBOARD_TAGS: usize = 20;
pub const MAX_DASHBOARD_TAG_BYTES: usize = 128;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListDashboardsInput {
    /// Optional dashboard title search, up to 256 UTF-8 bytes.
    pub query: Option<String>,
    /// Optional dashboard tags, up to 20 values of 128 UTF-8 bytes each.
    pub tags: Option<Vec<String>>,
    /// One-based result page, from 1 through 10000.
    pub page: Option<u32>,
    /// Maximum returned dashboards, from 1 through 100.
    pub limit: Option<u16>,
}

pub struct ListDashboardsQuery {
    pub(crate) query: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) page: u32,
    pub(crate) limit: u16,
}

impl ListDashboardsInput {
    pub fn validate(self) -> Result<ListDashboardsQuery, InvalidArguments> {
        if self.query.as_ref().is_some_and(|query| {
            query.trim().is_empty()
                || query.len() > MAX_DASHBOARD_QUERY_BYTES
                || query.chars().any(char::is_control)
        }) {
            return Err(InvalidArguments);
        }

        let tags = self.tags.unwrap_or_default();
        if tags.len() > MAX_DASHBOARD_TAGS
            || tags.iter().any(|tag| {
                tag.trim().is_empty()
                    || tag.len() > MAX_DASHBOARD_TAG_BYTES
                    || tag.chars().any(char::is_control)
            })
        {
            return Err(InvalidArguments);
        }
        let tags = tags
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let page = self.page.unwrap_or(DEFAULT_DASHBOARD_PAGE);
        let limit = self.limit.unwrap_or(DEFAULT_DASHBOARD_LIMIT);
        if !(1..=MAX_DASHBOARD_PAGE).contains(&page) || !(1..=MAX_DASHBOARD_LIMIT).contains(&limit)
        {
            return Err(InvalidArguments);
        }

        Ok(ListDashboardsQuery {
            query: self.query,
            tags,
            page,
            limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ListDashboardsInput {
        ListDashboardsInput {
            query: None,
            tags: None,
            page: None,
            limit: None,
        }
    }

    #[test]
    fn validates_defaults_bounds_and_deterministic_tags() {
        let default = input().validate().unwrap();
        assert_eq!(default.query, None);
        assert!(default.tags.is_empty());
        assert_eq!(default.page, DEFAULT_DASHBOARD_PAGE);
        assert_eq!(default.limit, DEFAULT_DASHBOARD_LIMIT);

        let bounded = ListDashboardsInput {
            query: Some("q".repeat(MAX_DASHBOARD_QUERY_BYTES)),
            tags: Some(vec!["z".to_owned(), "a".to_owned(), "z".to_owned()]),
            page: Some(MAX_DASHBOARD_PAGE),
            limit: Some(MAX_DASHBOARD_LIMIT),
        }
        .validate()
        .unwrap();
        assert_eq!(bounded.tags, ["a", "z"]);
        assert_eq!(bounded.page, MAX_DASHBOARD_PAGE);
        assert_eq!(bounded.limit, MAX_DASHBOARD_LIMIT);

        let maximum_tags = ListDashboardsInput {
            tags: Some(
                (0..MAX_DASHBOARD_TAGS)
                    .map(|index| format!("{index:02}{}", "x".repeat(MAX_DASHBOARD_TAG_BYTES - 2)))
                    .collect(),
            ),
            ..input()
        }
        .validate()
        .unwrap();
        assert_eq!(maximum_tags.tags.len(), MAX_DASHBOARD_TAGS);
        assert!(
            maximum_tags
                .tags
                .iter()
                .all(|tag| tag.len() == MAX_DASHBOARD_TAG_BYTES)
        );

        for invalid in [
            ListDashboardsInput {
                page: Some(0),
                ..input()
            },
            ListDashboardsInput {
                page: Some(MAX_DASHBOARD_PAGE + 1),
                ..input()
            },
            ListDashboardsInput {
                limit: Some(0),
                ..input()
            },
            ListDashboardsInput {
                limit: Some(MAX_DASHBOARD_LIMIT + 1),
                ..input()
            },
        ] {
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn rejects_invalid_query_and_tag_bounds() {
        for query in [
            String::new(),
            "x".repeat(MAX_DASHBOARD_QUERY_BYTES + 1),
            "bad\nquery".to_owned(),
        ] {
            assert!(
                ListDashboardsInput {
                    query: Some(query),
                    ..input()
                }
                .validate()
                .is_err()
            );
        }

        for tags in [
            vec![String::new()],
            vec!["x".repeat(MAX_DASHBOARD_TAG_BYTES + 1)],
            vec!["bad\ttag".to_owned()],
            vec!["tag".to_owned(); MAX_DASHBOARD_TAGS + 1],
        ] {
            assert!(
                ListDashboardsInput {
                    tags: Some(tags),
                    ..input()
                }
                .validate()
                .is_err()
            );
        }
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(
            serde_json::from_value::<ListDashboardsInput>(serde_json::json!({"folder": "x"}))
                .is_err()
        );
    }
}
