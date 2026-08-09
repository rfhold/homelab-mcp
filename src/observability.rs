use std::{collections::BTreeMap, env, fmt, io, time::Duration};

use chrono::{SecondsFormat, Utc};
use opentelemetry::{
    KeyValue, global,
    trace::{TraceContextExt as _, TracerProvider as _},
};
use opentelemetry_sdk::{
    Resource,
    metrics::SdkMeterProvider,
    propagation::TraceContextPropagator,
    trace::{Sampler, SdkTracerProvider},
};
use serde_json::Value;
use tracing::{Event, Level, Metadata, Subscriber, field::Visit};
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use tracing_subscriber::{
    EnvFilter, Layer as _,
    filter::{FilterExt as _, filter_fn},
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
};

use crate::config::TelemetryConfig;

pub const SERVICE_NAME: &str = "homelab-mcp";
pub const SERVICE_NAMESPACE: &str = "homelab";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct ObservabilityGuard {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
}

impl ObservabilityGuard {
    pub fn shutdown(mut self) {
        tracing::info!("observability shutdown started");
        if let Some(meter) = self.meter.take()
            && meter.shutdown_with_timeout(SHUTDOWN_TIMEOUT).is_err()
        {
            tracing::warn!("failed to shut down meter provider");
        }
        if let Some(tracer) = self.tracer.take() {
            tracing::info!("observability shutdown complete");
            if tracer.shutdown_with_timeout(SHUTDOWN_TIMEOUT).is_err() {
                tracing::warn!("failed to shut down tracer provider");
            }
        } else {
            tracing::info!("observability shutdown complete");
        }
    }
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        if let Some(meter) = self.meter.take() {
            let _ = meter.shutdown_with_timeout(SHUTDOWN_TIMEOUT);
        }
        if let Some(tracer) = self.tracer.take() {
            let _ = tracer.shutdown_with_timeout(SHUTDOWN_TIMEOUT);
        }
    }
}

pub fn init(
    config: &TelemetryConfig,
) -> Result<ObservabilityGuard, Box<dyn std::error::Error + Send + Sync>> {
    let export_mode = otlp_export_mode().map_err(io::Error::other)?;
    global::set_text_map_propagator(TraceContextPropagator::new());

    if export_mode == OtlpExportMode::Local {
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(JsonEventFormatter)
                    .with_filter(json_filter()),
            )
            .try_init()?;
        tracing::info!(
            service.name = SERVICE_NAME,
            service.namespace = SERVICE_NAMESPACE,
            deployment.environment.name = %config.deployment_environment,
            otlp.enabled = false,
            "observability initialized"
        );
        return Ok(ObservabilityGuard {
            tracer: None,
            meter: None,
        });
    }

    let resource = resource(config);
    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .map_err(|_| io::Error::other("failed to initialize OTLP trace exporter"))?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::AlwaysOn)
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();
    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .build()
        .map_err(|_| io::Error::other("failed to initialize OTLP metric exporter"))?;
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();

    global::set_tracer_provider(tracer_provider.clone());
    global::set_meter_provider(meter_provider.clone());
    let tracer = tracer_provider.tracer(SERVICE_NAME);
    tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(
            tracing_subscriber::fmt::layer()
                .event_format(JsonEventFormatter)
                .with_filter(json_filter()),
        )
        .with(trace_filter())
        .try_init()?;

    tracing::info!(
        service.name = SERVICE_NAME,
        service.namespace = SERVICE_NAMESPACE,
        deployment.environment.name = %config.deployment_environment,
        otlp.enabled = true,
        otel.traces.sampler = "always_on",
        "observability initialized"
    );
    Ok(ObservabilityGuard {
        tracer: Some(tracer_provider),
        meter: Some(meter_provider),
    })
}

fn resource(config: &TelemetryConfig) -> Resource {
    let mut attributes = vec![
        KeyValue::new("service.name", SERVICE_NAME),
        KeyValue::new("service.namespace", SERVICE_NAMESPACE),
        KeyValue::new(
            "deployment.environment.name",
            config.deployment_environment.clone(),
        ),
    ];
    for (key, value) in [
        ("k8s.namespace.name", &config.k8s_namespace),
        ("k8s.pod.name", &config.k8s_pod_name),
        ("k8s.pod.uid", &config.k8s_pod_uid),
        ("service.instance.id", &config.k8s_pod_uid),
    ] {
        if let Some(value) = value {
            attributes.push(KeyValue::new(key, value.clone()));
        }
    }
    Resource::builder().with_attributes(attributes).build()
}

