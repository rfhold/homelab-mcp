#![allow(clippy::useless_vec)]

use std::{future::Future, sync::Arc, time::Duration};

use axum::Router;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use mcp::{
    McpProtectedResourceMetadata, McpToolResult, OAuthAuthorizationServer, SkillCatalog,
    server::{
        ServerContext, ServerError, ServerResult, StreamableHttpAuthorization,
        StreamableHttpOptions,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

mod resources;
mod skills;
mod tools;

fn streamable_http_router_with_options(
    handler: Arc<HomelabMcp>,
    options: StreamableHttpOptions,
) -> mcp::server::StreamableHttpRouter {
    mcp::server::streamable_http_router_with_options(
        Arc::new(resources::ResourceHandler(handler)),
        options,
    )
}

#[cfg(test)]
fn streamable_http_router(handler: Arc<HomelabMcp>) -> mcp::server::StreamableHttpRouter {
    streamable_http_router_with_options(handler, StreamableHttpOptions::default())
}

use crate::{
    config::OAuthConfig,
    integrations::ceph::{
        Error as CephError,
        actions::{
            ClusterListInput as CephClusterListInput, DeviceGetInput as CephDeviceGetInput,
            DeviceListInput as CephDeviceListInput, ExecCommand as CephExecCommand,
            FlagsGetInput as CephFlagsGetInput, MetricsSummaryInput as CephMetricsSummaryInput,
            OsdDestroyInput as CephOsdDestroyInput, OsdGetInput as CephOsdGetInput,
            OsdListInput as CephOsdListInput, OsdMarkInput as CephOsdMarkInput,
            OsdPurgeInput as CephOsdPurgeInput, OsdReweightInput as CephOsdReweightInput,
            OsdSafeToDestroyInput as CephOsdSafeToDestroyInput, OsdScrubInput as CephOsdScrubInput,
            QueryCommand as CephQueryCommand, StatusGetInput as CephStatusGetInput,
            TaskListInput as CephTaskListInput, ValidationError as CephValidationError,
        },
    },
    integrations::deploys::DeployError,
    integrations::grafana::{
        Error as GrafanaError, RenderedImage,
        actions::{
            AlertInstancesInput, AlertRulesInput, CreateSilenceCommand, CreateSilenceInput,
            GetDashboardInput, ListDashboardsInput, ListSilencesInput, LogqlInput, ProfilesInput,
            PromqlInput, RecordingRulesInput, RenderDashboardInput, RenderPanelInput, TraceqlInput,
        },
    },
    integrations::kubernetes::{
        Error as KubernetesError,
        actions::{
            CapabilityListInput, ClusterListInput, CronjobSuspendInput, CronjobTriggerInput,
            ExecCommand as KubernetesExecCommand, PodDeleteInput, PodLogsInput,
            QueryCommand as KubernetesQueryCommand, ResourceGetInput, ResourceListInput,
            ValidationError as KubernetesValidationError, WorkloadRestartInput, WorkloadScaleInput,
        },
    },
    integrations::tekton::{
        Error as TektonError,
        actions::{
            RepositoryListInput, RunCancelCommand, RunCancelInput, RunGetInput, RunListInput,
            RunRerunCommand, RunRerunInput, RunStatusInput, RunWaitInput, TaskListInput,
            TaskLogsInput, WorkflowDispatchCommand, WorkflowDispatchInput, WorkflowListInput,
        },
    },
    inventory::{CreateMachine, Machine, RepositoryError, UpdateMachine},
    services::Services,
    tool_error::ToolError,
};

// Progressive schemas reject unsupported actions before handler dispatch.
const _: CephError = CephError::UnsupportedAction;

const PROGRESS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const PROGRESS_HEARTBEAT_MESSAGE: &str = "Request is still running";

#[cfg(test)]
const QUERY_TOOL_NAME: &str = "query";
#[cfg(test)]
const CREATE_TOOL_NAME: &str = "create";
#[cfg(test)]
const EXECUTE_TOOL_NAME: &str = "execute";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MachineListInput {
    #[serde(default = "default_machine_limit")]
    #[schemars(range(min = 1, max = 100))]
    limit: u16,
}

fn default_machine_limit() -> u16 {
    100
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MachineCreateInput {
    #[schemars(length(min = 1, max = 100))]
    display_name: String,
    #[schemars(length(min = 1, max = 253))]
    ssh_host: String,
    #[serde(default = "default_ssh_port")]
    #[schemars(range(min = 22, max = 22))]
    ssh_port: u16,
    #[serde(default = "default_ssh_username")]
    #[schemars(length(min = 7, max = 7), regex(pattern = "^homelab$"))]
    ssh_username: String,
    #[schemars(length(min = 1, max = 256))]
    pinned_host_public_key: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}
fn default_ssh_username() -> String {
    "homelab".to_owned()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MachineUpdateInput {
    #[schemars(length(min = 36, max = 36))]
    machine_id: String,
    #[schemars(length(min = 1, max = 100))]
    display_name: String,
    #[schemars(length(min = 1, max = 253))]
    ssh_host: String,
    #[schemars(range(min = 22, max = 22))]
    ssh_port: u16,
    #[schemars(length(min = 7, max = 7), regex(pattern = "^homelab$"))]
    ssh_username: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MachineIdInput {
    #[schemars(length(min = 36, max = 36))]
    machine_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MachineHostKeyInput {
    #[schemars(length(min = 36, max = 36))]
    machine_id: String,
    #[schemars(length(min = 1, max = 256))]
    host_public_key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeployListInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeployRunInput {
    #[schemars(length(min = 1, max = 64))]
    deploy_id: String,
    #[schemars(length(min = 36, max = 36))]
    machine_id: String,
}

#[derive(Clone)]
pub struct HomelabMcp {
    services: Arc<Services>,
    catalog: Arc<SkillCatalog>,
    progress_heartbeat_interval: Duration,
}

struct ProgressHeartbeat(Option<tokio::task::JoinHandle<()>>);

impl ProgressHeartbeat {
    fn start(context: &ServerContext, interval: Duration) -> Self {
        let Some(token) = context.progress_token() else {
            return Self(None);
        };
        let context = context.clone();
        let task = tokio::spawn(async move {
            let mut progress = 1.0;
            loop {
                tokio::time::sleep(interval).await;
                if context
                    .progress(
                        token.clone(),
                        progress,
                        None,
                        Some(PROGRESS_HEARTBEAT_MESSAGE.to_owned()),
                    )
                    .await
                    .is_err()
                {
                    break;
                }
                progress += 1.0;
            }
        });
        Self(Some(task))
    }
}

impl Drop for ProgressHeartbeat {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

pub fn router(
    config: &OAuthConfig,
    services: Arc<Services>,
    oauth: &OAuthAuthorizationServer,
) -> Result<Router, String> {
    let handler = Arc::new(HomelabMcp {
        services,
        catalog: Arc::new(skills::catalog().map_err(|error| error.to_string())?),
        progress_heartbeat_interval: PROGRESS_HEARTBEAT_INTERVAL,
    });
    let required_scopes = config.required_scopes.clone();
    let metadata =
        McpProtectedResourceMetadata::new(config.resource.clone(), [config.issuer.clone()])
            .with_scopes(required_scopes.clone())
            .with_resource_name("Homelab MCP");
    let hosted = oauth.clone();
    let authorization = StreamableHttpAuthorization::hosted(metadata, move |token, context| {
        hosted.authorize_token(token, context)
    })
    .map_err(|_| "invalid MCP authorization configuration".to_owned())?
    .with_required_scopes(required_scopes);
    let options = StreamableHttpOptions::default()
        .without_root_protected_resource_metadata()
        .with_authorization(authorization);
    Ok(streamable_http_router_with_options(handler, options))
}

impl HomelabMcp {
    fn progress_heartbeat(&self, context: &ServerContext) -> ProgressHeartbeat {
        ProgressHeartbeat::start(context, self.progress_heartbeat_interval)
    }

    /// Execute a LogQL instant or range query through Grafana.
    ///
    /// Use instant mode without start/end, or range mode with both endpoints.
    /// Log streams are normalized and limited to the requested number of lines.
    async fn logql(
        &self,
        input: LogqlInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("LogQL", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.execute(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(json_result(output)),
            Err(error) => Ok(tool_error("LogQL", error)),
        }
    }

    /// Execute a PromQL instant or range query through Grafana.
    ///
    /// Range mode requires start, end, and a positive Prometheus duration step.
    /// Ranges are limited to 24 hours and 11,000 points.
    async fn promql(
        &self,
        input: PromqlInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("PromQL", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.execute_promql(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(json_result(output)),
            Err(error) => Ok(tool_error("PromQL", error)),
        }
    }

    /// Search traces with TraceQL through Grafana.
    ///
    /// Optional start and end timestamps must appear together. Searches are
    /// limited to 24 hours and at most 100 returned traces.
    async fn traceql(
        &self,
        input: TraceqlInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("TraceQL", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.execute_traceql(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(json_result(output)),
            Err(error) => Ok(tool_error("TraceQL", error)),
        }
    }

    /// Merge Pyroscope stacktraces through Grafana.
    ///
    /// Start and end are required RFC3339 timestamps. Profile ranges are
    /// limited to one hour and at most 1,000 flame graph nodes.
    async fn profiles(
        &self,
        input: ProfilesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("profile", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.execute_profiles(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(json_result(output)),
            Err(error) => Ok(tool_error("profile", error)),
        }
    }

    /// List bounded Grafana alert-rule summaries.
    async fn alert_rules(
        &self,
        input: AlertRulesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("alert rule", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.alert_rules(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(json_result(output)),
            Err(error) => Ok(tool_error("alert rule", error)),
        }
    }

    /// List bounded Grafana recording-rule summaries.
    async fn recording_rules(
        &self,
        input: RecordingRulesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("recording rule", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.recording_rules(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(json_result(output)),
            Err(error) => Ok(tool_error("recording rule", error)),
        }
    }

    /// List bounded current Grafana alert instances, optionally filtered by labels.
    async fn alert_instances(
        &self,
        input: AlertInstancesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => {
                return Ok(tool_error("alert instance", GrafanaError::InvalidArguments));
            }
        };
        let result = tokio::select! {
            result = self.services.grafana.alert_instances(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(json_result(output)),
            Err(error) => Ok(tool_error("alert instance", error)),
        }
    }

    /// List bounded Grafana silences, optionally filtered by state.
    async fn list_silences(
        &self,
        input: ListSilencesInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("silence", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.list_silences(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        match result {
            Ok(output) => Ok(json_result(output)),
            Err(error) => Ok(tool_error("silence", error)),
        }
    }

    /// List bounded Grafana dashboard inventory.
    async fn list_dashboards(
        &self,
        input: ListDashboardsInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("dashboard", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.list_dashboards(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tool_error("dashboard", error),
        })
    }

    /// Get bounded inventory for one Grafana dashboard.
    async fn get_dashboard(
        &self,
        input: GetDashboardInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tool_error("dashboard", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.get_dashboard(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tool_error("dashboard", error),
        })
    }

    /// Render one bounded dashboard PNG.
    async fn render_dashboard(
        &self,
        input: RenderDashboardInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let request = match input.validate() {
            Ok(request) => request,
            Err(_) => return Ok(tool_error("render", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.render_dashboard(&request) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(image) => render_result("dashboard", &request.uid, None, &request.options, image),
            Err(error) => tool_error("render", error),
        })
    }

    /// Render one bounded panel PNG.
    async fn render_panel(
        &self,
        input: RenderPanelInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let request = match input.validate() {
            Ok(request) => request,
            Err(_) => return Ok(tool_error("render", GrafanaError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.grafana.render_panel(&request) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(image) => render_result(
                "panel",
                &request.uid,
                Some(&request.panel_id),
                &request.options,
                image,
            ),
            Err(error) => tool_error("render", error),
        })
    }

    /// Create a bounded Grafana silence that suppresses matching alert notifications.
    async fn create_silence(
        &self,
        input: CreateSilenceInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let command = match input.validate() {
            Ok(command) => command,
            Err(_) => return Ok(tool_error("silence", GrafanaError::InvalidArguments)),
        };
        Ok(self
            .dispatch_create_silence(&command, context.cancelled())
            .await)
    }

    async fn dispatch_create_silence(
        &self,
        command: &CreateSilenceCommand,
        cancellation: impl Future<Output = ()>,
    ) -> McpToolResult {
        let result = tokio::select! {
            result = self.services.grafana.create_silence(command) => result,
            () = cancellation => return tool_error("silence", GrafanaError::MutationOutcomeUnknown),
        };
        match result {
            Ok(output) => silence_result(output),
            Err(error) => tool_error("silence", error),
        }
    }

    /// List PAC Repository resources with valid configured Forgejo URLs.
    async fn tekton_repositories(
        &self,
        input: RepositoryListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => {
                return Ok(tekton_tool_error(
                    "repository",
                    TektonError::InvalidArguments,
                ));
            }
        };
        let result = tokio::select! {
            result = self.services.tekton.repositories(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("repository", error),
        })
    }

    /// List bounded Pipeline-as-Code definitions from configured repositories.
    async fn tekton_workflows(
        &self,
        input: WorkflowListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tekton_tool_error("workflow", TektonError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.tekton.workflows(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("workflow", error),
        })
    }

    /// List bounded PipelineRuns for an exact configured repository.
    async fn tekton_runs(
        &self,
        input: RunListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tekton_tool_error("run", TektonError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.tekton.runs(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("run", error),
        })
    }

    /// Get one owned PipelineRun by exact namespace-qualified identity.
    async fn tekton_run(
        &self,
        input: RunGetInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tekton_tool_error("run", TektonError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.tekton.run(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("run", error),
        })
    }

    /// Diagnose one owned PipelineRun and its failed owned TaskRuns.
    async fn tekton_run_status(
        &self,
        input: RunStatusInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tekton_tool_error("run", TektonError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.tekton.status(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("run", error),
        })
    }

    /// Wait up to a bounded deadline for one owned PipelineRun to become terminal.
    async fn tekton_run_wait(
        &self,
        input: RunWaitInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tekton_tool_error("run", TektonError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.tekton.wait(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("run", error),
        })
    }

    /// List owned TaskRuns for an exact PipelineRun.
    async fn tekton_tasks(
        &self,
        input: TaskListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tekton_tool_error("task", TektonError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.tekton.tasks(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("task", error),
        })
    }

    /// Read bounded, redacted logs for an owned TaskRun and optional step.
    async fn tekton_task_logs(
        &self,
        input: TaskLogsInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let query = match input.validate() {
            Ok(query) => query,
            Err(_) => return Ok(tekton_tool_error("task log", TektonError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.tekton.logs(&query) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("task log", error),
        })
    }

    /// Dispatch one exact incoming-enabled Pipeline-as-Code workflow.
    async fn tekton_dispatch(
        &self,
        input: WorkflowDispatchInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let command = match input.validate() {
            Ok(command) => command,
            Err(_) => return Ok(tekton_tool_error("workflow", TektonError::InvalidArguments)),
        };
        Ok(self
            .dispatch_tekton_workflow(&command, context.cancelled())
            .await)
    }

    /// Rerun one owned PipelineRun through the fixed PAC incoming route.
    async fn tekton_rerun(
        &self,
        input: RunRerunInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let command = match input.validate() {
            Ok(command) => command,
            Err(_) => return Ok(tekton_tool_error("run", TektonError::InvalidArguments)),
        };
        Ok(self
            .dispatch_tekton_rerun(&command, context.cancelled())
            .await)
    }

    /// Request cancellation of one active owned PipelineRun.
    async fn tekton_cancel(
        &self,
        input: RunCancelInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let command = match input.validate() {
            Ok(command) => command,
            Err(_) => return Ok(tekton_tool_error("run", TektonError::InvalidArguments)),
        };
        Ok(self
            .dispatch_tekton_cancel(&command, context.cancelled())
            .await)
    }

    async fn dispatch_tekton_workflow(
        &self,
        command: &WorkflowDispatchCommand,
        cancellation: impl Future<Output = ()>,
    ) -> McpToolResult {
        let result = tokio::select! {
            result = self.services.tekton.dispatch(command) => result,
            () = cancellation => return tekton_tool_error("workflow", TektonError::MutationOutcomeUnknown),
        };
        match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("workflow", error),
        }
    }

    async fn dispatch_tekton_rerun(
        &self,
        command: &RunRerunCommand,
        cancellation: impl Future<Output = ()>,
    ) -> McpToolResult {
        let result = tokio::select! {
            result = self.services.tekton.rerun(command) => result,
            () = cancellation => return tekton_tool_error("run", TektonError::MutationOutcomeUnknown),
        };
        match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("run", error),
        }
    }

    async fn dispatch_tekton_cancel(
        &self,
        command: &RunCancelCommand,
        cancellation: impl Future<Output = ()>,
    ) -> McpToolResult {
        let result = tokio::select! {
            result = self.services.tekton.cancel(command) => result,
            () = cancellation => return tekton_tool_error("run", TektonError::MutationOutcomeUnknown),
        };
        match result {
            Ok(output) => json_result(output),
            Err(error) => tekton_tool_error("run", error),
        }
    }

    /// List the configured Kubernetes cluster catalog without contacting a cluster.
    async fn kubernetes_clusters(
        &self,
        input: ClusterListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_kubernetes_query(input.validate(), "cluster", context.cancelled())
            .await
    }

    /// Report support for approved resource kinds on one configured cluster.
    async fn kubernetes_capabilities(
        &self,
        input: CapabilityListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_kubernetes_query(input.validate(), "capability", context.cancelled())
            .await
    }

    /// List a bounded set of normalized resources of one approved kind.
    async fn kubernetes_resources(
        &self,
        input: ResourceListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_kubernetes_query(input.validate(), "resource", context.cancelled())
            .await
    }

    /// Get one exact normalized resource of an approved kind.
    async fn kubernetes_resource(
        &self,
        input: ResourceGetInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_kubernetes_query(input.validate(), "resource", context.cancelled())
            .await
    }

    /// Read bounded current or previous logs for one exact Pod container and Pod UID.
    async fn kubernetes_pod_logs(
        &self,
        input: PodLogsInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_kubernetes_query(input.validate(), "Pod log", context.cancelled())
            .await
    }

    /// Restart one exact Deployment, StatefulSet, or DaemonSet.
    async fn kubernetes_workload_restart(
        &self,
        input: WorkloadRestartInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_kubernetes_exec(input.validate(), "workload", context.cancelled())
            .await)
    }

    /// Scale one exact Deployment or StatefulSet to a bounded replica count.
    async fn kubernetes_workload_scale(
        &self,
        input: WorkloadScaleInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_kubernetes_exec(input.validate(), "workload", context.cancelled())
            .await)
    }

    /// Suspend or resume one exact CronJob.
    async fn kubernetes_cronjob_suspend(
        &self,
        input: CronjobSuspendInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_kubernetes_exec(input.validate(), "CronJob", context.cancelled())
            .await)
    }

    /// Create one Job from one exact CronJob using a server-generated name.
    async fn kubernetes_cronjob_trigger(
        &self,
        input: CronjobTriggerInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_kubernetes_exec(input.validate(), "CronJob", context.cancelled())
            .await)
    }

    /// Request ordinary deletion of one exact Pod.
    async fn kubernetes_pod_delete(
        &self,
        input: PodDeleteInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_kubernetes_exec(input.validate(), "Pod", context.cancelled())
            .await)
    }

    async fn dispatch_kubernetes_query(
        &self,
        command: Result<KubernetesQueryCommand, KubernetesValidationError>,
        subject: &str,
        cancellation: impl Future<Output = ()> + Send,
    ) -> ServerResult<McpToolResult> {
        let command = match command {
            Ok(command) => command,
            Err(_) => {
                return Ok(kubernetes_tool_error(
                    subject,
                    KubernetesError::InvalidArguments,
                ));
            }
        };
        match self
            .services
            .kubernetes
            .dispatch_cancelled(&command, cancellation)
            .await
        {
            Ok(output) => Ok(json_result(
                serde_json::to_value(output).expect("Kubernetes query result must serialize"),
            )),
            Err(KubernetesError::RequestCancelled) => {
                Err(ServerError::internal("request cancelled"))
            }
            Err(error) => Ok(kubernetes_tool_error(subject, error)),
        }
    }

    async fn dispatch_kubernetes_exec(
        &self,
        command: Result<KubernetesExecCommand, KubernetesValidationError>,
        subject: &str,
        cancellation: impl Future<Output = ()> + Send,
    ) -> McpToolResult {
        let command = match command {
            Ok(command) => command,
            Err(_) => return kubernetes_tool_error(subject, KubernetesError::InvalidArguments),
        };
        match self
            .services
            .kubernetes
            .execute_cancelled(&command, cancellation)
            .await
        {
            Ok(output) => json_result(
                serde_json::to_value(output).expect("Kubernetes mutation result must serialize"),
            ),
            Err(error) => kubernetes_tool_error(subject, error),
        }
    }

    /// List the configured Ceph cluster catalog without contacting a Dashboard.
    async fn ceph_clusters(
        &self,
        input: CephClusterListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "cluster", context.cancelled())
            .await
    }

    /// Get bounded normalized native health for one configured Ceph cluster.
    async fn ceph_status(
        &self,
        input: CephStatusGetInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "status", context.cancelled())
            .await
    }

    /// Get a bounded current metrics snapshot for one configured Ceph cluster.
    async fn ceph_metrics(
        &self,
        input: CephMetricsSummaryInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "metrics", context.cancelled())
            .await
    }

    /// List bounded normalized OSD summaries for one configured Ceph cluster.
    async fn ceph_osds(
        &self,
        input: CephOsdListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "OSD", context.cancelled())
            .await
    }

    /// Get one exact normalized Ceph OSD.
    async fn ceph_osd(
        &self,
        input: CephOsdGetInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "OSD", context.cancelled())
            .await
    }

    /// Ask Ceph whether one exact OSD is currently safe to destroy.
    async fn ceph_osd_safe_to_destroy(
        &self,
        input: CephOsdSafeToDestroyInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "OSD", context.cancelled())
            .await
    }

    /// List bounded normalized devices attached to one exact Ceph OSD.
    async fn ceph_devices(
        &self,
        input: CephDeviceListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "device", context.cancelled())
            .await
    }

    /// Get one exact normalized device attached to one exact Ceph OSD.
    async fn ceph_device(
        &self,
        input: CephDeviceGetInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "device", context.cancelled())
            .await
    }

    /// Get the current state of curated Ceph cluster flags.
    async fn ceph_flags(
        &self,
        input: CephFlagsGetInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "flags", context.cancelled())
            .await
    }

    /// List bounded current and recent Ceph Dashboard tasks.
    async fn ceph_tasks(
        &self,
        input: CephTaskListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        self.dispatch_ceph_query(input.validate(), "task", context.cancelled())
            .await
    }

    /// Mark one exact Ceph OSD in, out, or down.
    async fn ceph_mark_osd(
        &self,
        input: CephOsdMarkInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_ceph_exec(input.validate(), "OSD", context.cancelled())
            .await)
    }

    /// Reweight one exact Ceph OSD to a finite value from zero through one.
    async fn ceph_reweight_osd(
        &self,
        input: CephOsdReweightInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_ceph_exec(input.validate(), "OSD", context.cancelled())
            .await)
    }

    /// Request a normal or deep scrub of one exact Ceph OSD.
    async fn ceph_scrub_osd(
        &self,
        input: CephOsdScrubInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_ceph_exec(input.validate(), "OSD", context.cancelled())
            .await)
    }

    /// Destroy one explicitly confirmed OSD only after a fresh safety check.
    async fn ceph_destroy_osd(
        &self,
        input: CephOsdDestroyInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_ceph_exec(input.validate(), "OSD", context.cancelled())
            .await)
    }

    /// Purge one explicitly confirmed OSD only after a fresh safety check.
    async fn ceph_purge_osd(
        &self,
        input: CephOsdPurgeInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(self
            .dispatch_ceph_exec(input.validate(), "OSD", context.cancelled())
            .await)
    }

    async fn dispatch_ceph_query(
        &self,
        command: Result<CephQueryCommand, CephValidationError>,
        subject: &str,
        cancellation: impl Future<Output = ()>,
    ) -> ServerResult<McpToolResult> {
        let command = match command {
            Ok(command) => command,
            Err(_) => return Ok(ceph_tool_error(subject, CephError::InvalidArguments)),
        };
        let result = tokio::select! {
            result = self.services.ceph.query(&command) => result,
            () = cancellation => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(output) => json_result(output),
            Err(error) => ceph_tool_error(subject, error),
        })
    }

    async fn dispatch_ceph_exec(
        &self,
        command: Result<CephExecCommand, CephValidationError>,
        subject: &str,
        cancellation: impl Future<Output = ()>,
    ) -> McpToolResult {
        let command = match command {
            Ok(command) => command,
            Err(_) => return ceph_tool_error(subject, CephError::InvalidArguments),
        };
        let result = tokio::select! {
            result = self.services.ceph.execute(&command) => result,
            () = cancellation => return ceph_tool_error(subject, CephError::MutationOutcomeUnknown),
        };
        match result {
            Ok(output) => json_result(output),
            Err(error) => ceph_tool_error(subject, error),
        }
    }

    /// List bounded machine inventory including explicit public host pins.
    async fn machine_list(
        &self,
        input: MachineListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let result = tokio::select! {
            result = self.services.inventory.list(input.limit) => result,
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(match result {
            Ok(machines) => json_result(
                json!({"machines": machines.iter().map(machine_json).collect::<Vec<_>>() }),
            ),
            Err(error) => inventory_tool_error(error),
        })
    }

    /// Create one exact machine inventory record.
    async fn machine_create(
        &self,
        input: MachineCreateInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let command = CreateMachine {
            display_name: input.display_name,
            ssh_host: input.ssh_host,
            ssh_port: input.ssh_port,
            ssh_username: input.ssh_username,
            pinned_host_public_key: input.pinned_host_public_key,
        };
        let result = tokio::select! {
            result = self.services.inventory.create(command) => result,
            () = context.cancelled() => return Ok(inventory_outcome_unknown()),
        };
        Ok(match result {
            Ok(machine) => json_result(machine_json(&machine)),
            Err(error) => inventory_tool_error(error),
        })
    }

    /// Update the connection fields of one exact machine UUID without changing host trust.
    async fn machine_update(
        &self,
        input: MachineUpdateInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let Some(id) = parse_machine_id(&input.machine_id) else {
            return Ok(invalid_machine_arguments());
        };
        let command = UpdateMachine {
            display_name: input.display_name,
            ssh_host: input.ssh_host,
            ssh_port: input.ssh_port,
            ssh_username: input.ssh_username,
        };
        let result = tokio::select! {
            result = self.services.inventory.update(id, command) => result,
            () = context.cancelled() => return Ok(inventory_outcome_unknown()),
        };
        Ok(match result {
            Ok(machine) => json_result(machine_json(&machine)),
            Err(error) => inventory_tool_error(error),
        })
    }

    /// Delete one exact machine UUID.
    async fn machine_delete(
        &self,
        input: MachineIdInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let Some(id) = parse_machine_id(&input.machine_id) else {
            return Ok(invalid_machine_arguments());
        };
        let result = tokio::select! {
            result = self.services.inventory.delete(id) => result,
            () = context.cancelled() => return Ok(inventory_outcome_unknown()),
        };
        Ok(match result {
            Ok(()) => json_result(json!({"machine_id": id.to_string(), "deleted": true})),
            Err(error) => inventory_tool_error(error),
        })
    }

    /// Clear the public host pin for one exact machine, immediately making it untrusted.
    async fn machine_host_key_clear(
        &self,
        input: MachineIdInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let Some(id) = parse_machine_id(&input.machine_id) else {
            return Ok(invalid_machine_arguments());
        };
        let result = tokio::select! {
            result = self.services.inventory.clear_host_key(id) => result,
            () = context.cancelled() => return Ok(inventory_outcome_unknown()),
        };
        Ok(match result {
            Ok(machine) => json_result(machine_json(&machine)),
            Err(error) => inventory_tool_error(error),
        })
    }

    /// Replace the public host pin for one exact machine with the supplied key.
    async fn machine_host_key_replace(
        &self,
        input: MachineHostKeyInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let Some(id) = parse_machine_id(&input.machine_id) else {
            return Ok(invalid_machine_arguments());
        };
        let result = tokio::select! {
            result = self.services.inventory.replace_host_key(id, input.host_public_key) => result,
            () = context.cancelled() => return Ok(inventory_outcome_unknown()),
        };
        Ok(match result {
            Ok(machine) => json_result(machine_json(&machine)),
            Err(error) => inventory_tool_error(error),
        })
    }

    /// List configured deploy metadata without contacting a machine or OpenBao.
    async fn deploy_list(
        &self,
        _: DeployListInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        Ok(json_result(
            json!({"deploys": self.services.deploys.list()}),
        ))
    }

    /// Run one approved deploy on one exact machine UUID.
    async fn deploy_run(
        &self,
        input: DeployRunInput,
        context: ServerContext,
    ) -> ServerResult<McpToolResult> {
        let _progress_heartbeat = self.progress_heartbeat(&context);
        let Some(id) = parse_machine_id(&input.machine_id) else {
            return Ok(invalid_deploy_arguments());
        };
        let machine = tokio::select! {
            result = self.services.inventory.get(id) => match result { Ok(machine) => machine, Err(error) => return Ok(inventory_tool_error(error)) },
            () = context.cancelled() => return Err(ServerError::internal("request cancelled")),
        };
        Ok(self
            .dispatch_deploy_run(&input.deploy_id, machine, context.cancelled())
            .await)
    }

    async fn dispatch_deploy_run(
        &self,
        deploy_id: &str,
        machine: Machine,
        cancellation: impl Future<Output = ()> + Send,
    ) -> McpToolResult {
        let mut cancellation = Box::pin(cancellation);
        let result = self
            .services
            .deploys
            .run_cancelled(deploy_id, machine, cancellation.as_mut())
            .await;
        match result {
            Ok(output) => {
                json_result(serde_json::to_value(output).expect("deploy result must serialize"))
            }
            Err(error) => deploy_tool_error(error),
        }
    }
}

