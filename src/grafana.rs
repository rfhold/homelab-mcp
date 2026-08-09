use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, SecondsFormat, Utc};
use opentelemetry::{KeyValue, global, trace::TraceContextExt as _};
use reqwest::{Client, Method, Response, StatusCode, Url, redirect::Policy};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::Semaphore;
use tracing::{Instrument as _, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use crate::{
    config::Secret,
    logql::{Mode, Query},
};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_URL_BYTES: usize = 8192;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RANGE: chrono::Duration = chrono::Duration::hours(24);
const MAX_PROFILE_RANGE: chrono::Duration = chrono::Duration::hours(1);

pub const DEFAULT_TRACE_LIMIT: u16 = 20;
pub const MAX_TRACE_LIMIT: u16 = 100;
pub const DEFAULT_PROFILE_TYPE: &str = "process_cpu:cpu:nanoseconds:cpu:nanoseconds";
pub const DEFAULT_MAX_NODES: u16 = 256;
pub const MAX_MAX_NODES: u16 = 1000;
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
    query: String,
    mode: Mode,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    step: Option<String>,
    time: Option<DateTime<Utc>>,
}

impl PromqlInput {
    pub fn validate(self) -> Result<PromqlQuery, ()> {
        if self.query.trim().is_empty() {
            return Err(());
        }
        let start = parse_timestamp(self.start)?;
        let end = parse_timestamp(self.end)?;
        let time = parse_timestamp(self.time)?;
        match (start, end, self.step) {
            (Some(start), Some(end), Some(step)) => {
                let step_nanos = prometheus_duration_nanos(&step).ok_or(())?;
                let range = valid_range(start, end, MAX_RANGE)?;
                let range_nanos =
                    u64::try_from(range.num_nanoseconds().ok_or(())?).map_err(|_| ())?;
                if range_nanos / step_nanos + 1 > MAX_PROMQL_POINTS || time.is_some() {
                    return Err(());
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
            _ => Err(()),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceqlInput {
    /// TraceQL query to execute.
    pub query: String,
    /// Inclusive range start as an RFC3339 timestamp.
    pub start: Option<String>,
    /// Inclusive range end as an RFC3339 timestamp.
    pub end: Option<String>,
    /// Maximum returned traces, from 1 through 100.
    pub limit: Option<u16>,
}

pub struct TraceqlQuery {
    query: String,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: u16,
}

impl TraceqlInput {
    pub fn validate(self) -> Result<TraceqlQuery, ()> {
        if self.query.trim().is_empty() {
            return Err(());
        }
        let limit = self.limit.unwrap_or(DEFAULT_TRACE_LIMIT);
        if !(1..=MAX_TRACE_LIMIT).contains(&limit) {
            return Err(());
        }
        let start = parse_timestamp(self.start)?;
        let end = parse_timestamp(self.end)?;
        match (start, end) {
            (Some(start), Some(end)) => {
                valid_range(start, end, MAX_RANGE)?;
                Ok(TraceqlQuery {
                    query: self.query,
                    start: Some(start),
                    end: Some(end),
                    limit,
                })
            }
            (None, None) => Ok(TraceqlQuery {
                query: self.query,
                start: None,
                end: None,
                limit,
            }),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfilesInput {
    /// Pyroscope label selector to query.
    pub selector: String,
    /// Inclusive range start as an RFC3339 timestamp.
    pub start: String,
    /// Inclusive range end as an RFC3339 timestamp.
    pub end: String,
    /// Pyroscope profile type, defaulting to process CPU.
    pub profile_type: Option<String>,
    /// Maximum returned flame graph nodes, from 1 through 1000.
    pub max_nodes: Option<u16>,
}

pub struct ProfilesQuery {
    selector: String,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    profile_type: String,
    max_nodes: u16,
}

impl ProfilesInput {
    pub fn validate(self) -> Result<ProfilesQuery, ()> {
        if self.selector.trim().is_empty() {
            return Err(());
        }
        let profile_type = self
            .profile_type
            .unwrap_or_else(|| DEFAULT_PROFILE_TYPE.to_owned());
        if profile_type.trim().is_empty() {
            return Err(());
        }
        let max_nodes = self.max_nodes.unwrap_or(DEFAULT_MAX_NODES);
        if !(1..=MAX_MAX_NODES).contains(&max_nodes) {
            return Err(());
        }
        let start = DateTime::parse_from_rfc3339(&self.start)
            .map_err(|_| ())?
            .to_utc();
        let end = DateTime::parse_from_rfc3339(&self.end)
            .map_err(|_| ())?
            .to_utc();
        valid_range(start, end, MAX_PROFILE_RANGE)?;
        Ok(ProfilesQuery {
            selector: self.selector,
            start,
            end,
            profile_type,
            max_nodes,
        })
    }
}

struct UpstreamRequest {
    method: Method,
    path: &'static str,
    parameters: Vec<(&'static str, String)>,
    body: Option<Value>,
}

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

struct GrafanaMetricsGuard {
    action: &'static str,
    mode: &'static str,
    datasource_uid: &'static str,
    started: Instant,
    finished: bool,
}

impl GrafanaMetricsGuard {
    fn new(action: &'static str, mode: &'static str, datasource_uid: &'static str) -> Self {
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

    fn finish(&mut self, outcome: &'static str) {
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

fn metric_action(action: &'static str) -> &'static str {
    match action {
        "logql" => "logql",
        "promql" => "promql",
        "traceql" => "traceql",
        "profiles" => "profiles",
        _ => "unknown",
    }
}

fn metric_mode(mode: &'static str) -> &'static str {
    match mode {
        "instant" => "instant",
        "range" => "range",
        "search" => "search",
        _ => "unknown",
    }
}

fn metric_datasource_uid(datasource_uid: &'static str) -> &'static str {
    match datasource_uid {
        "loki" => "loki",
        "mimir" => "mimir",
        "tempo" => "tempo",
        "pyroscope" => "pyroscope",
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
        "upstream_unavailable" => "upstream_unavailable",
        "invalid_response" => "invalid_response",
        "cancelled" => "cancelled",
        _ => "upstream_unavailable",
    }
}

impl Drop for GrafanaMetricsGuard {
    fn drop(&mut self) {
        self.finish("cancelled");
    }
}

#[derive(Clone)]
pub struct GrafanaClient {
    origin: Url,
    token: Secret,
    client: Client,
    permits: Arc<Semaphore>,
    timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArguments,
    CapacityExhausted,
    Timeout,
    Unauthorized,
    QueryRejected,
    UpstreamUnavailable,
    InvalidResponse,
}

impl GrafanaClient {
    pub fn production(origin: Url, token: Secret) -> Result<Self, String> {
        Self::new(origin, token, TIMEOUT)
    }

    fn new(origin: Url, token: Secret, timeout: Duration) -> Result<Self, String> {
        let client = Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(|_| "failed to initialize Grafana client".to_owned())?;
        Ok(Self {
            origin,
            token,
            client,
            permits: Arc::new(Semaphore::new(4)),
            timeout,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(origin: Url, timeout: Duration) -> Self {
        Self::new(origin, Secret::for_test("grafana-secret"), timeout).unwrap()
    }

    pub async fn execute(&self, query: &Query) -> Result<Value, Error> {
        let path = match query.mode {
            Mode::Instant => "/api/datasources/proxy/uid/loki/loki/api/v1/query",
            Mode::Range => "/api/datasources/proxy/uid/loki/loki/api/v1/query_range",
        };
        let mut parameters = vec![
            ("query", query.query.clone()),
            ("limit", query.limit.to_string()),
        ];
        if let Some(time) = query.time {
            parameters.push(("time", timestamp_parameter(time)));
        }
        if let (Some(start), Some(end), Some(direction)) = (query.start, query.end, query.direction)
        {
            parameters.extend([
                ("start", timestamp_parameter(start)),
                ("end", timestamp_parameter(end)),
                ("direction", direction.as_str().to_owned()),
            ]);
        }
        self.run(
            UpstreamRequest {
                method: Method::GET,
                path,
                parameters,
                body: None,
            },
            "logql",
            query.mode.as_str(),
            "loki",
            |body| normalize_logql(query.mode, query.limit, body),
        )
        .await
    }

    pub async fn execute_promql(&self, query: &PromqlQuery) -> Result<Value, Error> {
        let path = match query.mode {
            Mode::Instant => "/api/datasources/uid/mimir/resources/api/v1/query",
            Mode::Range => "/api/datasources/uid/mimir/resources/api/v1/query_range",
        };
        let mut parameters = vec![("query", query.query.clone())];
        if let Some(time) = query.time {
            parameters.push(("time", timestamp_parameter(time)));
        }
        if let (Some(start), Some(end), Some(step)) = (query.start, query.end, &query.step) {
            parameters.extend([
                ("start", timestamp_parameter(start)),
                ("end", timestamp_parameter(end)),
                ("step", step.clone()),
            ]);
        }
        self.run(
            UpstreamRequest {
                method: Method::GET,
                path,
                parameters,
                body: None,
            },
            "promql",
            query.mode.as_str(),
            "mimir",
            |body| normalize_promql(query.mode, body),
        )
        .await
    }

    pub async fn execute_traceql(&self, query: &TraceqlQuery) -> Result<Value, Error> {
        let mut parameters = vec![
            ("q", query.query.clone()),
            ("limit", query.limit.to_string()),
        ];
        if let (Some(start), Some(end)) = (query.start, query.end) {
            parameters.extend([
                ("start", start.timestamp().to_string()),
                ("end", end.timestamp().to_string()),
            ]);
        }
        self.run(
            UpstreamRequest {
                method: Method::GET,
                path: "/api/datasources/proxy/uid/tempo/api/search",
                parameters,
                body: None,
            },
            "traceql",
            "search",
            "tempo",
            |body| normalize_traceql(query.limit, body),
        )
        .await
    }

    pub async fn execute_profiles(&self, query: &ProfilesQuery) -> Result<Value, Error> {
        self.run(
            UpstreamRequest {
                method: Method::POST,
                path: "/api/datasources/proxy/uid/pyroscope/querier.v1.QuerierService/SelectMergeStacktraces",
                parameters: Vec::new(),
                body: Some(json!({
                    "profileTypeID": query.profile_type,
                    "labelSelector": query.selector,
                    "start": query.start.timestamp_millis(),
                    "end": query.end.timestamp_millis(),
                    "maxNodes": query.max_nodes,
                })),
            },
            "profiles",
            "range",
            "pyroscope",
            normalize_profiles,
        )
        .await
    }

    async fn run(
        &self,
        request: UpstreamRequest,
        action: &'static str,
        mode: &'static str,
        datasource_uid: &'static str,
        normalize: impl FnOnce(Value) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        let mut metrics = GrafanaMetricsGuard::new(action, mode, datasource_uid);
        let parent = Span::current();
        let parent_name = parent.metadata().map_or("none", |metadata| metadata.name());
        let parent_target = parent
            .metadata()
            .map_or("none", |metadata| metadata.target());
        let parent_context = parent.context();
        let parent_context_valid = parent_context.span().span_context().is_valid();
        let span = tracing::info_span!(
            "grafana.query",
            grafana.action = action,
            grafana.mode = mode,
            grafana.datasource_uid = datasource_uid,
            grafana.outcome = tracing::field::Empty,
            trace.parent.name = parent_name,
            trace.parent.target = parent_target,
            trace.parent.context_valid = parent_context_valid,
        );
        let result = async {
            let _permit = self
                .permits
                .clone()
                .try_acquire_owned()
                .map_err(|_| Error::CapacityExhausted)?;
            let operation = async {
                let mut url = self
                    .origin
                    .join(request.path)
                    .map_err(|_| Error::UpstreamUnavailable)?;
                url.query_pairs_mut()
                    .extend_pairs(request.parameters.iter().map(|(key, value)| (*key, value)));
                if url.as_str().len() > MAX_URL_BYTES {
                    return Err(Error::InvalidArguments);
                }
                let mut builder = self
                    .client
                    .request(request.method, url)
                    .bearer_auth(self.token.expose());
                if let Some(body) = request.body {
                    builder = builder.json(&body);
                }
                let response = builder
                    .send()
                    .await
                    .map_err(|_| Error::UpstreamUnavailable)?;
                let status = response.status();
                match status {
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                        return Err(Error::Unauthorized);
                    }
                    status if status.is_client_error() => return Err(Error::QueryRejected),
                    status if !status.is_success() => return Err(Error::UpstreamUnavailable),
                    _ => {}
                }
                normalize(read_json(response).await?)
            };
            tokio::time::timeout(self.timeout, operation)
                .await
                .map_err(|_| Error::Timeout)?
        }
        .instrument(span.clone())
        .await;
        let outcome = request_outcome(&result);
        span.record("grafana.outcome", outcome);
        metrics.finish(outcome);
        result
    }
}

fn request_outcome(result: &Result<Value, Error>) -> &'static str {
    match result {
        Ok(_) => "success",
        Err(Error::InvalidArguments) => "invalid_arguments",
        Err(Error::CapacityExhausted) => "capacity_exhausted",
        Err(Error::Timeout) => "timeout",
        Err(Error::Unauthorized) => "unauthorized",
        Err(Error::QueryRejected) => "query_rejected",
        Err(Error::UpstreamUnavailable) => "upstream_unavailable",
        Err(Error::InvalidResponse) => "invalid_response",
    }
}

async fn read_json(mut response: Response) -> Result<Value, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(Error::InvalidResponse);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Error::UpstreamUnavailable)?
    {
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(Error::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| Error::InvalidResponse)
}

fn parse_timestamp(value: Option<String>) -> Result<Option<DateTime<Utc>>, ()> {
    value
        .map(|value| DateTime::parse_from_rfc3339(&value).map(|time| time.to_utc()))
        .transpose()
        .map_err(|_| ())
}

fn valid_range(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    maximum: chrono::Duration,
) -> Result<chrono::Duration, ()> {
    let range = end.signed_duration_since(start);
    if range < chrono::Duration::zero() || range > maximum {
        return Err(());
    }
    Ok(range)
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

fn timestamp_parameter(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn normalize_logql(mode: Mode, limit: u16, wrapper: Value) -> Result<Value, Error> {
    if wrapper.get("status").and_then(Value::as_str) != Some("success") {
        return Err(Error::InvalidResponse);
    }
    let data = wrapper
        .get("data")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidResponse)?;
    let result_type = data
        .get("resultType")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidResponse)?;
    let result = data.get("result").ok_or(Error::InvalidResponse)?;
    let result = match result_type {
        "streams" => normalize_streams(result, usize::from(limit))?,
        "matrix" => normalize_series(result, "metric")?,
        "vector" => normalize_vector(result)?,
        "scalar" => normalize_sample(result)?,
        _ => return Err(Error::InvalidResponse),
    };
    let mut output = json!({
        "mode": mode.as_str(),
        "result_type": result_type,
        "result": result,
    });
    if let Some(stats) = data.get("stats").and_then(normalize_stats) {
        output["stats"] = stats;
    }
    Ok(output)
}

fn normalize_promql(mode: Mode, wrapper: Value) -> Result<Value, Error> {
    if wrapper.get("status").and_then(Value::as_str) != Some("success") {
        return Err(Error::InvalidResponse);
    }
    let data = wrapper
        .get("data")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidResponse)?;
    let result_type = data
        .get("resultType")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidResponse)?;
    let result = data.get("result").ok_or(Error::InvalidResponse)?;
    let result = match result_type {
        "matrix" => normalize_series(result, "metric")?,
        "vector" => normalize_vector(result)?,
        "scalar" | "string" => normalize_sample(result)?,
        _ => return Err(Error::InvalidResponse),
    };
    Ok(json!({
        "mode": mode.as_str(),
        "result_type": result_type,
        "result": result,
    }))
}

fn normalize_traceql(limit: u16, wrapper: Value) -> Result<Value, Error> {
    let object = wrapper.as_object().ok_or(Error::InvalidResponse)?;
    let traces = object
        .get("traces")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidResponse)?;
    let mut output = json!({
        "mode": "search",
        "result_type": "traces",
        "result": traces.iter().take(usize::from(limit)).cloned().collect::<Vec<_>>(),
    });
    if let Some(metrics) = object.get("metrics").filter(|value| value.is_object()) {
        output["metrics"] = metrics.clone();
    }
    Ok(output)
}

fn normalize_profiles(wrapper: Value) -> Result<Value, Error> {
    let object = wrapper.as_object().ok_or(Error::InvalidResponse)?;
    if object.len() != 1 {
        return Err(Error::InvalidResponse);
    }
    let flamegraph = object
        .get("flamegraph")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidResponse)?;
    if !flamegraph.get("names").is_some_and(Value::is_array)
        || !flamegraph.get("levels").is_some_and(Value::is_array)
        || !flamegraph.get("total").is_some_and(Value::is_string)
        || !flamegraph.get("maxSelf").is_some_and(Value::is_string)
    {
        return Err(Error::InvalidResponse);
    }
    Ok(json!({
        "mode": "range",
        "result_type": "stacktraces",
        "result": wrapper,
    }))
}

fn normalize_series(result: &Value, labels_key: &str) -> Result<Value, Error> {
    let series = result.as_array().ok_or(Error::InvalidResponse)?;
    series
        .iter()
        .map(|entry| {
            let object = entry.as_object().ok_or(Error::InvalidResponse)?;
            let labels = string_map(object.get(labels_key).ok_or(Error::InvalidResponse)?)?;
            let values = object
                .get("values")
                .and_then(Value::as_array)
                .ok_or(Error::InvalidResponse)?;
            let values = values
                .iter()
                .map(normalize_sample)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({ labels_key: labels, "values": values }))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn normalize_streams(result: &Value, limit: usize) -> Result<Value, Error> {
    let series = result.as_array().ok_or(Error::InvalidResponse)?;
    let mut remaining = limit;
    let mut normalized = Vec::new();
    for entry in series {
        let object = entry.as_object().ok_or(Error::InvalidResponse)?;
        let labels = string_map(object.get("stream").ok_or(Error::InvalidResponse)?)?;
        let values = object
            .get("values")
            .and_then(Value::as_array)
            .ok_or(Error::InvalidResponse)?;
        let mut selected = Vec::new();
        for sample in values {
            let sample = normalize_stream_entry(sample)?;
            if remaining > 0 {
                selected.push(sample);
                remaining -= 1;
            }
        }
        if !selected.is_empty() {
            normalized.push(json!({ "stream": labels, "values": selected }));
        }
    }
    Ok(Value::Array(normalized))
}

fn normalize_vector(result: &Value) -> Result<Value, Error> {
    result
        .as_array()
        .ok_or(Error::InvalidResponse)?
        .iter()
        .map(|entry| {
            let object = entry.as_object().ok_or(Error::InvalidResponse)?;
            Ok(json!({
                "metric": string_map(object.get("metric").ok_or(Error::InvalidResponse)?)?,
                "value": normalize_sample(object.get("value").ok_or(Error::InvalidResponse)?)?,
            }))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn normalize_stream_entry(value: &Value) -> Result<Value, Error> {
    let pair = value
        .as_array()
        .filter(|pair| pair.len() == 2)
        .ok_or(Error::InvalidResponse)?;
    let timestamp = pair[0].as_str().ok_or(Error::InvalidResponse)?;
    let nanoseconds = timestamp
        .parse::<i64>()
        .map_err(|_| Error::InvalidResponse)?;
    let timestamp = DateTime::from_timestamp(
        nanoseconds.div_euclid(1_000_000_000),
        nanoseconds.rem_euclid(1_000_000_000) as u32,
    )
    .ok_or(Error::InvalidResponse)?;
    let line = pair[1].as_str().ok_or(Error::InvalidResponse)?;
    Ok(json!({ "timestamp": timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true), "line": line }))
}

fn normalize_sample(value: &Value) -> Result<Value, Error> {
    let pair = value
        .as_array()
        .filter(|pair| pair.len() == 2)
        .ok_or(Error::InvalidResponse)?;
    let seconds = pair[0].as_f64().ok_or(Error::InvalidResponse)?;
    if !seconds.is_finite() {
        return Err(Error::InvalidResponse);
    }
    let nanoseconds = (seconds * 1_000_000_000.0).round() as i64;
    let timestamp = DateTime::from_timestamp(
        nanoseconds.div_euclid(1_000_000_000),
        nanoseconds.rem_euclid(1_000_000_000) as u32,
    )
    .ok_or(Error::InvalidResponse)?;
    let sample = pair[1].as_str().ok_or(Error::InvalidResponse)?;
    Ok(
        json!({ "timestamp": timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true), "value": sample }),
    )
}

fn string_map(value: &Value) -> Result<Map<String, Value>, Error> {
    value
        .as_object()
        .ok_or(Error::InvalidResponse)?
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), json!(value)))
                .ok_or(Error::InvalidResponse)
        })
        .collect()
}

fn normalize_stats(stats: &Value) -> Option<Value> {
    let summary = stats.get("summary").and_then(Value::as_object)?;
    let mut normalized = Map::new();
    copy_number(
        summary,
        &mut normalized,
        "totalBytesProcessed",
        "bytes_processed",
        1.0,
    );
    copy_number(
        summary,
        &mut normalized,
        "totalLinesProcessed",
        "lines_processed",
        1.0,
    );
    copy_number(
        summary,
        &mut normalized,
        "totalEntriesReturned",
        "entries_returned",
        1.0,
    );
    copy_number(
        summary,
        &mut normalized,
        "execTime",
        "execution_time_ms",
        1000.0,
    );
    (!normalized.is_empty()).then_some(Value::Object(normalized))
}

fn copy_number(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    upstream: &str,
    stable: &str,
    multiplier: f64,
) {
    if let Some(number) = source.get(upstream).and_then(Value::as_f64) {
        target.insert(stable.to_owned(), json!(number * multiplier));
    }
}

#[cfg(test)]
impl Secret {
    fn for_test(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use axum::{
        Json, Router,
        extract::{OriginalUri, Query as QueryParameters, State},
        http::{HeaderMap, StatusCode},
        response::Redirect,
        routing::{get, post},
    };
    use tokio::{net::TcpListener, task::JoinHandle};

    use crate::logql::{Direction, LogqlInput};

    use super::*;

    #[derive(Default)]
    struct RequestRecord {
        method: String,
        path: String,
        authorization: String,
        parameters: HashMap<String, String>,
        body: Value,
    }

    async fn record_request(
        State(record): State<Arc<Mutex<RequestRecord>>>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
        QueryParameters(parameters): QueryParameters<HashMap<String, String>>,
    ) -> Json<Value> {
        *record.lock().unwrap() = RequestRecord {
            method: "GET".to_owned(),
            path: uri.path().to_owned(),
            authorization: headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
            parameters,
            body: Value::Null,
        };
        Json(json!({"status":"success","data":{"resultType":"scalar","result":[1786276800,"1"]}}))
    }

    async fn record_trace_request(
        State(record): State<Arc<Mutex<RequestRecord>>>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
        QueryParameters(parameters): QueryParameters<HashMap<String, String>>,
    ) -> Json<Value> {
        *record.lock().unwrap() = RequestRecord {
            method: "GET".to_owned(),
            path: uri.path().to_owned(),
            authorization: headers["authorization"].to_str().unwrap().to_owned(),
            parameters,
            body: Value::Null,
        };
        Json(json!({"traces":[{"traceID":"one"}],"metrics":{"inspectedBytes":"10"}}))
    }

    async fn record_profile_request(
        State(record): State<Arc<Mutex<RequestRecord>>>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        *record.lock().unwrap() = RequestRecord {
            method: "POST".to_owned(),
            path: uri.path().to_owned(),
            authorization: headers["authorization"].to_str().unwrap().to_owned(),
            parameters: HashMap::new(),
            body,
        };
        Json(profile_response())
    }

    fn profile_response() -> Value {
        json!({
            "flamegraph": {
                "names": ["total", "main"],
                "levels": [[0, 10, 0, 0]],
                "total": "10",
                "maxSelf": "7"
            }
        })
    }

    async fn serve(router: Router) -> (Url, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (origin, task)
    }

    fn instant_query() -> Query {
        LogqlInput {
            query: "{job=\"test\"}".to_owned(),
            start: None,
            end: None,
            time: Some("2026-08-09T12:00:00Z".to_owned()),
            direction: None,
            limit: Some(12),
        }
        .validate()
        .unwrap()
    }

    fn promql_input() -> PromqlInput {
        PromqlInput {
            query: "up".to_owned(),
            start: None,
            end: None,
            step: None,
            time: None,
        }
    }

    fn traceql_input() -> TraceqlInput {
        TraceqlInput {
            query: "{ true }".to_owned(),
            start: None,
            end: None,
            limit: None,
        }
    }

    fn profiles_input() -> ProfilesInput {
        ProfilesInput {
            selector: "{service_name=\"api\"}".to_owned(),
            start: "2026-08-09T10:00:00Z".to_owned(),
            end: "2026-08-09T11:00:00Z".to_owned(),
            profile_type: None,
            max_nodes: None,
        }
    }

    #[test]
    fn validates_promql_modes_durations_ranges_and_point_bound() {
        let instant = promql_input().validate().unwrap();
        assert_eq!(instant.mode, Mode::Instant);

        let mut range = promql_input();
        range.start = Some("2026-08-09T10:00:00Z".to_owned());
        range.end = Some("2026-08-09T11:00:00Z".to_owned());
        range.step = Some("1m".to_owned());
        assert_eq!(range.validate().unwrap().mode, Mode::Range);
        assert_eq!(
            prometheus_duration_nanos("1h30m5s"),
            Some(5_405_000_000_000)
        );

        let mut zero_step = promql_input();
        zero_step.start = Some("2026-08-09T10:00:00Z".to_owned());
        zero_step.end = Some("2026-08-09T11:00:00Z".to_owned());
        zero_step.step = Some("0s".to_owned());
        assert!(zero_step.validate().is_err());

        let mut too_many_points = promql_input();
        too_many_points.start = Some("2026-08-09T10:00:00Z".to_owned());
        too_many_points.end = Some("2026-08-09T10:00:11Z".to_owned());
        too_many_points.step = Some("1ms".to_owned());
        assert!(too_many_points.validate().is_err());

        let mut exactly_max_points = promql_input();
        exactly_max_points.start = Some("2026-08-09T10:00:00Z".to_owned());
        exactly_max_points.end = Some("2026-08-09T10:00:10.999Z".to_owned());
        exactly_max_points.step = Some("1ms".to_owned());
        assert!(exactly_max_points.validate().is_ok());

        let mut too_long = promql_input();
        too_long.start = Some("2026-08-09T10:00:00Z".to_owned());
        too_long.end = Some("2026-08-10T10:00:00.001Z".to_owned());
        too_long.step = Some("1h".to_owned());
        assert!(too_long.validate().is_err());

        let mut incomplete = promql_input();
        incomplete.start = Some("2026-08-09T10:00:00Z".to_owned());
        assert!(incomplete.validate().is_err());
        assert!(prometheus_duration_nanos("1m1h").is_none());
        assert!(prometheus_duration_nanos("1").is_none());
    }

    #[test]
    fn validates_traceql_and_profile_defaults_and_bounds() {
        let trace = traceql_input().validate().unwrap();
        assert_eq!(trace.limit, DEFAULT_TRACE_LIMIT);
        let mut trace_range = traceql_input();
        trace_range.start = Some("2026-08-09T10:00:00Z".to_owned());
        trace_range.end = Some("2026-08-10T10:00:00Z".to_owned());
        trace_range.limit = Some(MAX_TRACE_LIMIT);
        assert!(trace_range.validate().is_ok());
        let mut invalid_trace = traceql_input();
        invalid_trace.limit = Some(MAX_TRACE_LIMIT + 1);
        assert!(invalid_trace.validate().is_err());
        let mut incomplete_trace = traceql_input();
        incomplete_trace.start = Some("2026-08-09T10:00:00Z".to_owned());
        assert!(incomplete_trace.validate().is_err());

        let profile = profiles_input().validate().unwrap();
        assert_eq!(profile.profile_type, DEFAULT_PROFILE_TYPE);
        assert_eq!(profile.max_nodes, DEFAULT_MAX_NODES);
        let mut invalid_profile = profiles_input();
        invalid_profile.end = "2026-08-09T11:00:00.001Z".to_owned();
        assert!(invalid_profile.validate().is_err());
        let mut invalid_nodes = profiles_input();
        invalid_nodes.max_nodes = Some(MAX_MAX_NODES + 1);
        assert!(invalid_nodes.validate().is_err());
        let mut empty_selector = profiles_input();
        empty_selector.selector = " ".to_owned();
        assert!(empty_selector.validate().is_err());
    }

    #[test]
    fn grafana_metric_labels_outcomes_and_guard_are_bounded() {
        assert_eq!(metric_action("profiles"), "profiles");
        assert_eq!(metric_action("raw-query"), "unknown");
        assert_eq!(metric_mode("instant"), "instant");
        assert_eq!(metric_mode("user-mode"), "unknown");
        assert_eq!(metric_datasource_uid("tempo"), "tempo");
        assert_eq!(metric_datasource_uid("user-uid"), "unknown");
        assert_eq!(metric_outcome("timeout"), "timeout");
        assert_eq!(metric_outcome("raw-error"), "upstream_unavailable");

        for (result, expected) in [
            (Ok(json!({})), "success"),
            (Err(Error::InvalidArguments), "invalid_arguments"),
            (Err(Error::CapacityExhausted), "capacity_exhausted"),
            (Err(Error::Timeout), "timeout"),
            (Err(Error::Unauthorized), "unauthorized"),
            (Err(Error::QueryRejected), "query_rejected"),
            (Err(Error::UpstreamUnavailable), "upstream_unavailable"),
            (Err(Error::InvalidResponse), "invalid_response"),
        ] {
            assert_eq!(request_outcome(&result), expected);
        }

        let mut guard = GrafanaMetricsGuard::new("not-an-action", "not-a-mode", "not-a-uid");
        assert_eq!(guard.action, "unknown");
        assert_eq!(guard.mode, "unknown");
        assert_eq!(guard.datasource_uid, "unknown");
        assert!(!guard.finished);
        guard.finish("success");
        assert!(guard.finished);
        guard.finish("timeout");
        assert!(guard.finished);

        // Dropping an unfinished guard exercises the cancellation completion path.
        drop(GrafanaMetricsGuard::new("logql", "instant", "loki"));
    }

    #[test]
    fn normalizes_all_loki_result_types_and_stats() {
        let cases = [
            (
                "streams",
                json!([{"stream":{"job":"a"},"values":[["1786276800000000000","line"]]}]),
            ),
            (
                "matrix",
                json!([{"metric":{"job":"a"},"values":[[1786276800.25,"2"]]}]),
            ),
            (
                "vector",
                json!([{"metric":{"job":"a"},"value":[1786276800,"2"]}]),
            ),
            ("scalar", json!([1786276800, "2"])),
        ];
        for (result_type, result) in cases {
            let body = json!({
                "status":"success",
                "data":{"resultType":result_type,"result":result,"stats":{"summary":{
                    "totalBytesProcessed":2,"totalLinesProcessed":3,"totalEntriesReturned":1,"execTime":0.25,"ignored":99
                }}}
            });
            let normalized = normalize_logql(Mode::Range, 5000, body).unwrap();
            assert_eq!(normalized["result_type"], result_type);
            assert_eq!(
                normalized["stats"],
                json!({
                    "bytes_processed":2.0,"lines_processed":3.0,"entries_returned":1.0,"execution_time_ms":250.0
                })
            );
            assert!(!normalized.to_string().contains("ignored"));
        }
    }

    #[test]
    fn rejects_wrappers_labels_and_samples_that_are_not_contract_shaped() {
        for body in [
            json!({"status":"error","data":{}}),
            json!({"status":"success","data":{"resultType":"unknown","result":[]}}),
            json!({"status":"success","data":{"resultType":"vector","result":[{"metric":{"x":1},"value":[1,"x"]}]}}),
        ] {
            assert_eq!(
                normalize_logql(Mode::Instant, 5000, body),
                Err(Error::InvalidResponse)
            );
        }
    }

    #[test]
    fn rejects_profile_wrappers_errors_tree_format_and_malformed_flamegraphs() {
        let valid_flamegraph = profile_response()["flamegraph"].clone();
        for body in [
            Value::Null,
            json!({}),
            json!({"error":"unsafe upstream detail"}),
            json!({"tree":"AQID"}),
            json!({"data":{"flamegraph":valid_flamegraph}}),
            json!({"flamegraph":valid_flamegraph,"status":"success"}),
            json!({"flamegraph":{}}),
            json!({"flamegraph":{"names":[],"levels":[],"total":"0"}}),
            json!({"flamegraph":{"names":{},"levels":[],"total":"0","maxSelf":"0"}}),
            json!({"flamegraph":{"names":[],"levels":{},"total":"0","maxSelf":"0"}}),
            json!({"flamegraph":{"names":[],"levels":[],"total":0,"maxSelf":"0"}}),
            json!({"flamegraph":{"names":[],"levels":[],"total":"0","maxSelf":0}}),
        ] {
            assert_eq!(normalize_profiles(body), Err(Error::InvalidResponse));
        }

        let normalized = normalize_profiles(profile_response()).unwrap();
        assert_eq!(normalized["mode"], "range");
        assert_eq!(normalized["result_type"], "stacktraces");
        assert_eq!(normalized["result"], profile_response());
    }

    #[test]
    fn truncates_stream_entries_across_series_in_upstream_order() {
        let body = json!({
            "status":"success",
            "data":{"resultType":"streams","result":[
                {"stream":{"job":"first"},"values":[
                    ["1786276800000000000","one"],
                    ["1786276801000000000","two"]
                ]},
                {"stream":{"job":"second"},"values":[
                    ["1786276802000000000","three"],
                    ["1786276803000000000","four"]
                ]}
            ]}
        });

        let normalized = normalize_logql(Mode::Range, 3, body).unwrap();
        assert_eq!(normalized["result"].as_array().unwrap().len(), 2);
        assert_eq!(
            normalized["result"][0]["values"].as_array().unwrap().len(),
            2
        );
        assert_eq!(
            normalized["result"][1]["values"].as_array().unwrap().len(),
            1
        );
        assert_eq!(normalized["result"][1]["values"][0]["line"], "three");
    }

    #[tokio::test]
    async fn sends_only_bearer_auth_and_expected_instant_query_parameters() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                get(record_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);

        client.execute(&instant_query()).await.unwrap();
        let record = record.lock().unwrap();
        assert_eq!(
            record.path,
            "/api/datasources/proxy/uid/loki/loki/api/v1/query"
        );
        assert_eq!(record.authorization, "Bearer grafana-secret");
        assert_eq!(record.parameters["query"], "{job=\"test\"}");
        assert_eq!(record.parameters["limit"], "12");
        assert_eq!(record.parameters["time"], "2026-08-09T12:00:00.000000000Z");
        assert!(
            !record
                .parameters
                .values()
                .any(|value| value == "grafana-secret")
        );
        task.abort();
    }

    #[tokio::test]
    async fn sends_expected_range_path_and_parameters() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query_range",
                get(record_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        let query = LogqlInput {
            query: "rate({job=\"test\"}[5m])".to_owned(),
            start: Some("2026-08-09T10:00:00+00:00".to_owned()),
            end: Some("2026-08-09T11:00:00Z".to_owned()),
            time: None,
            direction: Some(Direction::Forward),
            limit: None,
        }
        .validate()
        .unwrap();

        client.execute(&query).await.unwrap();
        let record = record.lock().unwrap();
        assert_eq!(
            record.path,
            "/api/datasources/proxy/uid/loki/loki/api/v1/query_range"
        );
        assert_eq!(record.parameters["start"], "2026-08-09T10:00:00.000000000Z");
        assert_eq!(record.parameters["end"], "2026-08-09T11:00:00.000000000Z");
        assert_eq!(record.parameters["direction"], "forward");
        assert!(!record.parameters.contains_key("time"));
        task.abort();
    }

    #[tokio::test]
    async fn sends_exact_promql_instant_and_range_requests() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/datasources/uid/mimir/resources/api/v1/query",
                get(record_request),
            )
            .route(
                "/api/datasources/uid/mimir/resources/api/v1/query_range",
                get(record_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);

        let mut instant = promql_input();
        instant.time = Some("2026-08-09T12:00:00+00:00".to_owned());
        client
            .execute_promql(&instant.validate().unwrap())
            .await
            .unwrap();
        {
            let record = record.lock().unwrap();
            assert_eq!(record.method, "GET");
            assert_eq!(
                record.path,
                "/api/datasources/uid/mimir/resources/api/v1/query"
            );
            assert_eq!(record.parameters["query"], "up");
            assert_eq!(record.parameters["time"], "2026-08-09T12:00:00.000000000Z");
            assert_eq!(record.parameters.len(), 2);
        }

        let mut range = promql_input();
        range.start = Some("2026-08-09T10:00:00Z".to_owned());
        range.end = Some("2026-08-09T11:00:00Z".to_owned());
        range.step = Some("30s".to_owned());
        client
            .execute_promql(&range.validate().unwrap())
            .await
            .unwrap();
        let record = record.lock().unwrap();
        assert_eq!(
            record.path,
            "/api/datasources/uid/mimir/resources/api/v1/query_range"
        );
        assert_eq!(record.parameters["start"], "2026-08-09T10:00:00.000000000Z");
        assert_eq!(record.parameters["end"], "2026-08-09T11:00:00.000000000Z");
        assert_eq!(record.parameters["step"], "30s");
        assert!(!record.parameters.contains_key("time"));
        task.abort();
    }

    #[tokio::test]
    async fn sends_exact_traceql_search_request() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/datasources/proxy/uid/tempo/api/search",
                get(record_trace_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        let mut input = traceql_input();
        input.start = Some("2026-08-09T10:00:00Z".to_owned());
        input.end = Some("2026-08-09T11:00:00Z".to_owned());
        input.limit = Some(42);

        let output = client
            .execute_traceql(&input.validate().unwrap())
            .await
            .unwrap();
        let record = record.lock().unwrap();
        assert_eq!(record.method, "GET");
        assert_eq!(record.path, "/api/datasources/proxy/uid/tempo/api/search");
        assert_eq!(record.parameters["q"], "{ true }");
        assert_eq!(record.parameters["limit"], "42");
        assert_eq!(record.parameters["start"], "1786269600");
        assert_eq!(record.parameters["end"], "1786273200");
        assert_eq!(output["result_type"], "traces");
        assert_eq!(output["metrics"]["inspectedBytes"], "10");
        task.abort();
    }

    #[tokio::test]
    async fn sends_exact_profile_post_body_with_defaults() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/datasources/proxy/uid/pyroscope/querier.v1.QuerierService/SelectMergeStacktraces",
                post(record_profile_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);

        let output = client
            .execute_profiles(&profiles_input().validate().unwrap())
            .await
            .unwrap();
        let record = record.lock().unwrap();
        assert_eq!(record.method, "POST");
        assert_eq!(
            record.path,
            "/api/datasources/proxy/uid/pyroscope/querier.v1.QuerierService/SelectMergeStacktraces"
        );
        assert!(record.parameters.is_empty());
        assert_eq!(
            record.body,
            json!({
                "profileTypeID": DEFAULT_PROFILE_TYPE,
                "labelSelector": "{service_name=\"api\"}",
                "start": 1786269600000_i64,
                "end": 1786273200000_i64,
                "maxNodes": DEFAULT_MAX_NODES,
            })
        );
        assert_eq!(
            output,
            json!({
                "mode": "range",
                "result_type": "stacktraces",
                "result": profile_response()
            })
        );
        task.abort();
    }

    #[tokio::test]
    async fn rejects_profile_error_body_without_exposing_it() {
        let router = Router::new().route(
            "/api/datasources/proxy/uid/pyroscope/querier.v1.QuerierService/SelectMergeStacktraces",
            post(|| async { Json(json!({"error":"unsafe upstream profile detail"})) }),
        );
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);

        let result = client
            .execute_profiles(&profiles_input().validate().unwrap())
            .await;
        assert_eq!(result, Err(Error::InvalidResponse));
        assert!(!format!("{result:?}").contains("unsafe upstream profile detail"));
        task.abort();
    }

    #[tokio::test]
    async fn enforces_encoded_url_and_decoded_response_caps() {
        let hit = Arc::new(AtomicBool::new(false));
        let hit_probe = Arc::clone(&hit);
        let router = Router::new().route(
            "/api/datasources/uid/mimir/resources/api/v1/query",
            get(move || {
                let hit_probe = Arc::clone(&hit_probe);
                async move {
                    hit_probe.store(true, Ordering::SeqCst);
                    Json(json!({}))
                }
            }),
        );
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        let mut input = promql_input();
        input.query = "x".repeat(MAX_URL_BYTES);
        assert_eq!(
            client.execute_promql(&input.validate().unwrap()).await,
            Err(Error::InvalidArguments)
        );
        let oversized_logql = LogqlInput {
            query: "x".repeat(MAX_URL_BYTES),
            start: None,
            end: None,
            time: None,
            direction: None,
            limit: None,
        }
        .validate()
        .unwrap();
        assert_eq!(
            client.execute(&oversized_logql).await,
            Err(Error::InvalidArguments)
        );
        assert!(!hit.load(Ordering::SeqCst));
        task.abort();

        let oversized = Router::new().route(
            "/api/datasources/proxy/uid/loki/loki/api/v1/query",
            get(|| async { vec![b'x'; MAX_RESPONSE_BYTES + 1] }),
        );
        let (origin, task) = serve(oversized).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        assert_eq!(
            client.execute(&instant_query()).await,
            Err(Error::InvalidResponse)
        );
        task.abort();
    }

    #[tokio::test]
    async fn shares_capacity_across_query_actions() {
        let client = GrafanaClient::for_test(Url::parse("http://127.0.0.1/").unwrap(), TIMEOUT);
        let permits = (0..4)
            .map(|_| client.permits.clone().try_acquire_owned().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            client
                .execute_promql(&promql_input().validate().unwrap())
                .await,
            Err(Error::CapacityExhausted)
        );
        assert_eq!(
            client
                .execute_traceql(&traceql_input().validate().unwrap())
                .await,
            Err(Error::CapacityExhausted)
        );
        drop(permits);
    }

    #[tokio::test]
    async fn refuses_redirects_without_forwarding_credentials() {
        let target_hit = Arc::new(AtomicBool::new(false));
        let target_probe = Arc::clone(&target_hit);
        let router = Router::new()
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                get(|| async { Redirect::temporary("/target") }),
            )
            .route(
                "/target",
                get(move || {
                    let target_probe = Arc::clone(&target_probe);
                    async move {
                        target_probe.store(true, Ordering::SeqCst);
                        Json(json!({}))
                    }
                }),
            );
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);

        assert_eq!(
            client.execute(&instant_query()).await,
            Err(Error::UpstreamUnavailable)
        );
        assert!(!target_hit.load(Ordering::SeqCst));
        task.abort();
    }

    #[tokio::test]
    async fn maps_grafana_statuses_without_reading_error_details() {
        for (status, expected) in [
            (StatusCode::UNAUTHORIZED, Error::Unauthorized),
            (StatusCode::FORBIDDEN, Error::Unauthorized),
            (StatusCode::BAD_REQUEST, Error::QueryRejected),
            (StatusCode::TOO_MANY_REQUESTS, Error::QueryRejected),
            (StatusCode::SERVICE_UNAVAILABLE, Error::UpstreamUnavailable),
        ] {
            let router = Router::new().route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                get(move || async move { (status, "unsafe upstream detail") }),
            );
            let (origin, task) = serve(router).await;
            let client = GrafanaClient::for_test(origin, TIMEOUT);
            assert_eq!(client.execute(&instant_query()).await, Err(expected));
            task.abort();
        }
    }

    #[tokio::test]
    async fn bounds_timeout_and_capacity() {
        let slow = Router::new().route(
            "/api/datasources/proxy/uid/loki/loki/api/v1/query",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Json(json!({}))
            }),
        );
        let (origin, task) = serve(slow).await;
        let client = GrafanaClient::for_test(origin, Duration::from_millis(10));
        assert_eq!(client.execute(&instant_query()).await, Err(Error::Timeout));
        task.abort();

        let permits = (0..4)
            .map(|_| client.permits.clone().try_acquire_owned().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            client.execute(&instant_query()).await,
            Err(Error::CapacityExhausted)
        );
        drop(permits);
    }

    #[tokio::test]
    async fn dropping_an_inflight_query_releases_its_permit() {
        let started = Arc::new(tokio::sync::Notify::new());
        let started_probe = Arc::clone(&started);
        let slow = Router::new().route(
            "/api/datasources/proxy/uid/loki/loki/api/v1/query",
            get(move || {
                let started_probe = Arc::clone(&started_probe);
                async move {
                    started_probe.notify_one();
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    Json(json!({}))
                }
            }),
        );
        let (origin, server_task) = serve(slow).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        let request_client = client.clone();
        let request_task =
            tokio::spawn(async move { request_client.execute(&instant_query()).await });
        started.notified().await;
        assert_eq!(client.permits.available_permits(), 3);

        request_task.abort();
        request_task.await.unwrap_err();
        tokio::task::yield_now().await;
        assert_eq!(client.permits.available_permits(), 4);
        server_task.abort();
    }
}