fn configured_env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("homelab_mcp=info,mcp=info"))
}

fn allowed_target(target: &str) -> bool {
    target == "homelab_mcp"
        || target.starts_with("homelab_mcp::")
        || target == "mcp"
        || target.starts_with("mcp::")
}

fn trace_metadata_allowed(metadata: &Metadata<'_>) -> bool {
    allowed_target(metadata.target())
        && matches!(*metadata.level(), Level::ERROR | Level::WARN | Level::INFO)
}

fn json_filter<S>() -> impl tracing_subscriber::layer::Filter<S>
where
    S: Subscriber,
{
    filter_fn(|metadata| allowed_target(metadata.target())).and(configured_env_filter())
}

fn trace_filter<S>() -> impl tracing_subscriber::Layer<S>
where
    S: Subscriber,
{
    filter_fn(trace_metadata_allowed)
}

#[cfg(test)]
pub(crate) fn test_json_filter<S>() -> impl tracing_subscriber::layer::Filter<S>
where
    S: Subscriber,
{
    json_filter()
}

#[cfg(test)]
pub(crate) fn test_trace_filter<S>() -> impl tracing_subscriber::Layer<S>
where
    S: Subscriber,
{
    trace_filter()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OtlpExportMode {
    Local,
    Export,
}

fn otlp_export_mode() -> Result<OtlpExportMode, &'static str> {
    otlp_export_mode_with(|name| env::var_os(name).is_some())
}

fn otlp_export_mode_with(
    mut is_set: impl FnMut(&str) -> bool,
) -> Result<OtlpExportMode, &'static str> {
    if is_set("OTEL_EXPORTER_OTLP_ENDPOINT") {
        return Ok(OtlpExportMode::Export);
    }
    match (
        is_set("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"),
        is_set("OTEL_EXPORTER_OTLP_METRICS_ENDPOINT"),
    ) {
        (false, false) => Ok(OtlpExportMode::Local),
        (true, true) => Ok(OtlpExportMode::Export),
        _ => Err("invalid OTLP endpoint configuration"),
    }
}

#[derive(Clone, Copy, Debug)]
struct JsonEventFormatter;

impl<S, N> FormatEvent<S, N> for JsonEventFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        let mut fields = JsonFields::default();
        event.record(&mut fields);
        let mut object = serde_json::Map::new();
        object.insert(
            "timestamp".to_owned(),
            Value::String(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
        );
        object.insert(
            "level".to_owned(),
            Value::String(metadata.level().to_string()),
        );
        object.insert(
            "target".to_owned(),
            Value::String(metadata.target().to_owned()),
        );
        for (key, value) in fields.0 {
            object.insert(key, value);
        }
        insert_trace_correlation(&mut object);
        let line = serde_json::to_string(&object).map_err(|_| fmt::Error)?;
        writeln!(writer, "{line}")
    }
}

fn insert_trace_correlation(object: &mut serde_json::Map<String, Value>) {
    if let Some((trace_id, span_id)) = active_trace_ids() {
        object.insert("trace_id".to_owned(), Value::String(trace_id));
        object.insert("span_id".to_owned(), Value::String(span_id));
    }
}

fn active_trace_ids() -> Option<(String, String)> {
    let context = tracing::Span::current().context();
    let span = context.span();
    let span_context = span.span_context();
    span_context.is_valid().then(|| {
        (
            span_context.trace_id().to_string(),
            span_context.span_id().to_string(),
        )
    })
}

#[derive(Default)]
struct JsonFields(BTreeMap<String, Value>);