fn parse_machine_id(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value).ok()
}

fn machine_json(machine: &Machine) -> serde_json::Value {
    json!({
        "id": machine.id.to_string(),
        "display_name": machine.display_name,
        "ssh_host": machine.ssh_host,
        "ssh_port": machine.ssh_port,
        "ssh_username": machine.ssh_username,
        "host_key_pinned": machine.pinned_host_public_key.is_some(),
        "pinned_host_public_key": machine.pinned_host_public_key,
        "created_at": machine.created_at.to_rfc3339(),
        "updated_at": machine.updated_at.to_rfc3339(),
    })
}

fn invalid_machine_arguments() -> McpToolResult {
    ToolError::new("invalid_arguments", "Invalid machine arguments.", false).into_mcp_result()
}
fn invalid_deploy_arguments() -> McpToolResult {
    ToolError::new("invalid_arguments", "Invalid deploy arguments.", false).into_mcp_result()
}
fn inventory_outcome_unknown() -> McpToolResult {
    ToolError::new(
        "mutation_outcome_unknown",
        "Machine mutation outcome is unknown.",
        true,
    )
    .into_mcp_result()
}

fn inventory_tool_error(error: RepositoryError) -> McpToolResult {
    let (code, message, retryable) = match error {
        RepositoryError::Validation(_) => {
            ("invalid_arguments", "Invalid machine arguments.", false)
        }
        RepositoryError::NotFound => ("machine_not_found", "Machine not found.", false),
        RepositoryError::Conflict => (
            "machine_conflict",
            "Machine conflicts with existing inventory.",
            false,
        ),
        RepositoryError::Database => (
            "inventory_unavailable",
            "Machine inventory is unavailable.",
            true,
        ),
    };
    ToolError::new(code, message, retryable).into_mcp_result()
}

