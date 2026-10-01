use super::*;
use mcp::progressive::{
    ActionInputSchema, action_tool_result, deserialize_action_input, input_schema, json_schema_for,
    parse_arguments, tool_definition, unknown_action,
};
use mcp::server::BoxFuture;
use mcp::{McpToolCall, McpToolList};

// The pinned progressive macro restricts action namespaces; use its public
// schema/dispatch primitives to retain fully qualified Homelab action names.
macro_rules! catalog {
    ($($tool:literal, $description:literal => [$(($action:literal, $method:ident, $input:ty)),* $(,)?]);* $(;)?) => {
        pub(super) fn list() -> McpToolList {
            McpToolList { tools: vec![$({
                let mut definition = tool_definition($tool, $description, input_schema(
                    &[$($action),*], vec![$(ActionInputSchema { action: $action, schema: Some(json_schema_for::<$input>()) }),*]));
                definition.annotations = Some(json!({"readOnlyHint":$tool == "query", "destructiveHint": matches!($tool, "execute" | "destroy"), "idempotentHint":$tool == "query", "openWorldHint":true}));
                definition
            }),*], next_cursor: None }
        }

        pub(super) fn call(handler: Arc<HomelabMcp>, call: McpToolCall, context: ServerContext) -> BoxFuture<ServerResult<McpToolResult>> {
            Box::pin(async move {
                let request = parse_arguments(&call.name, call.arguments)?;
                let output = match (call.name.as_str(), request.action.as_str()) {
                    $($(($tool, $action) => handler.$method(deserialize_action_input::<$input>($action, request.input)?, context).await?,)*)*
                    _ => return Err(unknown_action(&request.action)),
                };
                action_tool_result(output, request.filter.as_deref())
            })
        }
    }
}

catalog! {
    "query", "Query bounded live state, logs, runs, observability signals and rendered images." => [
        ("grafana.logql.query", logql, LogqlInput),
        ("grafana.promql.query", promql, PromqlInput),
        ("grafana.traceql.search", traceql, TraceqlInput),
        ("grafana.profile.merge", profiles, ProfilesInput),
        ("grafana.alert-instance.list", alert_instances, AlertInstancesInput),
        ("grafana.silence.list", list_silences, ListSilencesInput),
        ("grafana.render.dashboard", render_dashboard, RenderDashboardInput),
        ("grafana.render.panel", render_panel, RenderPanelInput),
        ("tekton.run.list", tekton_runs, RunListInput),
        ("tekton.run.get", tekton_run, RunGetInput),
        ("tekton.run.status", tekton_run_status, RunStatusInput),
        ("tekton.run.wait", tekton_run_wait, RunWaitInput),
        ("tekton.task.list", tekton_tasks, TaskListInput),
        ("tekton.task.logs", tekton_task_logs, TaskLogsInput),
        ("kubernetes.resource_list", kubernetes_resources, ResourceListInput),
        ("kubernetes.resource_get", kubernetes_resource, ResourceGetInput),
        ("kubernetes.pod_logs", kubernetes_pod_logs, PodLogsInput),
        ("ceph.status.get", ceph_status, CephStatusGetInput),
        ("ceph.metrics.summary", ceph_metrics, CephMetricsSummaryInput),
        ("ceph.osd.list", ceph_osds, CephOsdListInput),
        ("ceph.osd.get", ceph_osd, CephOsdGetInput),
        ("ceph.osd.safe-to-destroy", ceph_osd_safe_to_destroy, CephOsdSafeToDestroyInput),
        ("ceph.device.list", ceph_devices, CephDeviceListInput),
        ("ceph.device.get", ceph_device, CephDeviceGetInput),
        ("ceph.flags.get", ceph_flags, CephFlagsGetInput),
        ("ceph.task.list", ceph_tasks, CephTaskListInput),
    ];
    "create", "Create machine inventory, Grafana silences or Jobs from exact CronJobs." => [
        ("grafana.silence.create", create_silence, CreateSilenceInput),
        ("kubernetes.cronjob_trigger", kubernetes_cronjob_trigger, CronjobTriggerInput),
        ("machine.create", machine_create, MachineCreateInput),
    ];
    "execute", "Execute approved exact-target operationally consequential changes." => [
        ("tekton.workflow.dispatch", tekton_dispatch, WorkflowDispatchInput),
        ("tekton.run.rerun", tekton_rerun, RunRerunInput),
        ("tekton.run.cancel", tekton_cancel, RunCancelInput),
        ("kubernetes.workload_restart", kubernetes_workload_restart, WorkloadRestartInput),
        ("kubernetes.workload_scale", kubernetes_workload_scale, WorkloadScaleInput),
        ("kubernetes.cronjob_suspend", kubernetes_cronjob_suspend, CronjobSuspendInput),
        ("ceph.osd.mark", ceph_mark_osd, CephOsdMarkInput),
        ("ceph.osd.reweight", ceph_reweight_osd, CephOsdReweightInput),
        ("ceph.osd.scrub", ceph_scrub_osd, CephOsdScrubInput),
        ("machine.update", machine_update, MachineUpdateInput),
        ("machine.host-key.clear", machine_host_key_clear, MachineIdInput),
        ("machine.host-key.replace", machine_host_key_replace, MachineHostKeyInput),
        ("deploy.run", deploy_run, DeployRunInput),
    ];
    "destroy", "Delete exact inventory or Pods, or destroy/purge explicitly confirmed safe OSDs." => [
        ("kubernetes.pod_delete", kubernetes_pod_delete, PodDeleteInput),
        ("ceph.osd.destroy", ceph_destroy_osd, CephOsdDestroyInput),
        ("ceph.osd.purge", ceph_purge_osd, CephOsdPurgeInput),
        ("machine.delete", machine_delete, MachineIdInput),
    ];
}