impl Visit for JsonFields {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().to_owned(), Value::Bool(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.0.insert(
            field.name().to_owned(),
            serde_json::Number::from_f64(value)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0
            .insert(field.name().to_owned(), Value::String(value.to_owned()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.0
            .insert(field.name().to_owned(), Value::String(format!("{value:?}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use std::sync::{Arc, Mutex};

    #[test]
    fn export_mode_requires_shared_or_both_signal_endpoints() {
        let mode = |set: &[&str]| otlp_export_mode_with(|name| set.contains(&name));
        assert_eq!(mode(&[]), Ok(OtlpExportMode::Local));
        assert_eq!(
            mode(&["OTEL_EXPORTER_OTLP_ENDPOINT"]),
            Ok(OtlpExportMode::Export)
        );
        assert_eq!(
            mode(&[
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
                "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT"
            ]),
            Ok(OtlpExportMode::Export)
        );
        for endpoint in [
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
        ] {
            assert_eq!(
                mode(&[endpoint]),
                Err("invalid OTLP endpoint configuration")
            );
        }
        assert_eq!(
            mode(&[
                "OTEL_EXPORTER_OTLP_ENDPOINT",
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"
            ]),
            Ok(OtlpExportMode::Export)
        );
    }

    #[test]
    fn target_allowlist_cannot_be_bypassed_by_permissive_levels() {
        for target in ["homelab_mcp", "homelab_mcp::app", "mcp", "mcp::server"] {
            assert!(allowed_target(target));
        }
        for target in [
            "reqwest",
            "hyper::client",
            "tower_http",
            "opentelemetry_otlp",
            "mcp_evil",
            "homelab_mcp_dependency",
        ] {
            assert!(!allowed_target(target));
        }

        let captured = Arc::new(Mutex::new(Vec::new()));
        let filter =
            filter_fn(|metadata| allowed_target(metadata.target())).and(EnvFilter::new("trace"));
        let subscriber = tracing_subscriber::registry()
            .with(TargetCapture(captured.clone()).with_filter(filter));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "reqwest", "unsafe dependency event");
            tracing::info!(target: "homelab_mcp::app", "owned application event");
            tracing::info!(target: "mcp::server", "owned MCP event");
            tracing::info!(target: "opentelemetry_otlp", "unsafe exporter event");
        });
        assert_eq!(
            *captured.lock().unwrap(),
            ["homelab_mcp::app", "mcp::server"]
        );
    }

    #[test]
    fn trace_filter_allows_only_approved_targets_through_info() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry()
            .with(TargetCapture(captured.clone()))
            .with(trace_filter());
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(target: "homelab_mcp", "owned error");
            tracing::warn!(target: "homelab_mcp::app", "owned warning");
            tracing::info!(target: "mcp", "owned MCP info");
            tracing::info!(target: "mcp::server", "owned MCP server info");
            tracing::debug!(target: "mcp::server", "excluded MCP debug");
            tracing::trace!(target: "homelab_mcp::app", "excluded application trace");
            tracing::error!(target: "reqwest", "excluded dependency error");
            tracing::info!(target: "mcp_evil", "excluded lookalike target");
        });
        assert_eq!(
            *captured.lock().unwrap(),
            ["homelab_mcp", "homelab_mcp::app", "mcp", "mcp::server"]
        );
    }

    struct TargetCapture(Arc<Mutex<Vec<&'static str>>>);

    impl<S: Subscriber> tracing_subscriber::Layer<S> for TargetCapture {
        fn on_event(&self, event: &Event<'_>, _context: tracing_subscriber::layer::Context<'_, S>) {
            self.0.lock().unwrap().push(event.metadata().target());
        }
    }

    #[test]
    fn resource_contains_approved_service_and_kubernetes_metadata() {
        let config = TelemetryConfig {
            deployment_environment: "preview".to_owned(),
            k8s_namespace: Some("homelab".to_owned()),
            k8s_pod_name: Some("homelab-mcp-abc".to_owned()),
            k8s_pod_uid: Some("pod-uid".to_owned()),
            pyroscope_url: None,
        };
        let resource = resource(&config);
        for (key, expected) in [
            ("service.name", SERVICE_NAME),
            ("service.namespace", SERVICE_NAMESPACE),
            ("deployment.environment.name", "preview"),
            ("k8s.namespace.name", "homelab"),
            ("k8s.pod.name", "homelab-mcp-abc"),
            ("k8s.pod.uid", "pod-uid"),
            ("service.instance.id", "pod-uid"),
        ] {
            assert_eq!(
                resource
                    .get(&opentelemetry::Key::new(key))
                    .unwrap()
                    .to_string(),
                expected
            );
        }
    }

    #[test]
    fn active_span_ids_are_json_ready() {
        let provider = SdkTracerProvider::builder()
            .with_sampler(Sampler::AlwaysOn)
            .build();
        let tracer = provider.tracer("test");
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("correlation");
            let _entered = span.enter();
            let mut object = serde_json::Map::new();
            insert_trace_correlation(&mut object);
            let json = serde_json::to_string(&object).unwrap();
            let trace_id = object["trace_id"].as_str().unwrap();
            let span_id = object["span_id"].as_str().unwrap();

            assert_eq!(json.matches(trace_id).count(), 1);
            assert_eq!(json.matches(span_id).count(), 1);
        });
        let _ = provider.shutdown();
    }
}
