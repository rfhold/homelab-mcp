use std::{sync::OnceLock, time::Instant};

use opentelemetry::{KeyValue, global};
use serde_json::Value;

use super::Error;

struct GrafanaMetrics {
    requests: opentelemetry::metrics::Counter<u64>,
    duration: opentelemetry::metrics::Histogram<f64>,
    in_flight: opentelemetry::metrics::UpDownCounter<i64>,
}

fn grafana_metrics() -> &'static GrafanaMetrics {
    static METRICS: OnceLock<GrafanaMetrics> = OnceLock::new();
    METRICS.get_or_init(|| {
        let meter = global::meter("homelab_mcp.grafana");
        GrafanaMetrics {
            requests: meter
                .u64_counter("homelab_mcp.grafana.upstream.requests")
                .with_description("Completed Grafana upstream request attempts")
                .build(),
            duration: meter
                .f64_histogram("homelab_mcp.grafana.upstream.duration")
                .with_unit("s")
                .with_description("Grafana upstream request attempt duration")
                .build(),
            in_flight: meter
                .i64_up_down_counter("homelab_mcp.grafana.upstream.in_flight")
                .with_description("Active Grafana upstream request attempts")
                .build(),
        }
    })
}

pub(super) struct GrafanaMetricsGuard {
    pub(super) action: &'static str,
    pub(super) mode: &'static str,
    pub(super) datasource_uid: &'static str,
    started: Instant,
    finished: bool,
}

impl GrafanaMetricsGuard {
    pub(super) fn new(
        action: &'static str,
        mode: &'static str,
        datasource_uid: &'static str,
    ) -> Self {
        let guard = Self {
            action: metric_action(action),
            mode: metric_mode(mode),
            datasource_uid: metric_datasource_uid(datasource_uid),
            started: Instant::now(),
            finished: false,
        };
        grafana_metrics().in_flight.add(1, &guard.base_attributes());
        guard
    }

    fn base_attributes(&self) -> [KeyValue; 3] {
        [
            KeyValue::new("action", self.action),
            KeyValue::new("mode", self.mode),
            KeyValue::new("datasource_uid", self.datasource_uid),
        ]
    }

    pub(super) fn finish(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }
        let base_attributes = self.base_attributes();
        let mut completed_attributes = base_attributes.to_vec();
        completed_attributes.push(KeyValue::new("outcome", metric_outcome(outcome)));
        let metrics = grafana_metrics();
        metrics.in_flight.add(-1, &base_attributes);
        metrics.requests.add(1, &completed_attributes);
        metrics
            .duration
            .record(self.started.elapsed().as_secs_f64(), &completed_attributes);
        self.finished = true;
    }
}

impl Drop for GrafanaMetricsGuard {
    fn drop(&mut self) {
        self.finish("cancelled");
    }
}

fn metric_action(action: &'static str) -> &'static str {
    match action {
        "logql.query" => "logql.query",
        "promql.query" => "promql.query",
        "traceql.search" => "traceql.search",
        "profile.merge" => "profile.merge",
        "alert-rule.list" => "alert-rule.list",
        "alert-instance.list" => "alert-instance.list",
        "silence.list" => "silence.list",
        "silence.create" => "silence.create",
        _ => "unknown",
    }
}

fn metric_mode(mode: &'static str) -> &'static str {
    match mode {
        "instant" => "instant",
        "range" => "range",
        "search" => "search",
        "list" => "list",
        "create" => "create",
        _ => "unknown",
    }
}

fn metric_datasource_uid(datasource_uid: &'static str) -> &'static str {
    match datasource_uid {
        "loki" => "loki",
        "mimir" => "mimir",
        "tempo" => "tempo",
        "pyroscope" => "pyroscope",
        "grafana_alerting" => "grafana_alerting",
        _ => "unknown",
    }
}

fn metric_outcome(outcome: &'static str) -> &'static str {
    match outcome {
        "success" => "success",
        "invalid_arguments" => "invalid_arguments",
        "capacity_exhausted" => "capacity_exhausted",
        "timeout" => "timeout",
        "unauthorized" => "unauthorized",
        "query_rejected" => "query_rejected",
        "mutation_rejected" => "mutation_rejected",
        "mutation_outcome_unknown" => "mutation_outcome_unknown",
        "upstream_unavailable" => "upstream_unavailable",
        "invalid_response" => "invalid_response",
        "cancelled" => "cancelled",
        _ => "upstream_unavailable",
    }
}

pub(super) fn request_outcome(result: &Result<Value, Error>) -> &'static str {
    match result {
        Ok(_) => "success",
        Err(Error::InvalidArguments) => "invalid_arguments",
        Err(Error::CapacityExhausted) => "capacity_exhausted",
        Err(Error::Timeout) => "timeout",
        Err(Error::Unauthorized) => "unauthorized",
        Err(Error::QueryRejected) => "query_rejected",
        Err(Error::MutationRejected) => "mutation_rejected",
        Err(Error::MutationOutcomeUnknown) => "mutation_outcome_unknown",
        Err(Error::UpstreamUnavailable) => "upstream_unavailable",
        Err(Error::InvalidResponse) => "invalid_response",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_labels_are_allowlisted() {
        let mut guard =
            GrafanaMetricsGuard::new("attacker-action", "attacker-mode", "attacker-uid");
        assert_eq!(guard.action, "unknown");
        assert_eq!(guard.mode, "unknown");
        assert_eq!(guard.datasource_uid, "unknown");
        guard.finish("attacker-outcome");

        assert_eq!(metric_action("alert-rule.list"), "alert-rule.list");
        assert_eq!(metric_action("alert-instance.list"), "alert-instance.list");
        assert_eq!(metric_action("silence.list"), "silence.list");
        assert_eq!(metric_action("silence.create"), "silence.create");
        assert_eq!(metric_action("alert_rules"), "unknown");
        assert_eq!(metric_action("alert_instances"), "unknown");
        assert_eq!(metric_action("create_silence"), "unknown");
        assert_eq!(metric_mode("list"), "list");
        assert_eq!(metric_mode("create"), "create");
        assert_eq!(
            metric_datasource_uid("grafana_alerting"),
            "grafana_alerting"
        );
        assert_eq!(metric_outcome("mutation_rejected"), "mutation_rejected");
        assert_eq!(
            metric_outcome("mutation_outcome_unknown"),
            "mutation_outcome_unknown"
        );
    }
}