fn deploy_tool_error(error: DeployError) -> McpToolResult {
    let retryable = matches!(
        error,
        DeployError::Busy
            | DeployError::CredentialUnavailable
            | DeployError::ExecutionOutcomeUnknown
            | DeployError::TimeoutOutcomeUnknown
            | DeployError::CancelledOutcomeUnknown
            | DeployError::OutputTooLargeOutcomeUnknown
    );
    let message = match error {
        DeployError::DeployNotFound => "Deploy not found.",
        DeployError::DeployUnavailable => "Deploy is not available through MCP.",
        DeployError::MissingHostPin => "Machine has no trusted host key pin.",
        DeployError::Busy => "Another deploy is already active.",
        DeployError::Cancelled => "Deploy was cancelled before execution.",
        DeployError::ExecutionOutcomeUnknown | DeployError::CancelledOutcomeUnknown => {
            "Deploy execution outcome is unknown."
        }
        DeployError::TimeoutOutcomeUnknown => "Deploy timed out and its outcome is unknown.",
        DeployError::OutputTooLargeOutcomeUnknown => {
            "Deploy output exceeded its bound and the outcome is unknown."
        }
        DeployError::CredentialUnavailable => "Deploy credential service is unavailable.",
        DeployError::CredentialInvalid => "Deploy credential response was invalid.",
        DeployError::InvalidOutput => "Deploy returned invalid structured output.",
        DeployError::ExecutionRejected => "Deploy execution failed.",
        DeployError::InvalidConfiguration | DeployError::InvalidCatalog => {
            "Deploy service is unavailable."
        }
    };
    ToolError::new(error.code(), message, retryable).into_mcp_result()
}

fn json_result(output: serde_json::Value) -> McpToolResult {
    mcp::progressive::tool_result(output, None)
        .expect("unfiltered JSON output must produce a tool result")
}

fn silence_result(output: serde_json::Value) -> McpToolResult {
    let silence_id = output["silence_id"].as_str().unwrap_or("unknown");
    McpToolResult::new(json!({
        "content": [{"type":"text","text":format!("Created Grafana silence {silence_id}.")}],
        "structuredContent": output
    }))
}

fn render_result(
    render_type: &str,
    uid: &str,
    panel_id: Option<&str>,
    options: &crate::integrations::grafana::actions::RenderOptions,
    image: RenderedImage,
) -> McpToolResult {
    let mut structured = json!({
        "render_type": render_type,
        "uid": uid,
        "from": options.from,
        "to": options.to,
        "width": options.width,
        "height": options.height,
        "scale": options.scale,
        "theme": options.theme.as_str(),
        "timezone": options.timezone,
        "bytes": image.decoded_bytes,
        "sha256": image.sha256,
    });
    if let Some(panel_id) = panel_id {
        structured["panel_id"] = json!(panel_id);
    }
    McpToolResult::new(json!({
        "content": [
            {"type":"text", "text":format!("Rendered Grafana {render_type} PNG ({} bytes).", image.decoded_bytes)},
            {"type":"image", "data":STANDARD.encode(image.bytes), "mimeType":"image/png"}
        ],
        "structuredContent": structured,
    }))
}

fn tool_error(query_name: &str, error: GrafanaError) -> McpToolResult {
    error.into_tool_error(query_name).into_mcp_result()
}

fn tekton_tool_error(subject: &str, error: TektonError) -> McpToolResult {
    error.into_tool_error(subject).into_mcp_result()
}

