use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;

const MAX_IDENTIFIER_BYTES: usize = 512;
const MAX_REF_BYTES: usize = 512;
const MAX_PARAMS: usize = 20;
const MAX_PARAM_KEY_BYTES: usize = 128;
const MAX_PARAM_VALUE_BYTES: usize = 4096;

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryListInput {
    /// Maximum repositories to return, from 1 through 100.
    pub limit: Option<u16>,
}

#[derive(Clone)]
pub struct RepositoryListQuery {
    pub limit: u16,
}

impl RepositoryListInput {
    pub fn validate(self) -> Result<RepositoryListQuery, ()> {
        Ok(RepositoryListQuery {
            limit: bounded_limit(self.limit, 50, 100)?,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowListInput {
    /// Exact `org/repo` repository identity returned by `repository.list`.
    pub repository: Option<String>,
    /// Maximum workflows to return, from 1 through 200.
    pub limit: Option<u16>,
}

#[derive(Clone)]
pub struct WorkflowListQuery {
    pub repository: Option<String>,
    pub limit: u16,
}

impl WorkflowListInput {
    pub fn validate(self) -> Result<WorkflowListQuery, ()> {
        if self
            .repository
            .as_ref()
            .is_some_and(|value| !valid_repository_key(value))
        {
            return Err(());
        }
        Ok(WorkflowListQuery {
            repository: self.repository,
            limit: bounded_limit(self.limit, 100, 200)?,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunListInput {
    /// Exact `org/repo` repository identity returned by `repository.list`.
    pub repository: String,
    /// Optional workflow definition name.
    pub workflow: Option<String>,
    /// Optional exact branch or ref recorded by PAC.
    pub branch: Option<String>,
    /// Optional exact normalized PAC revision SHA.
    pub revision: Option<String>,
    /// Optional normalized status: running, succeeded, failed, or cancelled.
    pub status: Option<String>,
    /// Maximum runs to return, from 1 through 100.
    pub limit: Option<u16>,
}

#[derive(Clone)]
pub struct RunListQuery {
    pub repository: String,
    pub workflow: Option<String>,
    pub branch: Option<String>,
    pub revision: Option<String>,
    pub status: Option<String>,
    pub limit: u16,
}

impl RunListInput {
    pub fn validate(self) -> Result<RunListQuery, ()> {
        if !valid_repository_key(&self.repository)
            || self
                .workflow
                .as_ref()
                .is_some_and(|value| !valid_text(value, 253))
            || self
                .branch
                .as_ref()
                .is_some_and(|value| !valid_text(value, MAX_REF_BYTES))
            || self
                .revision
                .as_ref()
                .is_some_and(|value| !valid_text(value, MAX_REF_BYTES))
            || self.status.as_ref().is_some_and(|value| {
                !matches!(
                    value.as_str(),
                    "running" | "succeeded" | "failed" | "cancelled"
                )
            })
        {
            return Err(());
        }
        Ok(RunListQuery {
            repository: self.repository,
            workflow: self.workflow,
            branch: self.branch,
            revision: self.revision,
            status: self.status,
            limit: bounded_limit(self.limit, 50, 100)?,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunWaitInput {
    /// Exact `<namespace>/<name>` PipelineRun identity.
    pub run_id: String,
    /// Maximum wait in seconds, from 1 through 300. Defaults to 60.
    pub timeout_seconds: Option<u16>,
}

#[derive(Clone)]
pub struct RunWaitQuery {
    pub run_id: String,
    pub timeout_seconds: u16,
}

impl RunWaitInput {
    pub fn validate(self) -> Result<RunWaitQuery, ()> {
        if !valid_namespaced_id(&self.run_id) {
            return Err(());
        }
        Ok(RunWaitQuery {
            run_id: self.run_id,
            timeout_seconds: bounded_limit(self.timeout_seconds, 60, 300)?,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunGetInput {
    /// Exact `<namespace>/<name>` PipelineRun identity.
    pub run_id: String,
}

#[derive(Clone)]
pub struct RunGetQuery {
    pub run_id: String,
}

impl RunGetInput {
    pub fn validate(self) -> Result<RunGetQuery, ()> {
        if !valid_namespaced_id(&self.run_id) {
            return Err(());
        }
        Ok(RunGetQuery {
            run_id: self.run_id,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskListInput {
    /// Exact `<namespace>/<name>` PipelineRun identity.
    pub run_id: String,
    /// Maximum tasks to return, from 1 through 100.
    pub limit: Option<u16>,
}

#[derive(Clone)]
pub struct TaskListQuery {
    pub run_id: String,
    pub limit: u16,
}

impl TaskListInput {
    pub fn validate(self) -> Result<TaskListQuery, ()> {
        if !valid_namespaced_id(&self.run_id) {
            return Err(());
        }
        Ok(TaskListQuery {
            run_id: self.run_id,
            limit: bounded_limit(self.limit, 50, 100)?,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskLogsInput {
    /// Exact `<namespace>/<name>` PipelineRun identity.
    pub run_id: String,
    /// Exact `<namespace>/<name>` TaskRun identity.
    pub task_id: String,
    /// Optional step name returned by `task.list`.
    pub step: Option<String>,
    /// Per-step tail, from 1 through 1,000 lines.
    pub tail_lines: Option<u16>,
    /// Total returned log bytes, from 1 through 262,144.
    pub max_bytes: Option<u32>,
}

#[derive(Clone)]
pub struct TaskLogsQuery {
    pub run_id: String,
    pub task_id: String,
    pub step: Option<String>,
    pub tail_lines: u16,
    pub max_bytes: u32,
}

impl TaskLogsInput {
    pub fn validate(self) -> Result<TaskLogsQuery, ()> {
        if !valid_namespaced_id(&self.run_id)
            || !valid_namespaced_id(&self.task_id)
            || self
                .step
                .as_ref()
                .is_some_and(|value| !valid_text(value, 128))
        {
            return Err(());
        }
        let tail_lines = self.tail_lines.unwrap_or(200);
        let max_bytes = self.max_bytes.unwrap_or(65_536);
        if !(1..=1_000).contains(&tail_lines) || !(1..=262_144).contains(&max_bytes) {
            return Err(());
        }
        Ok(TaskLogsQuery {
            run_id: self.run_id,
            task_id: self.task_id,
            step: self.step,
            tail_lines,
            max_bytes,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDispatchInput {
    /// Exact `org/repo` repository identity returned by `repository.list`.
    pub repository: String,
    /// Opaque workflow identity returned by `workflow.list`.
    pub workflow: String,
    /// Git branch, tag, or revision supplied to PAC.
    #[serde(rename = "ref")]
    pub reference: String,
    /// String parameters passed to the PipelineRun template.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct WorkflowDispatchCommand {
    pub repository: String,
    pub workflow: String,
    pub reference: String,
    pub params: BTreeMap<String, String>,
}

impl WorkflowDispatchInput {
    pub fn validate(self) -> Result<WorkflowDispatchCommand, ()> {
        if !valid_repository_key(&self.repository)
            || !valid_text(&self.workflow, MAX_IDENTIFIER_BYTES)
            || !valid_text(&self.reference, MAX_REF_BYTES)
            || !valid_params(&self.params)
        {
            return Err(());
        }
        Ok(WorkflowDispatchCommand {
            repository: self.repository,
            workflow: self.workflow,
            reference: self.reference,
            params: self.params,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRerunInput {
    /// Exact `<namespace>/<name>` PipelineRun identity.
    pub run_id: String,
}

#[derive(Clone)]
pub struct RunRerunCommand {
    pub run_id: String,
}

impl RunRerunInput {
    pub fn validate(self) -> Result<RunRerunCommand, ()> {
        if !valid_namespaced_id(&self.run_id) {
            return Err(());
        }
        Ok(RunRerunCommand {
            run_id: self.run_id,
        })
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunCancelInput {
    /// Exact `<namespace>/<name>` active PipelineRun identity.
    pub run_id: String,
}

#[derive(Clone)]
pub struct RunCancelCommand {
    pub run_id: String,
}

impl RunCancelInput {
    pub fn validate(self) -> Result<RunCancelCommand, ()> {
        if !valid_namespaced_id(&self.run_id) {
            return Err(());
        }
        Ok(RunCancelCommand {
            run_id: self.run_id,
        })
    }
}

fn bounded_limit(value: Option<u16>, default: u16, maximum: u16) -> Result<u16, ()> {
    let value = value.unwrap_or(default);
    (value > 0 && value <= maximum).then_some(value).ok_or(())
}

fn valid_repository_key(value: &str) -> bool {
    let mut parts = value.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(owner), Some(repository), None)
            if valid_repository_component(owner) && valid_repository_component(repository)
    )
}

pub(super) fn valid_repository_component(value: &str) -> bool {
    valid_text(value, 253)
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_' | '~')
        })
}

fn valid_namespaced_id(value: &str) -> bool {
    let mut parts = value.split('/');
    matches!((parts.next(), parts.next(), parts.next()), (Some(namespace), Some(name), None) if valid_dns(namespace) && valid_dns(name))
}

fn valid_dns(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '.')
        })
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn valid_params(params: &BTreeMap<String, String>) -> bool {
    params.len() <= MAX_PARAMS
        && params.iter().all(|(key, value)| {
            valid_text(key, MAX_PARAM_KEY_BYTES)
                && value.len() <= MAX_PARAM_VALUE_BYTES
                && !key.chars().any(char::is_control)
                && !value.chars().any(char::is_control)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_limits_are_strict() {
        assert!(RepositoryListInput { limit: Some(100) }.validate().is_ok());
        assert!(RepositoryListInput { limit: Some(101) }.validate().is_err());
        assert!(
            TaskLogsInput {
                run_id: "pipelines-as-code/run".to_owned(),
                task_id: "pipelines-as-code/task".to_owned(),
                step: None,
                tail_lines: Some(1_000),
                max_bytes: Some(262_144),
            }
            .validate()
            .is_ok()
        );
        assert!(
            RunWaitInput {
                run_id: "pipelines-as-code/run".to_owned(),
                timeout_seconds: Some(300),
            }
            .validate()
            .is_ok()
        );
        assert!(
            RunWaitInput {
                run_id: "pipelines-as-code/run".to_owned(),
                timeout_seconds: Some(301),
            }
            .validate()
            .is_err()
        );
        assert!(
            RunGetInput {
                run_id: "ambiguous".to_owned()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn repository_and_run_filters_are_strict() {
        for repository in ["org", "org/repo/extra", "/repo", "org/", "org/re%2Fpo"] {
            assert!(
                WorkflowListInput {
                    repository: Some(repository.to_owned()),
                    limit: None,
                }
                .validate()
                .is_err(),
                "accepted {repository}"
            );
        }
        for repository in ["my_org/my_repo", "My~Org/Repo_Name"] {
            assert!(
                WorkflowListInput {
                    repository: Some(repository.to_owned()),
                    limit: None,
                }
                .validate()
                .is_ok(),
                "rejected {repository}"
            );
        }
        let query = RunListInput {
            repository: "rfhold/repo".to_owned(),
            workflow: None,
            branch: Some("refs/heads/main".to_owned()),
            revision: Some("abc123".to_owned()),
            status: None,
            limit: Some(1),
        }
        .validate()
        .unwrap();
        assert_eq!(query.revision.as_deref(), Some("abc123"));
        assert_eq!(query.limit, 1);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(
            serde_json::from_value::<RunListInput>(serde_json::json!({
                "repository":"rfhold/repo", "unknown":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RunWaitInput>(serde_json::json!({
                "run_id":"pipelines-as-code/run", "unknown":true
            }))
            .is_err()
        );
    }
}