fn kubernetes_tool_error(subject: &str, error: KubernetesError) -> McpToolResult {
    error.into_tool_error(subject).into_mcp_result()
}

fn ceph_tool_error(subject: &str, error: CephError) -> McpToolResult {
    error.into_tool_error(subject).into_mcp_result()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::integrations::grafana::GrafanaClient;
    use axum::{
        Json, Router,
        body::Body,
        extract::State,
        http::{HeaderMap, Request as HttpRequest},
        response::IntoResponse,
        routing::{get, post},
    };
    use mcp::{
        McpPrincipalId,
        protocol::MCP_PROTOCOL_VERSION,
        server::{
            McpHostedTokenValidation, McpTokenAuthorization, StreamableHttpAuthorization,
            StreamableHttpOptions,
        },
    };
    use opentelemetry::trace::{SpanId, SpanKind, Status, TracerProvider as _};
    use opentelemetry_sdk::{
        error::OTelSdkResult,
        trace::{SdkTracerProvider, SpanData, SpanExporter},
    };
    use reqwest::{Client, StatusCode};
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::{io::AsyncWriteExt as _, net::TcpListener, sync::Notify, task::JoinHandle};
    use tracing_subscriber::{Layer as _, layer::SubscriberExt as _};

    use super::*;

    #[test]
    fn machine_input_schemas_match_the_fixed_ssh_identity() {
        for schema in [
            schemars::schema_for!(MachineCreateInput),
            schemars::schema_for!(MachineUpdateInput),
        ] {
            let schema = serde_json::to_value(schema).unwrap();
            let properties = &schema["properties"];
            assert_eq!(properties["ssh_port"]["minimum"], 22);
            assert_eq!(properties["ssh_port"]["maximum"], 22);
            assert_eq!(properties["ssh_username"]["minLength"], 7);
            assert_eq!(properties["ssh_username"]["maxLength"], 7);
            assert_eq!(properties["ssh_username"]["pattern"], "^homelab$");
        }
    }

    type PropagatedRequests = Arc<Mutex<Vec<(String, String)>>>;

    struct CancellingDeploys(Arc<AtomicBool>);

    impl crate::services::DeployService for CancellingDeploys {
        fn list(&self) -> Vec<crate::integrations::deploys::DeployMetadata> {
            Vec::new()
        }

        fn run_cancelled<'a>(
            &'a self,
            _: &'a str,
            _: Machine,
            mut cancellation: std::pin::Pin<&'a mut (dyn Future<Output = ()> + Send)>,
        ) -> crate::services::ServiceFuture<
            'a,
            Result<crate::integrations::deploys::DeployResult, DeployError>,
        > {
            Box::pin(async move {
                cancellation.as_mut().await;
                self.0.store(true, Ordering::Relaxed);
                Err(DeployError::CancelledOutcomeUnknown)
            })
        }
    }

    async fn serve(router: Router) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (origin, task)
    }

    async fn test_handler() -> (Arc<HomelabMcp>, PropagatedRequests, JoinHandle<()>) {
        let propagated = Arc::new(Mutex::new(Vec::new()));
        let grafana = Router::new()
            .route(
                "/api/search",
                get(|| async {
                    Json(json!([{
                        "id":1, "uid":"dash-1", "title":"Overview", "uri":"db/overview",
                        "url":"/d/dash-1/overview", "slug":"overview", "type":"dash-db",
                        "folderUid":"folder-1", "folderTitle":"Operations", "tags":["prod"]
                    }]))
                }),
            )
            .route(
                "/api/dashboards/uid/dash-1",
                get(|| async {
                    Json(json!({
                        "meta":{"folderUid":"folder-1","folderTitle":"Operations","url":"/d/dash-1/overview"},
                        "dashboard":{
                            "id":1,"uid":"dash-1","title":"Overview","tags":["prod"],
                            "timezone":"browser","version":4,"schemaVersion":42,
                            "templating":{"list":[{"name":"cluster","label":"Cluster","type":"query","query":"secret"}]},
                            "panels":[{"id":13,"title":"CPU","type":"timeseries","targets":[{"expr":"secret"}]}]
                        }
                    }))
                }),
            )
            .route(
                "/render/d/dash-1",
                get(|| async { ([("content-type", "image/png; charset=binary")], b"\x89PNG\r\n\x1a\nimage".as_slice()) }),
            )
            .route(
                "/render/d-solo/dash-1",
                get(|| async { ([("content-type", "image/png")], b"\x89PNG\r\n\x1a\npanel".as_slice()) }),
            )
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                get(
                    |State(propagated): State<PropagatedRequests>,
                     headers: HeaderMap| async move {
                        record_propagated_context(&propagated, "GET", &headers);
                    Json(json!({
                        "status":"success",
                        "data":{"resultType":"vector","result":[{"metric":{"job":"test"},"value":[1786276800,"2"]}]}
                    }))
                    },
                ),
            )
            .route(
                "/api/v1/provisioning/alert-rules",
                get(|| async {
                    Json(json!([
                        {
                            "uid":"recording-1", "title":"API request rate", "folderUID":"folder-1",
                            "ruleGroup":"api", "record":{"metric":"api_request_rate","from":"A"},
                            "isPaused":false, "labels":{"team":"platform"}
                        },
                        {
                            "uid":"rule-1", "title":"API errors", "folderUID":"folder-1",
                            "ruleGroup":"api", "condition":"C", "noDataState":"NoData",
                            "execErrState":"Error", "for":"5m", "isPaused":false,
                            "labels":{"severity":"critical"}, "annotations":{"summary":"API is failing"}
                        }
                    ]))
                }),
            )
            .route(
                "/api/alertmanager/grafana/api/v2/alerts",
                get(|| async {
                    Json(json!([{
                        "fingerprint":"abc123", "startsAt":"2026-08-10T12:00:00Z",
                        "endsAt":"2026-08-10T13:00:00Z", "updatedAt":"2026-08-10T12:01:00Z",
                        "status":{"state":"active","silencedBy":[],"inhibitedBy":[]},
                        "labels":{"alertname":"APIError"}, "annotations":{"summary":"API is failing"}
                    }]))
                }),
            )
            .route(
                "/api/alertmanager/grafana/api/v2/silences",
                get(|| async {
                    Json(json!([{
                        "id":"silence-active", "status":{"state":"active"},
                        "startsAt":"2026-08-10T12:00:00Z", "endsAt":"2026-08-10T13:00:00Z",
                        "createdBy":"homelab-mcp", "comment":"maintenance",
                        "matchers":[{"name":"alertname","value":"APIError","isRegex":false,"isEqual":true}]
                    }]))
                })
                .post(
                    |State(propagated): State<PropagatedRequests>,
                     headers: HeaderMap,
                     Json(_body): Json<Value>| async move {
                        record_propagated_context(&propagated, "POST", &headers);
                        Json(json!({"silenceID":"silence-123"}))
                    },
                ),
            )
            .with_state(Arc::clone(&propagated));
        let (origin, task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services::new(GrafanaClient::for_test(
                url::Url::parse(&format!("{origin}/")).unwrap(),
                std::time::Duration::from_secs(1),
            ))),
            catalog: Arc::new(skills::catalog().unwrap()),
            progress_heartbeat_interval: PROGRESS_HEARTBEAT_INTERVAL,
        });
        (handler, propagated, task)
    }

    fn record_propagated_context(
        propagated: &Mutex<Vec<(String, String)>>,
        method: &str,
        headers: &HeaderMap,
    ) {
        propagated.lock().unwrap().push((
            method.to_owned(),
            headers
                .get("traceparent")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        ));
    }

    #[test]
    fn unfiltered_grafana_and_tekton_text_matches_structured_json() {
        for output in [
            json!({
                "mode":"list", "result_type":"dashboards",
                "result":[{"uid":"dash-1","title":"Overview"}]
            }),
            json!({
                "status":"accepted", "source_run_id":"pipelines-as-code/run-1",
                "repository":"rfhold/repo", "workflow":"workflow/abc"
            }),
        ] {
            let result = json_result(output).raw;
            let text = result["content"][0]["text"].as_str().unwrap();
            let parsed: Value = serde_json::from_str(text).unwrap();

            assert_eq!(parsed, result["structuredContent"]);
        }
    }

    fn request(method: &str, id: &str, params: Value) -> Value {
        let mut body = json!({
            "jsonrpc":"2.0", "id":id, "method":method, "params":params
        });
        body["params"]["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": {"name":"homelab-tests","version":"1"}
        });
        body
    }

    fn action_schemas(tool: &Value) -> Vec<Value> {
        let schema = &tool["inputSchema"]["properties"];
        let actions = schema["action"]["enum"].as_array().unwrap();
        let inputs = schema["input"]["oneOf"].as_array().unwrap();
        assert_eq!(actions.len(), inputs.len());
        actions
            .iter()
            .zip(inputs)
            .map(|(action, input)| json!({"action":action,"input_schema":input}))
            .collect()
    }

    mod skills_contract;

    async fn post_mcp(endpoint: &str, body: Value) -> (StatusCode, Value) {
        let method = body["method"].as_str().unwrap();
        let mut request = Client::new()
            .post(endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("mcp-method", method);
        if let Some(name) = body["params"]["name"].as_str() {
            request = request.header("mcp-name", name);
        }
        if method == "resources/read" {
            request = request.header("mcp-name", body["params"]["uri"].as_str().unwrap());
        }
        let response = request.json(&body).send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        let payload = text
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap_or(&text);
        (status, serde_json::from_str(payload).unwrap())
    }

    async fn progress_test_server(
        action_duration: Duration,
        heartbeat_interval: Duration,
    ) -> (String, Arc<AtomicBool>, JoinHandle<()>, JoinHandle<()>) {
        let completed = Arc::new(AtomicBool::new(false));
        let upstream_completed = Arc::clone(&completed);
        let grafana = Router::new().route(
            "/api/datasources/proxy/uid/loki/loki/api/v1/query",
            get(move || {
                let upstream_completed = Arc::clone(&upstream_completed);
                async move {
                    tokio::time::sleep(action_duration).await;
                    upstream_completed.store(true, Ordering::SeqCst);
                    Json(json!({
                        "status":"success",
                        "data":{"resultType":"vector","result":[]}
                    }))
                }
            }),
        );
        let (grafana_origin, grafana_task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services::new(GrafanaClient::for_test(
                url::Url::parse(&format!("{grafana_origin}/")).unwrap(),
                Duration::from_secs(1),
            ))),
            catalog: Arc::new(skills::catalog().unwrap()),
            progress_heartbeat_interval: heartbeat_interval,
        });
        let (mcp_origin, mcp_task) = serve(streamable_http_router(handler)).await;
        (
            format!("{mcp_origin}/mcp"),
            completed,
            grafana_task,
            mcp_task,
        )
    }

    fn progress_call(token: Option<Value>) -> Value {
        let mut body = request(
            "tools/call",
            "progress-call",
            json!({
                "name": "query",
                "arguments":{
                    "action":"grafana.logql.query",
                    "input":{"query":"{job=\"progress-test\"}"}
                }
            }),
        );
        if let Some(token) = token {
            body["params"]["_meta"]["progressToken"] = token;
        }
        body
    }

    async fn send_mcp_stream(endpoint: &str, body: &Value) -> reqwest::Response {
        Client::new()
            .post(endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("mcp-method", "tools/call")
            .header("mcp-name", QUERY_TOOL_NAME)
            .json(body)
            .send()
            .await
            .unwrap()
    }

    async fn next_sse_payload(
        response: &mut reqwest::Response,
        buffered: &mut Vec<u8>,
    ) -> Option<Value> {
        loop {
            if let Some(end) = buffered.windows(2).position(|window| window == b"\n\n") {
                let frame = buffered.drain(..end + 2).collect::<Vec<_>>();
                let frame = std::str::from_utf8(&frame).unwrap();
                if let Some(data) = frame.lines().find_map(|line| line.strip_prefix("data: ")) {
                    return Some(serde_json::from_str(data).unwrap());
                }
                continue;
            }
            match response.chunk().await.unwrap() {
                Some(chunk) => buffered.extend_from_slice(&chunk),
                None => return None,
            }
        }
    }

    #[tokio::test]
    async fn progress_heartbeat_streams_increasing_correlated_events_before_completion() {
        for token in [json!("request-progress"), json!(42)] {
            let (endpoint, completed, grafana_task, mcp_task) =
                progress_test_server(Duration::from_millis(120), Duration::from_millis(15)).await;
            let mut response =
                send_mcp_stream(&endpoint, &progress_call(Some(token.clone()))).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[reqwest::header::CONTENT_TYPE],
                "text/event-stream"
            );

            let mut buffered = Vec::new();
            let first = tokio::time::timeout(
                Duration::from_millis(100),
                next_sse_payload(&mut response, &mut buffered),
            )
            .await
            .expect("progress must arrive before terminal completion")
            .expect("progress SSE payload");
            assert_eq!(first["method"], "notifications/progress");
            assert!(!completed.load(Ordering::SeqCst));

            let mut payloads = vec![first];
            while let Some(payload) = next_sse_payload(&mut response, &mut buffered).await {
                payloads.push(payload);
            }
            let progress = payloads
                .iter()
                .filter(|payload| payload["method"] == "notifications/progress")
                .collect::<Vec<_>>();
            assert!(progress.len() >= 2, "payloads: {payloads:?}");
            for (index, payload) in progress.iter().enumerate() {
                let params = payload["params"].as_object().unwrap();
                assert_eq!(params.len(), 3);
                assert_eq!(params["progressToken"], token);
                assert_eq!(params["progress"], json!((index + 1) as f64));
                assert_eq!(params["message"], PROGRESS_HEARTBEAT_MESSAGE);
                assert!(!params.contains_key("total"));
            }
            assert!(payloads.last().unwrap().get("result").is_some());
            assert!(completed.load(Ordering::SeqCst));
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(40),
                    next_sse_payload(&mut response, &mut buffered)
                )
                .await
                .expect("completed stream must close")
                .is_none()
            );

            grafana_task.abort();
            mcp_task.abort();
        }
    }

    #[tokio::test]
    async fn progress_heartbeat_omits_events_without_token_and_for_short_calls() {
        for (action_duration, token) in [
            (Duration::from_millis(70), None),
            (Duration::from_millis(1), Some(json!(7))),
        ] {
            let (endpoint, _, grafana_task, mcp_task) =
                progress_test_server(action_duration, Duration::from_millis(15)).await;
            let response = send_mcp_stream(&endpoint, &progress_call(token)).await;
            let payload = response.text().await.unwrap();
            assert!(!payload.contains("notifications/progress"), "{payload}");
            assert!(payload.contains("structuredContent"), "{payload}");
            grafana_task.abort();
            mcp_task.abort();
        }
    }

    #[derive(Clone, Debug, Default)]
    struct TestSpanExporter(Arc<Mutex<Vec<SpanData>>>);

    impl SpanExporter for TestSpanExporter {
        async fn export(&self, mut batch: Vec<SpanData>) -> OTelSdkResult {
            self.0.lock().unwrap().append(&mut batch);
            Ok(())
        }
    }

    #[derive(Clone)]
    struct SpanTargetCapture(Arc<Mutex<Vec<(&'static str, &'static str)>>>);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for SpanTargetCapture {
        fn on_new_span(
            &self,
            attributes: &tracing::span::Attributes<'_>,
            _id: &tracing::span::Id,
            _context: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let metadata = attributes.metadata();
            self.0
                .lock()
                .unwrap()
                .push((metadata.name(), metadata.target()));
        }
    }

    struct TelemetryCapture {
        spans: Vec<SpanData>,
        targets: Vec<(&'static str, &'static str)>,
        propagated: Vec<(String, String)>,
    }

    fn capture_authorized_call(runtime: &tokio::runtime::Runtime) -> TelemetryCapture {
        let exporter = TestSpanExporter::default();
        let targets = Arc::new(Mutex::new(Vec::new()));
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let ignored_json_targets = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry()
            .with(SpanTargetCapture(targets.clone()))
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("homelab-mcp-test")))
            .with(
                SpanTargetCapture(ignored_json_targets)
                    .with_filter(crate::observability::test_json_filter()),
            )
            .with(crate::observability::test_trace_filter());

        let propagated = tracing::subscriber::with_default(subscriber, || {
            runtime.block_on(async {
                opentelemetry::global::set_text_map_propagator(
                    opentelemetry_sdk::propagation::TraceContextPropagator::new(),
                );
                let (handler, propagated, grafana_task) = test_handler().await;
                let metadata = McpProtectedResourceMetadata::new(
                    "https://mcp.example.test/mcp",
                    ["https://mcp.example.test/oauth"],
                )
                .with_scopes(["mcp:use"]);
                let authorization = StreamableHttpAuthorization::hosted(metadata, |_, _| {
                    Box::pin(async {
                        McpHostedTokenValidation::Authorized(McpTokenAuthorization {
                            principal_id: McpPrincipalId::new("telemetry-test").unwrap(),
                            expires_at: None,
                            revocation: None,
                        })
                    })
                })
                .unwrap();
                let authorization = authorization.with_required_scopes(["mcp:use"]);
                let router = streamable_http_router_with_options(
                    handler,
                    StreamableHttpOptions::default()
                        .without_root_protected_resource_metadata()
                        .with_authorization(authorization),
                );
                let (origin, mcp_task) = serve(router).await;
                let endpoint = format!("{origin}/mcp");
                let body = request(
                    "tools/call",
                    "telemetry-call",
                    json!({
                        "name": "query",
                        "arguments": {
                            "action":"grafana.logql.query",
                            "input": {"query": "{job=\"telemetry-test\"}"}
                        }
                    }),
                );
                let response = Client::new()
                    .post(&endpoint)
                    .header("accept", "application/json, text/event-stream")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
                    .header("mcp-method", "tools/call")
                    .header("mcp-name", QUERY_TOOL_NAME)
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let payload = response.text().await.unwrap();
                assert!(payload.contains("structuredContent"));
                assert!(!payload.contains("test-token"));

                let body = request(
                    "tools/call",
                    "telemetry-post",
                    json!({
                        "name": "create",
                        "arguments": {
                            "action":"grafana.silence.create",
                            "input": {
                                "matchers": [{"name":"alertname","operator":"=","value":"SensitiveMatcher"}],
                                "duration_seconds": 3600,
                                "comment": "SensitiveComment"
                            }
                        }
                    }),
                );
                let response = Client::new()
                    .post(&endpoint)
                    .header("accept", "application/json, text/event-stream")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
                    .header("mcp-method", "tools/call")
                    .header("mcp-name", CREATE_TOOL_NAME)
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert!(response.text().await.unwrap().contains("structuredContent"));

                let disconnect_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let disconnect_origin = url::Url::parse(&format!(
                    "http://{}/sensitive-path?secret=query",
                    disconnect_listener.local_addr().unwrap()
                ))
                .unwrap();
                let disconnect_task = tokio::spawn(async move {
                    let (connection, _) = disconnect_listener.accept().await.unwrap();
                    drop(connection);
                });
                let unavailable = GrafanaClient::for_test(
                    disconnect_origin,
                    std::time::Duration::from_secs(1),
                );
                assert!(
                    unavailable
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveRawErrorQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await
                        .is_err()
                );
                disconnect_task.await.unwrap();

                let http_error = Router::new().route(
                    "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                    get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
                );
                let (origin, http_error_task) = serve(http_error).await;
                let unavailable = GrafanaClient::for_test(
                    url::Url::parse(&format!("{origin}/")).unwrap(),
                    std::time::Duration::from_secs(1),
                );
                assert!(
                    unavailable
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveHttpErrorQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await
                        .is_err()
                );
                http_error_task.abort();

                let slow = Router::new().route(
                    "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                    get(|| async {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        Json(json!({}))
                    }),
                );
                let (origin, slow_task) = serve(slow).await;
                let timeout = GrafanaClient::for_test(
                    url::Url::parse(&format!("{origin}/")).unwrap(),
                    std::time::Duration::from_millis(10),
                );
                assert_eq!(
                    timeout
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveTimeoutQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await,
                    Err(crate::integrations::grafana::Error::Timeout)
                );
                slow_task.abort();

                let truncated_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let truncated_origin = url::Url::parse(&format!(
                    "http://{}/",
                    truncated_listener.local_addr().unwrap()
                ))
                .unwrap();
                let truncated_task = tokio::spawn(async move {
                    let (mut connection, _) = truncated_listener.accept().await.unwrap();
                    connection
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{",
                        )
                        .await
                        .unwrap();
                });
                let truncated = GrafanaClient::for_test(
                    truncated_origin,
                    std::time::Duration::from_secs(1),
                );
                assert_eq!(
                    truncated
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveTruncatedBodyQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await,
                    Err(crate::integrations::grafana::Error::UpstreamUnavailable)
                );
                truncated_task.await.unwrap();

                let oversized_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let oversized_origin = url::Url::parse(&format!(
                    "http://{}/",
                    oversized_listener.local_addr().unwrap()
                ))
                .unwrap();
                let oversized_task = tokio::spawn(async move {
                    let (mut connection, _) = oversized_listener.accept().await.unwrap();
                    connection
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 4194305\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .unwrap();
                });
                let oversized = GrafanaClient::for_test(
                    oversized_origin,
                    std::time::Duration::from_secs(1),
                );
                assert_eq!(
                    oversized
                        .execute(
                            &crate::integrations::grafana::actions::LogqlInput {
                                query: "SensitiveOversizedBodyQuery".to_owned(),
                                start: None,
                                end: None,
                                time: None,
                                direction: None,
                                limit: None,
                            }
                            .validate()
                            .unwrap()
                        )
                        .await,
                    Err(crate::integrations::grafana::Error::InvalidResponse)
                );
                oversized_task.await.unwrap();

                let propagated = propagated.lock().unwrap().clone();
                grafana_task.abort();
                mcp_task.abort();
                propagated
            })
        });

        provider.force_flush().unwrap();
        let spans = exporter.0.lock().unwrap().clone();
        let targets = targets.lock().unwrap().clone();
        TelemetryCapture {
            spans,
            targets,
            propagated,
        }
    }

    #[test]
    fn host_filters_export_grafana_http_hierarchy_and_safe_propagation() {
        const CHILD_MARKER: &str = "HOMELAB_MCP_TELEMETRY_TEST_CHILD";
        if std::env::var_os(CHILD_MARKER).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "mcp::tests::host_filters_export_grafana_http_hierarchy_and_safe_propagation",
                    "--nocapture",
                ])
                .env(CHILD_MARKER, "1")
                .env("RUST_LOG", "homelab_mcp=info")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let TelemetryCapture {
            spans,
            targets,
            propagated,
        } = capture_authorized_call(&runtime);
        let exported = spans
            .iter()
            .map(|span| (span.name.as_ref(), span.instrumentation_scope.name()))
            .collect::<Vec<_>>();
        let server_spans = spans
            .iter()
            .filter(|span| span.name == "mcp.server.request")
            .collect::<Vec<_>>();
        let grafana_spans = spans
            .iter()
            .filter(|span| span.name == "grafana.query")
            .collect::<Vec<_>>();
        let client_spans = spans
            .iter()
            .filter(|span| span.name == "http.client.request")
            .collect::<Vec<_>>();

        assert!(targets.contains(&("mcp.server.request", "mcp::server")));
        assert!(targets.contains(&("grafana.query", "homelab_mcp::grafana")));
        assert!(targets.contains(&("http.client.request", "homelab_mcp::http_client")));
        assert_eq!(server_spans.len(), 2, "exported spans: {exported:?}");
        assert_eq!(grafana_spans.len(), 7);
        assert_eq!(client_spans.len(), 7);
        for server in &server_spans {
            assert_eq!(server.parent_span_id, SpanId::INVALID);
            let grafana_children = grafana_spans
                .iter()
                .filter(|grafana| {
                    grafana.parent_span_id == server.span_context.span_id()
                        && grafana.span_context.trace_id() == server.span_context.trace_id()
                })
                .collect::<Vec<_>>();
            assert_eq!(grafana_children.len(), 1);
            let client_children = client_spans
                .iter()
                .filter(|client| {
                    client.parent_span_id == grafana_children[0].span_context.span_id()
                        && client.span_context.trace_id()
                            == grafana_children[0].span_context.trace_id()
                })
                .collect::<Vec<_>>();
            assert_eq!(client_children.len(), 1);
        }

        let mut outcomes = Vec::new();
        for client in &client_spans {
            assert_eq!(client.span_kind, SpanKind::Client);
            assert!(grafana_spans.iter().any(|grafana| {
                client.parent_span_id == grafana.span_context.span_id()
                    && client.span_context.trace_id() == grafana.span_context.trace_id()
            }));
            assert!(
                client.attributes.iter().all(|attribute| matches!(
                    attribute.key.as_str(),
                    "code.file.path"
                        | "code.module.name"
                        | "code.line.number"
                        | "thread.id"
                        | "thread.name"
                        | "target"
                        | "http.request.method"
                        | "http.response.status_code"
                        | "http.outcome"
                        | "busy_ns"
                        | "idle_ns"
                )),
                "unexpected client attributes: {:?}",
                client.attributes
            );
            let attributes = client
                .attributes
                .iter()
                .map(|attribute| (attribute.key.as_str(), attribute.value.to_string()))
                .collect::<std::collections::HashMap<_, _>>();
            assert!(matches!(
                attributes["http.outcome"].as_str(),
                "success" | "http_error" | "transport_error" | "response_error" | "cancelled"
            ));
            let status = attributes.get("http.response.status_code").cloned();
            let otel_error = match &client.status {
                Status::Unset => false,
                Status::Error { description } => {
                    assert!(description.is_empty());
                    true
                }
                Status::Ok => panic!("client spans must not override success status"),
            };
            outcomes.push((
                attributes["http.request.method"].clone(),
                attributes["http.outcome"].clone(),
                status,
                otel_error,
            ));
        }
        outcomes.sort();
        assert_eq!(
            outcomes,
            vec![
                ("GET".to_owned(), "cancelled".to_owned(), None, true),
                (
                    "GET".to_owned(),
                    "http_error".to_owned(),
                    Some("503".to_owned()),
                    true
                ),
                (
                    "GET".to_owned(),
                    "response_error".to_owned(),
                    Some("200".to_owned()),
                    true
                ),
                (
                    "GET".to_owned(),
                    "success".to_owned(),
                    Some("200".to_owned()),
                    false
                ),
                ("GET".to_owned(), "transport_error".to_owned(), None, true),
                (
                    "GET".to_owned(),
                    "transport_error".to_owned(),
                    Some("200".to_owned()),
                    true
                ),
                (
                    "POST".to_owned(),
                    "success".to_owned(),
                    Some("200".to_owned()),
                    false
                ),
            ]
        );
        let serialized = format!("{spans:?}");
        for excluded in [
            "sensitive-path",
            "secret=query",
            "SensitiveRawErrorQuery",
            "SensitiveHttpErrorQuery",
            "SensitiveTimeoutQuery",
            "SensitiveTruncatedBodyQuery",
            "SensitiveOversizedBodyQuery",
            "SensitiveMatcher",
            "SensitiveComment",
            "grafana-secret",
            "test-token",
            "error.message",
            "error.cause_chain",
        ] {
            assert!(!serialized.contains(excluded), "leaked {excluded}");
        }
        assert_eq!(
            propagated
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            ["GET", "POST"]
        );
        for (method, traceparent) in propagated {
            let parts = traceparent.split('-').collect::<Vec<_>>();
            assert_eq!(parts.len(), 4, "invalid traceparent: {traceparent}");
            assert_eq!(parts[0], "00");
            assert_eq!(parts[1].len(), 32);
            assert_eq!(parts[2].len(), 16);
            assert_eq!(parts[3], "01");
            assert!(
                parts[1]
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            );
            assert!(
                parts[2]
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            );
            let client = client_spans
                .iter()
                .find(|span| {
                    span.span_context.span_id().to_string() == parts[2]
                        && span.attributes.iter().any(|attribute| {
                            attribute.key.as_str() == "http.request.method"
                                && attribute.value.to_string() == method
                        })
                })
                .unwrap();
            assert_eq!(client.span_context.trace_id().to_string(), parts[1]);
        }
    }

    #[tokio::test]
    async fn discovery_list_schema_filter_and_call_follow_skills_contract() {
        let (handler, _, grafana_task) = test_handler().await;
        let (origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let endpoint = format!("{origin}/mcp");

        let (_, discover) =
            post_mcp(&endpoint, request("server/discover", "discover", json!({}))).await;
        assert_eq!(
            discover["result"]["supportedVersions"],
            json!(["2026-07-28"])
        );
        assert_eq!(
            discover["result"]["capabilities"],
            json!({"tools":{"listChanged":false},"resources":{"listChanged":false,"subscribe":false},"extensions":{"io.modelcontextprotocol/skills":{}}})
        );
        assert_eq!(discover["result"]["cacheScope"], "private");
        assert_eq!(discover["result"]["ttlMs"], 0);

        let (_, listed) = post_mcp(&endpoint, request("tools/list", "list", json!({}))).await;
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);
        for tool in tools {
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert_eq!(tool["annotations"]["readOnlyHint"], tool["name"] == "query");
            for action in action_schemas(tool) {
                assert!(!action.to_string().contains("__"));
                let schema = &action["input_schema"];
                if let Some(branches) = schema["anyOf"].as_array() {
                    assert!(
                        branches
                            .iter()
                            .all(|branch| branch["additionalProperties"] == false)
                    );
                } else {
                    assert_eq!(schema["additionalProperties"], false);
                }
            }
        }
        let find_schema = |tool_name: &str, action_name: &str| {
            action_schemas(tools.iter().find(|tool| tool["name"] == tool_name).unwrap())
                .into_iter()
                .find(|action| action["action"] == action_name)
                .unwrap()["input_schema"]
                .clone()
        };
        let pod_logs = find_schema("query", "kubernetes.pod_logs");
        assert_eq!(
            pod_logs["properties"]["instance"]["enum"],
            json!(["current", "previous"])
        );
        assert_eq!(pod_logs["properties"]["tail_lines"]["maximum"], 1000);
        assert_eq!(pod_logs["properties"]["max_bytes"]["maximum"], 262144);
        assert_eq!(
            pod_logs["required"],
            json!([
                "cluster",
                "namespace",
                "pod",
                "pod_uid",
                "container",
                "instance"
            ])
        );
        let resources = find_schema("query", "kubernetes.resource_list");
        assert_eq!(resources["anyOf"].as_array().unwrap().len(), 2);
        assert!(resources["anyOf"][0]["properties"].get("labels").is_some());
        assert!(
            resources["anyOf"][1]["properties"]
                .get("namespace")
                .is_none()
        );
        for action in ["ceph.osd.list", "ceph.device.list", "ceph.task.list"] {
            let schema = find_schema("query", action);
            assert_eq!(schema["properties"]["limit"]["minimum"], 1);
            assert_eq!(schema["properties"]["limit"]["maximum"], 100);
        }
        for action in ["ceph.osd.destroy", "ceph.osd.purge"] {
            assert_eq!(
                find_schema("destroy", action)["required"],
                json!(["cluster", "osd_id", "confirmation"])
            );
        }
        let weight = find_schema("execute", "ceph.osd.reweight");
        assert_eq!(weight["properties"]["weight"]["minimum"], 0.0);
        assert_eq!(weight["properties"]["weight"]["maximum"], 1.0);
        for action in ["tekton.run.get", "tekton.run.status", "tekton.run.wait"] {
            assert_eq!(find_schema("query", action)["required"], json!(["run_id"]));
        }
        assert!(
            find_schema("query", "tekton.run.list")["properties"]
                .get("revision")
                .is_some()
        );
        assert_eq!(
            find_schema("create", "grafana.silence.create")["required"],
            json!(["matchers", "duration_seconds", "comment"])
        );

        let (_, listed) =
            post_mcp(&endpoint, request("resources/list", "resources", json!({}))).await;
        let catalogs = listed["result"]["resources"].as_array().unwrap();
        assert_eq!(catalogs.len(), 9);
        for uri in [
            "homelab://kubernetes/clusters",
            "homelab://ceph/clusters",
            "homelab://machines",
            "homelab://deploys",
        ] {
            assert!(catalogs.iter().any(|resource| resource["uri"] == uri));
            let (_, read) = post_mcp(
                &endpoint,
                request("resources/read", "catalog", json!({"uri":uri})),
            )
            .await;
            assert!(read["result"]["contents"][0]["text"].is_string(), "{read}");
        }
        let (_, templates) = post_mcp(
            &endpoint,
            request("resources/templates/list", "templates", json!({})),
        )
        .await;
        let templates = templates["result"]["resourceTemplates"].as_array().unwrap();
        assert_eq!(templates.len(), 11);
        assert!(templates.iter().all(|template| {
            template["description"]
                .as_str()
                .unwrap()
                .contains("Input schema:")
        }));
        for (uri, result_type, field, expected) in [
            (
                "homelab://grafana/alert-rules",
                "alert_rules",
                "title",
                "API errors",
            ),
            (
                "homelab://grafana/recording-rules",
                "recording_rules",
                "metric",
                "api_request_rate",
            ),
            (
                "homelab://grafana/dashboards",
                "dashboards",
                "title",
                "Overview",
            ),
        ] {
            let (_, read) = post_mcp(
                &endpoint,
                request("resources/read", "grafana-catalog", json!({"uri":uri})),
            )
            .await;
            let value: Value =
                serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(value["result_type"], result_type);
            assert_eq!(value["result"][0][field], expected);
        }
        let (_, dashboard) = post_mcp(
            &endpoint,
            request(
                "resources/read",
                "dashboard",
                json!({"uri":"homelab://grafana/dashboards/dash-1"}),
            ),
        )
        .await;
        let dashboard: Value =
            serde_json::from_str(dashboard["result"]["contents"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(
            dashboard["result"]["panels"][0],
            json!({"id":"13","title":"CPU","type":"timeseries"})
        );
        assert_eq!(
            dashboard["result"]["variables"][0],
            json!({"name":"cluster","label":"Cluster","type":"query"})
        );
        for omitted in ["targets", "expr", "secret", "url", "\"id\":1"] {
            assert!(!dashboard.to_string().contains(omitted));
        }
        let uri = format!(
            "homelab://grafana/dashboards?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("input", "{\"limit\":1,\"tags\":[\"prod\"]}")
                .append_pair("filter", ".result[0].title")
                .finish()
        );
        let (_, projected) = post_mcp(
            &endpoint,
            request("resources/read", "projected", json!({"uri":uri})),
        )
        .await;
        assert_eq!(projected["result"]["contents"][0]["uri"], uri);
        let value: Value =
            serde_json::from_str(projected["result"]["contents"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(value, json!({"result":"Overview"}));

        let (_, rendered) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "render-dashboard",
                json!({"name": "query","arguments":{"action":"grafana.render.dashboard","input":{"uid":"dash-1"}}}),
            ),
        )
        .await;
        assert_eq!(rendered["result"]["content"][1]["type"], "image");
        assert_eq!(rendered["result"]["content"][1]["mimeType"], "image/png");
        assert!(
            rendered["result"]["content"][1]["data"]
                .as_str()
                .unwrap()
                .ends_with('=')
        );
        assert_eq!(
            rendered["result"]["structuredContent"]["render_type"],
            "dashboard"
        );
        assert!(
            rendered["result"]["structuredContent"]["sha256"]
                .as_str()
                .unwrap()
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );

        let (_, filtered_render) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "filtered-render",
                json!({"name": "query","arguments":{"action":"grafana.render.dashboard","input":{"uid":"dash-1"},"filter":".sha256"}}),
            ),
        )
        .await;
        assert!(filtered_render["result"]["structuredContent"].is_string());
        assert_eq!(filtered_render["result"]["content"][1]["type"], "image");
        assert_eq!(
            filtered_render["result"]["content"][1]["mimeType"],
            rendered["result"]["content"][1]["mimeType"]
        );
        assert_eq!(
            filtered_render["result"]["content"][1]["data"],
            rendered["result"]["content"][1]["data"]
        );

        let (_, silence) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "create-silence",
                json!({
                    "name": "create",
                    "arguments":{
                        "action":"grafana.silence.create",
                        "input":{
                            "matchers":[{"name":"alertname","operator":"=","value":"APIError"}],
                            "duration_seconds":3600,
                            "comment":"maintenance"
                        }
                    }
                }),
            ),
        )
        .await;
        assert_eq!(silence["result"]["isError"], Value::Null, "{silence}");
        let structured = &silence["result"]["structuredContent"];
        assert_eq!(structured["silence_id"], "silence-123");
        assert!(structured["starts_at"].is_string());
        assert!(structured["ends_at"].is_string());
        assert_eq!(structured.as_object().unwrap().len(), 3);
        assert_eq!(
            silence["result"]["content"][0]["text"],
            "Created Grafana silence silence-123."
        );

        let (_, filtered) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "filtered-alerts",
                json!({
                    "name": "query",
                    "arguments":{"action":"grafana.alert-instance.list","input":{},"filter":".result[]"}
                }),
            ),
        )
        .await;
        let expected_filtered = json!({
            "annotations": {"summary": "API is failing"},
            "ends_at": "2026-08-10T13:00:00.000000000Z",
            "fingerprint": "abc123",
            "inhibited": false,
            "labels": {"alertname": "APIError"},
            "silenced": false,
            "starts_at": "2026-08-10T12:00:00.000000000Z",
            "state": "active",
            "updated_at": "2026-08-10T12:01:00.000000000Z"
        });
        assert_eq!(filtered["result"]["structuredContent"], expected_filtered);
        assert_eq!(
            filtered["result"]["content"][0]["text"],
            expected_filtered.to_string()
        );

        grafana_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn tekton_run_reads_dispatch_through_progressive_mcp_with_strict_schemas() {
        let repository_url = Arc::new(Mutex::new(String::new()));
        let upstream = Router::new()
            .fallback(
                |State(repository_url): State<Arc<Mutex<String>>>,
                 request: HttpRequest<Body>| async move {
                    match request.uri().path() {
                        "/apis/pipelinesascode.tekton.dev/v1alpha1/namespaces/pipelines-as-code/repositories" => Json(json!({
                            "items":[{
                                "metadata":{"name":"pac-rfhold-repo"},
                                "spec":{"url":repository_url.lock().unwrap().clone()}
                            }],
                            "metadata":{}
                        }))
                        .into_response(),
                        "/apis/tekton.dev/v1/namespaces/pipelines-as-code/pipelineruns/run" => Json(json!({
                            "metadata":{
                                "name":"run", "uid":"run-uid",
                                "labels":{
                                    "pipelinesascode.tekton.dev/repository":"pac-rfhold-repo",
                                    "pipelinesascode.tekton.dev/sha":"abc"
                                }
                            },
                            "status":{"conditions":[{
                                "type":"Succeeded", "status":"True", "reason":"Succeeded"
                            }]}
                        }))
                        .into_response(),
                        "/apis/tekton.dev/v1/namespaces/pipelines-as-code/taskruns" => Json(json!({
                            "items":[], "metadata":{}
                        }))
                        .into_response(),
                        _ => StatusCode::NOT_FOUND.into_response(),
                    }
                },
            )
            .with_state(Arc::clone(&repository_url));
        let (upstream_origin, upstream_task) = serve(upstream).await;
        *repository_url.lock().unwrap() = format!("{upstream_origin}/rfhold/repo.git");
        let origin = url::Url::parse(&format!("{upstream_origin}/")).unwrap();
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services {
                grafana: GrafanaClient::for_test(origin.clone(), std::time::Duration::from_secs(1)),
                tekton: crate::integrations::tekton::TektonClient::for_test(
                    origin,
                    std::time::Duration::from_secs(1),
                ),
                kubernetes: crate::integrations::kubernetes::KubernetesCatalog::inert_for_test(),
                ceph: crate::integrations::ceph::CephCatalog::disabled_for_test(),
                inventory: Arc::new(crate::services::InertInventory),
                deploys: Arc::new(crate::services::InertDeploys),
            }),
            catalog: Arc::new(skills::catalog().unwrap()),
            progress_heartbeat_interval: PROGRESS_HEARTBEAT_INTERVAL,
        });
        let (mcp_origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let (_, response) = post_mcp(
            &format!("{mcp_origin}/mcp"),
            request(
                "tools/call",
                "run-wait",
                json!({
                    "name": "query",
                    "arguments":{
                        "action":"tekton.run.wait",
                        "input":{"run_id":"pipelines-as-code/run","timeout_seconds":1}
                    }
                }),
            ),
        )
        .await;
        assert_eq!(response["result"]["isError"], Value::Null, "{response}");
        let result = &response["result"]["structuredContent"];
        assert_eq!(result["mode"], "wait");
        assert_eq!(result["result_type"], "run");
        assert_eq!(result["result"]["repository"], "rfhold/repo");
        assert_eq!(result["timed_out"], false);

        let (_, response) = post_mcp(
            &format!("{mcp_origin}/mcp"),
            request(
                "tools/call",
                "run-status",
                json!({
                    "name": "query",
                    "arguments":{
                        "action":"tekton.run.status",
                        "input":{"run_id":"pipelines-as-code/run"}
                    }
                }),
            ),
        )
        .await;
        assert_eq!(response["result"]["isError"], Value::Null, "{response}");
        let result = &response["result"]["structuredContent"];
        assert_eq!(result["mode"], "status");
        assert_eq!(result["result_type"], "run_status");
        assert_eq!(result["result"]["repository"], "rfhold/repo");
        assert_eq!(result["failed_tasks"], json!([]));

        upstream_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn malformed_shapes_actions_filters_and_semantic_failures_keep_error_boundary() {
        let (handler, _, grafana_task) = test_handler().await;
        let (origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let endpoint = format!("{origin}/mcp");

        for (tool, action) in [
            (QUERY_TOOL_NAME, "logql"),
            (QUERY_TOOL_NAME, "promql"),
            (QUERY_TOOL_NAME, "traceql"),
            (QUERY_TOOL_NAME, "profiles"),
            (QUERY_TOOL_NAME, "alert_rules"),
            (QUERY_TOOL_NAME, "alert_instances"),
            (QUERY_TOOL_NAME, "list_silences"),
            (CREATE_TOOL_NAME, "create_silence"),
        ] {
            let (_, response) = post_mcp(
                &endpoint,
                request(
                    "tools/call",
                    "legacy-action",
                    json!({"name":tool,"arguments":{"action":action}}),
                ),
            )
            .await;
            assert_eq!(response["error"]["code"], -32602, "{action}: {response}");
        }

        for (tool, arguments) in [
            (QUERY_TOOL_NAME, json!({"action":"unknown"})),
            (QUERY_TOOL_NAME, json!({"action":"silence.create"})),
            (CREATE_TOOL_NAME, json!({"action":"alert-rule.list"})),
            (CREATE_TOOL_NAME, json!({"action":"recording-rule.list"})),
            (CREATE_TOOL_NAME, json!({"action":"silence.list"})),
            (
                QUERY_TOOL_NAME,
                json!({"action":"alert-rule.list","input":{"limit":1,"extra":true}}),
            ),
            (
                QUERY_TOOL_NAME,
                json!({"action":"recording-rule.list","input":{"limit":1,"extra":true}}),
            ),
            (
                QUERY_TOOL_NAME,
                json!({
                    "action":"tekton.run.status",
                    "input":{"run_id":"pipelines-as-code/run","extra":true}
                }),
            ),
            (
                CREATE_TOOL_NAME,
                json!({
                    "action":"silence.create",
                    "input":{
                        "matchers":[], "duration_seconds":1, "comment":"x", "extra":true
                    }
                }),
            ),
            (
                QUERY_TOOL_NAME,
                json!({"action":"kubernetes.pod_delete","input":{"cluster":"test","namespace":"ns","name":"pod"}}),
            ),
            (
                EXECUTE_TOOL_NAME,
                json!({"action":"kubernetes.resource_get","input":{"cluster":"test","kind":"pod","namespace":"ns","name":"pod"}}),
            ),
            (
                QUERY_TOOL_NAME,
                json!({"action":"cluster_list","input":{"extra":true}}),
            ),
            (
                QUERY_TOOL_NAME,
                json!({"action":"ceph.osd.mark","input":{"cluster":"test-cluster","osd_id":1,"state":"out"}}),
            ),
            (
                EXECUTE_TOOL_NAME,
                json!({"action":"ceph.status.get","input":{"cluster":"test-cluster"}}),
            ),
            (
                EXECUTE_TOOL_NAME,
                json!({"action":"flags.set","input":{"cluster":"test-cluster","flag":"noout","state":"set"}}),
            ),
            (
                QUERY_TOOL_NAME,
                json!({"action":"cluster.list","input":{"extra":true}}),
            ),
            (
                QUERY_TOOL_NAME,
                json!({"action":"alert-rule.list","input":{},"extra":true}),
            ),
            (
                QUERY_TOOL_NAME,
                json!({"action":"alert-rule.list","input":{},"filter":".["}),
            ),
        ] {
            let (_, response) = post_mcp(
                &endpoint,
                request(
                    "tools/call",
                    "invalid",
                    json!({"name":tool,"arguments":arguments}),
                ),
            )
            .await;
            assert_eq!(response["error"]["code"], -32602, "{response}");
        }

        let (_, semantic) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "semantic",
                json!({"name": "query","arguments":{"action":"grafana.logql.query","input":{"query":" "},"filter":".error"}}),
            ),
        )
        .await;
        assert_eq!(semantic["result"]["isError"], true);
        assert_eq!(
            semantic["result"]["structuredContent"]["error"],
            json!({"code":"invalid_arguments","message":"The LogQL arguments are invalid.","retryable":false})
        );
        assert!(!semantic.to_string().contains("grafana-secret"));
        assert_eq!(
            semantic["result"]["content"][0]["text"],
            "The LogQL arguments are invalid."
        );

        let (_, recording_semantic) = post_mcp(
            &endpoint,
            request(
                "resources/read",
                "recording-semantic",
                json!({"uri":"homelab://grafana/recording-rules?input=%7B%22limit%22%3A0%7D"}),
            ),
        )
        .await;
        assert_eq!(recording_semantic["error"]["code"], -32602);
        assert!(
            recording_semantic["error"]["message"]
                .as_str()
                .unwrap()
                .contains("invalid_arguments")
        );

        let (_, mutation_semantic) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "mutation-semantic",
                json!({
                    "name": "create",
                    "arguments":{
                        "action":"grafana.silence.create",
                        "input":{"matchers":[],"duration_seconds":0,"comment":" "}
                    }
                }),
            ),
        )
        .await;
        assert_eq!(mutation_semantic["result"]["isError"], true);
        assert_eq!(
            mutation_semantic["result"]["structuredContent"]["error"],
            json!({"code":"invalid_arguments","message":"The silence arguments are invalid.","retryable":false})
        );

        let (_, kubernetes_semantic) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "kubernetes-semantic",
                json!({
                    "name": "execute",
                    "arguments":{
                        "action":"kubernetes.workload_scale",
                        "input":{
                            "cluster":"test", "kind":"deployment", "namespace":"ns",
                            "name":"app", "replicas":1001
                        }
                    }
                }),
            ),
        )
        .await;
        assert_eq!(kubernetes_semantic["result"]["isError"], true);
        assert_eq!(
            kubernetes_semantic["result"]["structuredContent"]["error"],
            json!({
                "code":"invalid_arguments",
                "message":"The Kubernetes workload arguments are invalid.",
                "retryable":false
            })
        );

        let (_, ceph_semantic) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "ceph-semantic",
                json!({
                    "name": "query",
                    "arguments":{"action":"ceph.status.get","input":{"cluster":"Bad.Name"}}
                }),
            ),
        )
        .await;
        assert_eq!(ceph_semantic["result"]["isError"], true);
        assert_eq!(
            ceph_semantic["result"]["structuredContent"]["error"],
            json!({
                "code":"invalid_arguments",
                "message":"The Ceph status arguments are invalid.",
                "retryable":false
            })
        );
        assert!(!ceph_semantic.to_string().contains("dashboard-password"));

        let (_, unknown_tool) = post_mcp(
            &endpoint,
            request(
                "tools/call",
                "unknown-tool",
                json!({"name":"other","arguments":{"action":"list","input":{}}}),
            ),
        )
        .await;
        // The 2026-07-28 transport rejects an unknown dynamic Mcp-Name before handler dispatch.
        assert_eq!(unknown_tool["error"]["code"], -32020);
        assert!(unknown_tool.get("result").is_none());

        grafana_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn uncertain_mutation_failure_is_exact_safe_and_non_retryable() {
        let grafana = Router::new().route(
            "/api/alertmanager/grafana/api/v2/silences",
            post(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unsafe upstream mutation detail",
                )
            }),
        );
        let (grafana_origin, grafana_task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services::new(GrafanaClient::for_test(
                url::Url::parse(&format!("{grafana_origin}/")).unwrap(),
                std::time::Duration::from_secs(1),
            ))),
            catalog: Arc::new(skills::catalog().unwrap()),
            progress_heartbeat_interval: PROGRESS_HEARTBEAT_INTERVAL,
        });
        let (origin, mcp_task) = serve(streamable_http_router(handler)).await;
        let (_, response) = post_mcp(
            &format!("{origin}/mcp"),
            request(
                "tools/call",
                "uncertain-mutation",
                json!({
                    "name": "create",
                    "arguments":{
                        "action":"grafana.silence.create",
                        "input":{
                            "matchers":[{"name":"alertname","operator":"=","value":"APIError"}],
                            "duration_seconds":3600,
                            "comment":"maintenance"
                        }
                    }
                }),
            ),
        )
        .await;

        let message = "The Grafana mutation did not complete cleanly; its outcome may be uncertain. Check existing silences before retrying.";
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["structuredContent"]["error"],
            json!({"code":"mutation_outcome_unknown","message":message,"retryable":false})
        );
        assert_eq!(response["result"]["content"][0]["text"], message);
        assert!(
            !response
                .to_string()
                .contains("unsafe upstream mutation detail")
        );

        grafana_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn cancellation_after_silence_post_is_exact_safe_and_non_retryable() {
        let post_received = Arc::new(Notify::new());
        let post_probe = Arc::clone(&post_received);
        let grafana = Router::new().route(
            "/api/alertmanager/grafana/api/v2/silences",
            post(move || {
                let post_probe = Arc::clone(&post_probe);
                async move {
                    post_probe.notify_one();
                    std::future::pending::<Json<Value>>().await
                }
            }),
        );
        let (grafana_origin, grafana_task) = serve(grafana).await;
        let handler = Arc::new(HomelabMcp {
            services: Arc::new(Services::new(GrafanaClient::for_test(
                url::Url::parse(&format!("{grafana_origin}/")).unwrap(),
                std::time::Duration::from_secs(1),
            ))),
            catalog: Arc::new(skills::catalog().unwrap()),
            progress_heartbeat_interval: PROGRESS_HEARTBEAT_INTERVAL,
        });
        let cancellation = Arc::new(Notify::new());
        let cancellation_signal = Arc::clone(&cancellation);
        let dispatch = tokio::spawn(async move {
            let command = CreateSilenceInput {
                matchers: vec![crate::integrations::grafana::actions::LabelMatcher {
                    name: "alertname".to_owned(),
                    operator: crate::integrations::grafana::actions::MatcherOperator::Equal,
                    value: "APIError".to_owned(),
                }],
                duration_seconds: 3600,
                comment: "maintenance".to_owned(),
            }
            .validate()
            .unwrap();
            handler
                .dispatch_create_silence(&command, cancellation_signal.notified())
                .await
        });

        post_received.notified().await;
        cancellation.notify_one();
        let response = dispatch.await.unwrap().raw;

        let message = "The Grafana mutation did not complete cleanly; its outcome may be uncertain. Check existing silences before retrying.";
        assert_eq!(response["isError"], true);
        assert_eq!(
            response["structuredContent"]["error"],
            json!({"code":"mutation_outcome_unknown","message":message,"retryable":false})
        );
        assert_eq!(response["content"][0]["text"], message);
        assert!(!response.to_string().contains("request cancelled"));

        grafana_task.abort();
    }

    #[test]
    fn semantic_errors_are_exact_and_safe() {
        let cases = [
            (
                GrafanaError::InvalidArguments,
                "invalid_arguments",
                "The LogQL arguments are invalid.",
                false,
            ),
            (
                GrafanaError::CapacityExhausted,
                "capacity_exhausted",
                "Grafana request capacity is currently exhausted.",
                true,
            ),
            (
                GrafanaError::Timeout,
                "timeout",
                "The Grafana query timed out.",
                true,
            ),
            (
                GrafanaError::Unauthorized,
                "grafana_unauthorized",
                "Grafana rejected the service credentials.",
                false,
            ),
            (
                GrafanaError::QueryRejected,
                "query_rejected",
                "Grafana rejected the LogQL query.",
                false,
            ),
            (
                GrafanaError::MutationRejected,
                "mutation_rejected",
                "Grafana rejected the requested mutation.",
                false,
            ),
            (
                GrafanaError::MutationOutcomeUnknown,
                "mutation_outcome_unknown",
                "The Grafana mutation did not complete cleanly; its outcome may be uncertain. Check existing silences before retrying.",
                false,
            ),
            (
                GrafanaError::UpstreamUnavailable,
                "upstream_unavailable",
                "Grafana is currently unavailable.",
                true,
            ),
            (
                GrafanaError::InvalidResponse,
                "invalid_response",
                "Grafana returned an invalid response.",
                false,
            ),
            (
                GrafanaError::RenderTimeout,
                "render_timeout",
                "The Grafana render timed out.",
                true,
            ),
            (
                GrafanaError::RenderRejected,
                "render_rejected",
                "Grafana rejected the render request.",
                false,
            ),
            (
                GrafanaError::RenderInvalidResponse,
                "render_invalid_response",
                "Grafana returned an invalid render response.",
                false,
            ),
        ];
        for (error, code, message, retryable) in cases {
            let result = tool_error("LogQL", error).raw;
            assert_eq!(result["isError"], true);
            assert_eq!(
                result["structuredContent"]["error"],
                json!({"code":code,"message":message,"retryable":retryable})
            );
            assert_eq!(result["content"][0]["text"], message);
            assert!(!result.to_string().contains("secret"));
        }
    }

    #[test]
    fn pinned_mcp_parses_render_image_content_without_exposing_bytes_in_debug() {
        let request = RenderDashboardInput {
            uid: "dash-1".to_owned(),
            from: None,
            to: None,
            width: None,
            height: None,
            scale: None,
            theme: None,
            timezone: None,
            variables: None,
        }
        .validate()
        .unwrap();
        let result = render_result(
            "dashboard",
            &request.uid,
            None,
            &request.options,
            RenderedImage {
                bytes: b"\x89PNG\r\n\x1a\nbody".to_vec(),
                decoded_bytes: 12,
                sha256: "0".repeat(64),
            },
        );
        let parsed = result.content().unwrap();
        assert!(matches!(
            parsed.content[1],
            mcp::McpToolContent::Image {
                mime_type: "image/png",
                ..
            }
        ));
        assert!(!format!("{parsed:?}").contains("iVBOR"));
    }

    #[tokio::test]
    async fn hosted_authorization_advertises_all_required_scopes_and_denies_bad_origins() {
        let (handler, _, grafana_task) = test_handler().await;
        let metadata = McpProtectedResourceMetadata::new(
            "http://127.0.0.1/mcp",
            ["https://auth.example.com/oauth"],
        )
        .with_scopes([
            "mcp:use",
            "kubernetes:read",
            "kubernetes:write",
            "inventory:read",
            "inventory:write",
            "inventory:host-trust",
            "deploy:read",
            "deploy:run",
        ]);
        let authorization = StreamableHttpAuthorization::hosted(metadata, |_, _| {
            Box::pin(async { McpHostedTokenValidation::Unavailable })
        })
        .unwrap()
        .with_required_scopes([
            "mcp:use",
            "kubernetes:read",
            "kubernetes:write",
            "inventory:read",
            "inventory:write",
            "inventory:host-trust",
            "deploy:read",
            "deploy:run",
        ]);
        let router = streamable_http_router_with_options(
            handler,
            StreamableHttpOptions::default().with_authorization(authorization),
        );
        let (origin, mcp_task) = serve(router).await;
        let endpoint = format!("{origin}/mcp");
        let body = request("server/discover", "auth", json!({}));
        let client = Client::new();
        let missing = client
            .post(&endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("mcp-method", "server/discover")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
        let challenge = missing.headers()["www-authenticate"].to_str().unwrap();
        assert!(challenge.starts_with("Bearer "));
        for scope in [
            "mcp:use",
            "kubernetes:read",
            "kubernetes:write",
            "inventory:read",
            "inventory:write",
            "inventory:host-trust",
            "deploy:read",
            "deploy:run",
        ] {
            assert!(
                challenge.contains(scope),
                "challenge omitted {scope}: {challenge}"
            );
        }

        let unavailable = client
            .post(&endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("mcp-method", "server/discover")
            .header("authorization", "Bearer local-token")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);

        let forbidden_origin = client
            .post(&endpoint)
            .header("origin", "https://attacker.example")
            .send()
            .await
            .unwrap();
        assert_eq!(forbidden_origin.status(), StatusCode::FORBIDDEN);

        grafana_task.abort();
        mcp_task.abort();
    }

    #[tokio::test]
    async fn deploy_cancellation_preserves_outcome_unknown_error() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut services = Services::new(GrafanaClient::for_test(
            url::Url::parse("http://127.0.0.1/").unwrap(),
            std::time::Duration::from_secs(1),
        ));
        services.deploys = Arc::new(CancellingDeploys(cancelled.clone()));
        let handler = HomelabMcp {
            services: Arc::new(services),
            catalog: Arc::new(skills::catalog().unwrap()),
            progress_heartbeat_interval: PROGRESS_HEARTBEAT_INTERVAL,
        };
        let now = chrono::Utc::now();
        let machine = Machine {
            id: Uuid::new_v4(),
            display_name: "test".to_owned(),
            ssh_host: "test.example".to_owned(),
            ssh_port: 22,
            ssh_username: "homelab".to_owned(),
            pinned_host_public_key: Some("ssh-ed25519 public".to_owned()),
            created_at: now,
            updated_at: now,
        };

        let result = handler
            .dispatch_deploy_run("system-info", machine, async {})
            .await
            .raw;

        assert!(cancelled.load(Ordering::Relaxed));
        assert_eq!(
            result["structuredContent"]["error"]["code"],
            "cancelled_outcome_unknown"
        );
        assert_eq!(result["structuredContent"]["error"]["retryable"], true);
        assert_eq!(result["isError"], true);
    }
}
