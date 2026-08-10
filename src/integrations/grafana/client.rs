use std::{sync::Arc, time::Duration};

use chrono::{DateTime, SecondsFormat, Utc};
use opentelemetry::trace::TraceContextExt as _;
use reqwest::{Client, Method, Response, StatusCode, Url, redirect::Policy};
use reqwest_middleware::ClientWithMiddleware;
use serde_json::{Map, Value, json};
use tokio::sync::Semaphore;
use tracing::{Instrument as _, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use crate::{
    config::Secret,
    http_client::{ClientRequestSpanGuard, traced_client},
};

use super::{
    Error,
    actions::{
        AlertInstancesQuery, AlertRulesQuery, CreateSilenceCommand, LabelMatcher,
        ListSilencesQuery, Mode, ProfilesQuery, PromqlQuery, Query, SilenceState, TraceqlQuery,
        valid_matcher_name,
    },
    telemetry::{GrafanaMetricsGuard, request_outcome},
};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_URL_BYTES: usize = 8192;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SAFE_MAP_ENTRIES: usize = 64;
const MAX_SAFE_KEY_BYTES: usize = 128;
const MAX_SAFE_VALUE_BYTES: usize = 4096;
const MAX_SUMMARY_BYTES: usize = 512;

#[derive(Clone, Copy)]
enum OperationKind {
    Read,
    Mutation,
}

struct UpstreamRequest {
    method: Method,
    path: &'static str,
    parameters: Vec<(&'static str, String)>,
    body: Option<Value>,
}

#[derive(Clone)]
pub struct GrafanaClient {
    origin: Url,
    token: Secret,
    client: ClientWithMiddleware,
    permits: Arc<Semaphore>,
    timeout: Duration,
}

impl GrafanaClient {
    pub fn production(origin: Url, token: Secret) -> Result<Self, String> {
        Self::new(origin, token, TIMEOUT)
    }

    fn new(origin: Url, token: Secret, timeout: Duration) -> Result<Self, String> {
        let client = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| "failed to initialize Grafana client".to_owned())?;
        Ok(Self {
            origin,
            token,
            client: traced_client(client),
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
            "logql.query",
            query.mode.as_str(),
            "loki",
            OperationKind::Read,
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
            "promql.query",
            query.mode.as_str(),
            "mimir",
            OperationKind::Read,
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
            "traceql.search",
            "search",
            "tempo",
            OperationKind::Read,
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
            "profile.merge",
            "range",
            "pyroscope",
            OperationKind::Read,
            normalize_profiles,
        )
        .await
    }

    pub async fn alert_rules(&self, query: &AlertRulesQuery) -> Result<Value, Error> {
        self.run(
            UpstreamRequest {
                method: Method::GET,
                path: "/api/v1/provisioning/alert-rules",
                parameters: Vec::new(),
                body: None,
            },
            "alert-rule.list",
            "list",
            "grafana_alerting",
            OperationKind::Read,
            |body| normalize_alert_rules(query.limit, body),
        )
        .await
    }

    pub async fn alert_instances(&self, query: &AlertInstancesQuery) -> Result<Value, Error> {
        let parameters = query
            .matchers
            .iter()
            .map(|matcher| ("filter", matcher_filter(matcher)))
            .collect();
        self.run(
            UpstreamRequest {
                method: Method::GET,
                path: "/api/alertmanager/grafana/api/v2/alerts",
                parameters,
                body: None,
            },
            "alert-instance.list",
            "list",
            "grafana_alerting",
            OperationKind::Read,
            |body| normalize_alert_instances(query.limit, body),
        )
        .await
    }

    pub async fn list_silences(&self, query: &ListSilencesQuery) -> Result<Value, Error> {
        self.run(
            UpstreamRequest {
                method: Method::GET,
                path: "/api/alertmanager/grafana/api/v2/silences",
                parameters: Vec::new(),
                body: None,
            },
            "silence.list",
            "list",
            "grafana_alerting",
            OperationKind::Read,
            |body| normalize_silences(query.state, query.limit, body),
        )
        .await
    }

    pub async fn create_silence(&self, command: &CreateSilenceCommand) -> Result<Value, Error> {
        let starts_at = Utc::now();
        let duration =
            chrono::Duration::from_std(command.duration).map_err(|_| Error::InvalidArguments)?;
        let ends_at = starts_at
            .checked_add_signed(duration)
            .ok_or(Error::InvalidArguments)?;
        let starts_at = timestamp_parameter(starts_at);
        let ends_at = timestamp_parameter(ends_at);
        let matchers = command
            .matchers
            .iter()
            .map(|matcher| {
                json!({
                    "name": matcher.name,
                    "value": matcher.value,
                    "isRegex": matcher.operator.is_regex(),
                    "isEqual": matcher.operator.is_equal(),
                })
            })
            .collect::<Vec<_>>();
        self.run(
            UpstreamRequest {
                method: Method::POST,
                path: "/api/alertmanager/grafana/api/v2/silences",
                parameters: Vec::new(),
                body: Some(json!({
                    "matchers": matchers,
                    "startsAt": starts_at,
                    "endsAt": ends_at,
                    "createdBy": "homelab-mcp",
                    "comment": command.comment,
                })),
            },
            "silence.create",
            "create",
            "grafana_alerting",
            OperationKind::Mutation,
            |body| normalize_create_silence(&starts_at, &ends_at, body),
        )
        .await
    }

    async fn run(
        &self,
        request: UpstreamRequest,
        action: &'static str,
        mode: &'static str,
        datasource_uid: &'static str,
        kind: OperationKind,
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
            target: "homelab_mcp::grafana",
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
                    .map_err(|_| operation_failure(kind))?;
                url.query_pairs_mut()
                    .extend_pairs(request.parameters.iter().map(|(key, value)| (*key, value)));
                if url.as_str().len() > MAX_URL_BYTES {
                    return Err(Error::InvalidArguments);
                }
                let mut client_span = ClientRequestSpanGuard::new(&request.method);
                let mut builder = self
                    .client
                    .request(request.method, url)
                    .bearer_auth(self.token.expose());
                if let Some(body) = request.body {
                    builder = builder.json(&body);
                }
                let response = match client_span.attach(builder).send().await {
                    Ok(response) => response,
                    Err(_) => {
                        client_span.finish_transport_error();
                        return Err(operation_failure(kind));
                    }
                };
                let status = response.status();
                client_span.record_status(status);
                match status {
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                        client_span.finish_http_error(status);
                        return Err(Error::Unauthorized);
                    }
                    status if status.is_client_error() => {
                        client_span.finish_http_error(status);
                        return Err(match kind {
                            OperationKind::Read => Error::QueryRejected,
                            OperationKind::Mutation if status == StatusCode::BAD_REQUEST => {
                                Error::MutationRejected
                            }
                            OperationKind::Mutation => Error::MutationOutcomeUnknown,
                        });
                    }
                    status if !status.is_success() => {
                        client_span.finish_http_error(status);
                        return Err(operation_failure(kind));
                    }
                    _ => {}
                }
                let body =
                    read_json(response, &mut client_span)
                        .await
                        .map_err(|error| match kind {
                            OperationKind::Read => error,
                            OperationKind::Mutation => Error::MutationOutcomeUnknown,
                        })?;
                normalize(body).map_err(|error| match kind {
                    OperationKind::Read => error,
                    OperationKind::Mutation => Error::MutationOutcomeUnknown,
                })
            };
            tokio::time::timeout(self.timeout, operation)
                .await
                .map_err(|_| match kind {
                    OperationKind::Read => Error::Timeout,
                    OperationKind::Mutation => Error::MutationOutcomeUnknown,
                })?
        }
        .instrument(span.clone())
        .await;
        let outcome = request_outcome(&result);
        span.record("grafana.outcome", outcome);
        metrics.finish(outcome);
        result
    }
}

async fn read_json(
    mut response: Response,
    client_span: &mut ClientRequestSpanGuard,
) -> Result<Value, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        client_span.finish_response_error();
        return Err(Error::InvalidResponse);
    }
    let mut body = Vec::new();
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(_) => {
                client_span.finish_transport_error();
                return Err(Error::UpstreamUnavailable);
            }
        };
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            client_span.finish_response_error();
            return Err(Error::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    client_span.finish_success();
    serde_json::from_slice(&body).map_err(|_| Error::InvalidResponse)
}

fn timestamp_parameter(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

const fn operation_failure(kind: OperationKind) -> Error {
    match kind {
        OperationKind::Read => Error::UpstreamUnavailable,
        OperationKind::Mutation => Error::MutationOutcomeUnknown,
    }
}

fn matcher_filter(matcher: &LabelMatcher) -> String {
    format!(
        "{}{}{}",
        matcher.name,
        matcher.operator.as_str(),
        serde_json::to_string(&matcher.value).expect("serializing a string cannot fail")
    )
}

fn normalize_alert_rules(limit: u16, wrapper: Value) -> Result<Value, Error> {
    let rules = wrapper.as_array().ok_or(Error::InvalidResponse)?;
    let result = rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let object = rule.as_object().ok_or(Error::InvalidResponse)?;
            let normalized = json!({
                "uid": bounded_string(object, "uid", MAX_SAFE_KEY_BYTES)?,
                "title": bounded_string(object, "title", MAX_SUMMARY_BYTES)?,
                "folder_uid": bounded_string(object, "folderUID", MAX_SAFE_KEY_BYTES)?,
                "rule_group": bounded_string(object, "ruleGroup", MAX_SUMMARY_BYTES)?,
                "condition": bounded_string(object, "condition", MAX_SAFE_KEY_BYTES)?,
                "no_data_state": bounded_string(object, "noDataState", MAX_SAFE_KEY_BYTES)?,
                "exec_err_state": bounded_string(object, "execErrState", MAX_SAFE_KEY_BYTES)?,
                "for": bounded_string(object, "for", MAX_SAFE_KEY_BYTES)?,
                "is_paused": object.get("isPaused").and_then(Value::as_bool).ok_or(Error::InvalidResponse)?,
                "labels": safe_string_map(object.get("labels").ok_or(Error::InvalidResponse)?)?,
                "annotations": safe_string_map(object.get("annotations").ok_or(Error::InvalidResponse)?)?,
            });
            Ok((index < usize::from(limit)).then_some(normalized))
        })
        .collect::<Result<Vec<_>, Error>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    Ok(json!({
        "mode": "list",
        "result_type": "alert_rules",
        "result": result,
    }))
}

fn normalize_alert_instances(limit: u16, wrapper: Value) -> Result<Value, Error> {
    let alerts = wrapper.as_array().ok_or(Error::InvalidResponse)?;
    let result = alerts
        .iter()
        .enumerate()
        .map(|(index, alert)| {
            let object = alert.as_object().ok_or(Error::InvalidResponse)?;
            let status = object
                .get("status")
                .and_then(Value::as_object)
                .ok_or(Error::InvalidResponse)?;
            let silenced_by = status
                .get("silencedBy")
                .and_then(Value::as_array)
                .ok_or(Error::InvalidResponse)?;
            let inhibited_by = status
                .get("inhibitedBy")
                .and_then(Value::as_array)
                .ok_or(Error::InvalidResponse)?;
            if !silenced_by.iter().all(Value::is_string)
                || !inhibited_by.iter().all(Value::is_string)
            {
                return Err(Error::InvalidResponse);
            }
            let normalized = json!({
                "fingerprint": bounded_string(object, "fingerprint", MAX_SAFE_KEY_BYTES)?,
                "starts_at": normalized_timestamp(object, "startsAt")?,
                "ends_at": normalized_timestamp(object, "endsAt")?,
                "updated_at": normalized_timestamp(object, "updatedAt")?,
                "state": bounded_string(status, "state", MAX_SAFE_KEY_BYTES)?,
                "silenced": !silenced_by.is_empty(),
                "inhibited": !inhibited_by.is_empty(),
                "labels": safe_string_map(object.get("labels").ok_or(Error::InvalidResponse)?)?,
                "annotations": safe_string_map(object.get("annotations").ok_or(Error::InvalidResponse)?)?,
            });
            Ok((index < usize::from(limit)).then_some(normalized))
        })
        .collect::<Result<Vec<_>, Error>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    Ok(json!({
        "mode": "list",
        "result_type": "alert_instances",
        "result": result,
    }))
}

fn normalize_silences(
    state_filter: Option<SilenceState>,
    limit: u16,
    wrapper: Value,
) -> Result<Value, Error> {
    let silences = wrapper.as_array().ok_or(Error::InvalidResponse)?;
    let mut result = Vec::new();
    for silence in silences {
        let object = silence.as_object().ok_or(Error::InvalidResponse)?;
        let status = object
            .get("status")
            .and_then(Value::as_object)
            .ok_or(Error::InvalidResponse)?;
        let state = silence_state(status)?;
        let matchers = normalize_silence_matchers(
            object
                .get("matchers")
                .and_then(Value::as_array)
                .ok_or(Error::InvalidResponse)?,
        )?;
        let normalized = json!({
            "silence_id": bounded_string(object, "id", MAX_SAFE_KEY_BYTES)?,
            "state": state.as_str(),
            "starts_at": normalized_timestamp(object, "startsAt")?,
            "ends_at": normalized_timestamp(object, "endsAt")?,
            "created_by": bounded_string(object, "createdBy", MAX_SUMMARY_BYTES)?,
            "comment": bounded_string(object, "comment", MAX_SAFE_VALUE_BYTES)?,
            "matchers": matchers,
        });
        if state_filter.is_none_or(|filter| filter == state) && result.len() < usize::from(limit) {
            result.push(normalized);
        }
    }
    Ok(json!({
        "mode": "list",
        "result_type": "silences",
        "result": result,
    }))
}

fn silence_state(status: &Map<String, Value>) -> Result<SilenceState, Error> {
    match bounded_string(status, "state", MAX_SAFE_KEY_BYTES)? {
        "active" => Ok(SilenceState::Active),
        "pending" => Ok(SilenceState::Pending),
        "expired" => Ok(SilenceState::Expired),
        _ => Err(Error::InvalidResponse),
    }
}

fn normalize_silence_matchers(matchers: &[Value]) -> Result<Vec<Value>, Error> {
    if matchers.is_empty() || matchers.len() > 20 {
        return Err(Error::InvalidResponse);
    }
    matchers
        .iter()
        .map(|matcher| {
            let object = matcher.as_object().ok_or(Error::InvalidResponse)?;
            let name = bounded_string(object, "name", 128)?;
            if !valid_matcher_name(name) {
                return Err(Error::InvalidResponse);
            }
            let value = object
                .get("value")
                .and_then(Value::as_str)
                .filter(|value| value.len() <= 1024)
                .ok_or(Error::InvalidResponse)?;
            let is_regex = object
                .get("isRegex")
                .and_then(Value::as_bool)
                .ok_or(Error::InvalidResponse)?;
            let is_equal = object
                .get("isEqual")
                .and_then(Value::as_bool)
                .ok_or(Error::InvalidResponse)?;
            let operator = match (is_regex, is_equal) {
                (false, true) => "=",
                (false, false) => "!=",
                (true, true) => "=~",
                (true, false) => "!~",
            };
            Ok(json!({"name": name, "operator": operator, "value": value}))
        })
        .collect()
}

fn normalize_create_silence(
    starts_at: &str,
    ends_at: &str,
    wrapper: Value,
) -> Result<Value, Error> {
    let object = wrapper.as_object().ok_or(Error::InvalidResponse)?;
    if object.len() != 1 {
        return Err(Error::InvalidResponse);
    }
    Ok(json!({
        "silence_id": bounded_string(object, "silenceID", MAX_SAFE_KEY_BYTES)?,
        "starts_at": starts_at,
        "ends_at": ends_at,
    }))
}

fn bounded_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    maximum_bytes: usize,
) -> Result<&'a str, Error> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= maximum_bytes)
        .ok_or(Error::InvalidResponse)
}

fn normalized_timestamp(object: &Map<String, Value>, key: &str) -> Result<String, Error> {
    let value = bounded_string(object, key, MAX_SAFE_KEY_BYTES)?;
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| {
            timestamp
                .to_utc()
                .to_rfc3339_opts(SecondsFormat::Nanos, true)
        })
        .map_err(|_| Error::InvalidResponse)
}

fn safe_string_map(value: &Value) -> Result<Map<String, Value>, Error> {
    let object = value.as_object().ok_or(Error::InvalidResponse)?;
    if object.len() > MAX_SAFE_MAP_ENTRIES {
        return Err(Error::InvalidResponse);
    }
    object
        .iter()
        .try_fold(Map::new(), |mut output, (key, value)| {
            let value = value.as_str().ok_or(Error::InvalidResponse)?;
            if key.is_empty()
                || key.len() > MAX_SAFE_KEY_BYTES
                || value.len() > MAX_SAFE_VALUE_BYTES
            {
                return Err(Error::InvalidResponse);
            }
            if !url_designated_key(key) && !url_like_value(value) {
                output.insert(key.clone(), json!(value));
            }
            Ok(output)
        })
}

fn url_designated_key(key: &str) -> bool {
    key.to_ascii_lowercase().ends_with("url")
}

fn url_like_value(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('/') || has_absolute_scheme(value) || has_markdown_link(value)
}

fn has_absolute_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    let mut characters = scheme.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || "+-.".contains(character))
}

fn has_markdown_link(value: &str) -> bool {
    value
        .match_indices("](")
        .any(|(index, _)| value[index + 2..].find(')').is_some_and(|end| end > 0))
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

    use crate::integrations::grafana::actions::{
        AlertInstancesInput, AlertRulesInput, CreateSilenceInput, DEFAULT_MAX_NODES,
        DEFAULT_PROFILE_TYPE, Direction, LabelMatcher, ListSilencesInput, LogqlInput,
        MatcherOperator, ProfilesInput, PromqlInput, TraceqlInput,
    };

    use super::*;

    #[derive(Default)]
    struct RequestRecord {
        method: String,
        path: String,
        authorization: String,
        traceparent: String,
        parameters: HashMap<String, String>,
        parameter_pairs: Vec<(String, String)>,
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
            traceparent: traceparent(&headers),
            parameters,
            parameter_pairs: uri
                .query()
                .map(|query| {
                    url::form_urlencoded::parse(query.as_bytes())
                        .into_owned()
                        .collect()
                })
                .unwrap_or_default(),
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
            traceparent: traceparent(&headers),
            parameters,
            parameter_pairs: uri
                .query()
                .map(|query| {
                    url::form_urlencoded::parse(query.as_bytes())
                        .into_owned()
                        .collect()
                })
                .unwrap_or_default(),
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
            traceparent: traceparent(&headers),
            parameters: HashMap::new(),
            parameter_pairs: Vec::new(),
            body,
        };
        Json(profile_response())
    }

    async fn record_alert_rules_request(
        State(record): State<Arc<Mutex<RequestRecord>>>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
    ) -> Json<Value> {
        *record.lock().unwrap() = RequestRecord {
            method: "GET".to_owned(),
            path: uri.path().to_owned(),
            authorization: headers["authorization"].to_str().unwrap().to_owned(),
            traceparent: traceparent(&headers),
            parameters: HashMap::new(),
            parameter_pairs: Vec::new(),
            body: Value::Null,
        };
        Json(alert_rules_response())
    }

    async fn record_alert_instances_request(
        State(record): State<Arc<Mutex<RequestRecord>>>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
    ) -> Json<Value> {
        let parameter_pairs = uri
            .query()
            .map(|query| {
                url::form_urlencoded::parse(query.as_bytes())
                    .into_owned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        *record.lock().unwrap() = RequestRecord {
            method: "GET".to_owned(),
            path: uri.path().to_owned(),
            authorization: headers["authorization"].to_str().unwrap().to_owned(),
            traceparent: traceparent(&headers),
            parameters: parameter_pairs.iter().cloned().collect(),
            parameter_pairs,
            body: Value::Null,
        };
        Json(alert_instances_response())
    }

    async fn record_silence_request(
        State(record): State<Arc<Mutex<RequestRecord>>>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        *record.lock().unwrap() = RequestRecord {
            method: "POST".to_owned(),
            path: uri.path().to_owned(),
            authorization: headers["authorization"].to_str().unwrap().to_owned(),
            traceparent: traceparent(&headers),
            parameters: HashMap::new(),
            parameter_pairs: Vec::new(),
            body,
        };
        Json(json!({"silenceID":"silence-123"}))
    }

    async fn record_silences_request(
        State(record): State<Arc<Mutex<RequestRecord>>>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
    ) -> Json<Value> {
        *record.lock().unwrap() = RequestRecord {
            method: "GET".to_owned(),
            path: uri.path().to_owned(),
            authorization: headers["authorization"].to_str().unwrap().to_owned(),
            traceparent: traceparent(&headers),
            parameters: HashMap::new(),
            parameter_pairs: Vec::new(),
            body: Value::Null,
        };
        Json(silences_response())
    }

    fn traceparent(headers: &HeaderMap) -> String {
        headers
            .get("traceparent")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
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

    fn alert_rules_response() -> Value {
        json!([{
            "id": 7,
            "uid": "rule-1",
            "orgID": 1,
            "folderUID": "folder-1",
            "ruleGroup": "api",
            "title": "API errors",
            "condition": "C",
            "data": [{"refId":"C","model":{"datasource":{"uid":"internal"}}}],
            "updated": "2026-08-10T12:00:00Z",
            "noDataState": "NoData",
            "execErrState": "Error",
            "for": "5m",
            "annotations": {"summary":"API is failing","dashboard_url":"http://grafana.internal/d/one"},
            "labels": {"severity":"critical"},
            "isPaused": false,
        }])
    }

    fn alert_instances_response() -> Value {
        json!([{
            "annotations": {"summary":"API is failing","runbook_url":"https://wiki.internal/runbook"},
            "endsAt": "2026-08-10T13:00:00Z",
            "fingerprint": "abc123",
            "receivers": [{"name":"internal-receiver"}],
            "startsAt": "2026-08-10T12:00:00Z",
            "status": {"inhibitedBy":[],"silencedBy":["secret-silence-id"],"state":"suppressed"},
            "updatedAt": "2026-08-10T12:01:00Z",
            "generatorURL": "http://grafana.internal/alerting/1",
            "labels": {"alertname":"APIError","severity":"critical"},
        }])
    }

    fn silences_response() -> Value {
        json!([
            {
                "id":"silence-active", "status":{"state":"active"},
                "startsAt":"2026-08-10T12:00:00Z", "endsAt":"2026-08-10T13:00:00Z",
                "createdBy":"homelab-mcp", "comment":"maintenance",
                "matchers":[
                    {"name":"alertname","value":"API.*","isRegex":true,"isEqual":true},
                    {"name":"severity","value":"warning","isRegex":false,"isEqual":false}
                ],
                "updatedAt":"2026-08-10T12:01:00Z"
            },
            {
                "id":"silence-expired", "status":{"state":"expired"},
                "startsAt":"2026-08-09T12:00:00Z", "endsAt":"2026-08-09T13:00:00Z",
                "createdBy":"operator", "comment":"old maintenance",
                "matchers":[{"name":"job","value":"api","isRegex":false,"isEqual":true}]
            }
        ])
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

    fn silence_input() -> CreateSilenceInput {
        CreateSilenceInput {
            matchers: vec![LabelMatcher {
                name: "alertname".to_owned(),
                operator: MatcherOperator::Equal,
                value: "APIError".to_owned(),
            }],
            duration_seconds: 3600,
            comment: "maintenance".to_owned(),
        }
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
    fn normalizes_alert_rules_and_instances_without_upstream_internal_fields() {
        let rules = normalize_alert_rules(1, alert_rules_response()).unwrap();
        assert_eq!(rules["mode"], "list");
        assert_eq!(rules["result_type"], "alert_rules");
        assert_eq!(rules["result"][0]["uid"], "rule-1");
        assert_eq!(rules["result"][0]["labels"]["severity"], "critical");
        assert!(!rules.to_string().contains("datasource"));
        assert!(!rules.to_string().contains("orgID"));

        let alerts = normalize_alert_instances(1, alert_instances_response()).unwrap();
        assert_eq!(alerts["result_type"], "alert_instances");
        assert_eq!(alerts["result"][0]["state"], "suppressed");
        assert_eq!(alerts["result"][0]["silenced"], true);
        assert_eq!(alerts["result"][0]["inhibited"], false);
        assert_eq!(
            alerts["result"][0]["starts_at"],
            "2026-08-10T12:00:00.000000000Z"
        );
        assert!(!alerts.to_string().contains("grafana.internal"));
        assert!(!alerts.to_string().contains("secret-silence-id"));
        assert!(!alerts.to_string().contains("internal-receiver"));
    }

    #[test]
    fn normalizes_filters_and_limits_silences_without_upstream_fields() {
        let silences =
            normalize_silences(Some(SilenceState::Active), 1, silences_response()).unwrap();
        assert_eq!(silences["mode"], "list");
        assert_eq!(silences["result_type"], "silences");
        assert_eq!(silences["result"].as_array().unwrap().len(), 1);
        assert_eq!(silences["result"][0]["silence_id"], "silence-active");
        assert_eq!(silences["result"][0]["state"], "active");
        assert_eq!(silences["result"][0]["created_by"], "homelab-mcp");
        assert_eq!(silences["result"][0]["comment"], "maintenance");
        assert_eq!(
            silences["result"][0]["matchers"],
            json!([
                {"name":"alertname","operator":"=~","value":"API.*"},
                {"name":"severity","operator":"!=","value":"warning"}
            ])
        );
        assert!(!silences.to_string().contains("updatedAt"));

        let expired =
            normalize_silences(Some(SilenceState::Expired), 1, silences_response()).unwrap();
        assert_eq!(expired["result"][0]["silence_id"], "silence-expired");
    }

    #[test]
    fn safe_alert_maps_omit_deterministic_url_shapes_and_preserve_text() {
        let normalized = safe_string_map(&json!({
            "http": "http://grafana.internal/d/one",
            "mixed_https": "HtTpS://grafana.internal/d/two",
            "protocol_relative": "//grafana.internal/d/three",
            "root_relative": "/alerting/rules/four",
            "other_scheme": "ftp:internal.example/five",
            "markdown": "See [runbook](internal.example/runbooks/six)",
            "dashboard_url": "grafana.internal/d/seven",
            "RUNBOOKURL": "wiki.internal/runbooks/eight",
            "summary": "API is failing",
            "severity": "critical"
        }))
        .unwrap();

        assert_eq!(
            Value::Object(normalized),
            json!({"summary":"API is failing","severity":"critical"})
        );
    }

    #[test]
    fn strictly_validates_alert_responses_including_truncated_entries() {
        let mut rules = alert_rules_response().as_array().unwrap().clone();
        rules.push(json!({"uid":"malformed"}));
        assert_eq!(
            normalize_alert_rules(1, Value::Array(rules)),
            Err(Error::InvalidResponse)
        );

        let mut alerts = alert_instances_response().as_array().unwrap().clone();
        alerts.push(json!({"generatorURL":"http://unsafe"}));
        assert_eq!(
            normalize_alert_instances(1, Value::Array(alerts)),
            Err(Error::InvalidResponse)
        );

        let mut oversized_labels = serde_json::Map::new();
        for index in 0..=MAX_SAFE_MAP_ENTRIES {
            oversized_labels.insert(format!("label_{index}"), json!("value"));
        }
        let mut response = alert_instances_response();
        response[0]["labels"] = Value::Object(oversized_labels);
        assert_eq!(
            normalize_alert_instances(1, response),
            Err(Error::InvalidResponse)
        );

        let mut silences = silences_response().as_array().unwrap().clone();
        silences.push(json!({"id":"malformed"}));
        assert_eq!(
            normalize_silences(None, 1, Value::Array(silences)),
            Err(Error::InvalidResponse)
        );

        let mut invalid_state = silences_response();
        invalid_state[0]["status"]["state"] = json!("unknown");
        assert_eq!(
            normalize_silences(None, 1, invalid_state),
            Err(Error::InvalidResponse)
        );

        let mut filtered_malformed = silences_response();
        filtered_malformed[1]["startsAt"] = json!("not-a-timestamp");
        assert_eq!(
            normalize_silences(Some(SilenceState::Active), 1, filtered_malformed),
            Err(Error::InvalidResponse)
        );
    }

    #[test]
    fn rejects_malformed_and_oversized_silence_fields() {
        let mut cases = Vec::new();

        let mut empty_id = silences_response();
        empty_id[0]["id"] = json!("");
        cases.push(empty_id);

        let mut oversized_id = silences_response();
        oversized_id[0]["id"] = json!("x".repeat(MAX_SAFE_KEY_BYTES + 1));
        cases.push(oversized_id);

        let mut empty_creator = silences_response();
        empty_creator[0]["createdBy"] = json!("");
        cases.push(empty_creator);

        let mut oversized_creator = silences_response();
        oversized_creator[0]["createdBy"] = json!("x".repeat(MAX_SUMMARY_BYTES + 1));
        cases.push(oversized_creator);

        let mut oversized_comment = silences_response();
        oversized_comment[0]["comment"] = json!("x".repeat(MAX_SAFE_VALUE_BYTES + 1));
        cases.push(oversized_comment);

        let mut no_matchers = silences_response();
        no_matchers[0]["matchers"] = json!([]);
        cases.push(no_matchers);

        let mut too_many_matchers = silences_response();
        too_many_matchers[0]["matchers"] = json!(vec![
            json!({"name":"job","value":"api","isRegex":false,"isEqual":true});
            21
        ]);
        cases.push(too_many_matchers);

        let mut invalid_name = silences_response();
        invalid_name[0]["matchers"][0]["name"] = json!("invalid-name");
        cases.push(invalid_name);

        let mut oversized_value = silences_response();
        oversized_value[0]["matchers"][0]["value"] = json!("x".repeat(1025));
        cases.push(oversized_value);

        let mut invalid_flag = silences_response();
        invalid_flag[0]["matchers"][0]["isRegex"] = json!("false");
        cases.push(invalid_flag);

        for response in cases {
            assert_eq!(
                normalize_silences(None, 100, response),
                Err(Error::InvalidResponse)
            );
        }
    }

    #[test]
    fn strictly_normalizes_only_documented_silence_creation_response() {
        assert_eq!(
            normalize_create_silence(
                "2026-08-10T12:00:00Z",
                "2026-08-10T13:00:00Z",
                json!({"silenceID":"silence-123"})
            )
            .unwrap(),
            json!({
                "silence_id":"silence-123",
                "starts_at":"2026-08-10T12:00:00Z",
                "ends_at":"2026-08-10T13:00:00Z"
            })
        );
        for invalid in [
            json!({}),
            json!({"silenceID":""}),
            json!({"silenceID":"one","message":"unsafe"}),
        ] {
            assert_eq!(
                normalize_create_silence("start", "end", invalid),
                Err(Error::InvalidResponse)
            );
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
        assert!(record.traceparent.is_empty());
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
    async fn sends_exact_alert_rule_request_and_truncates_in_upstream_order() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/v1/provisioning/alert-rules",
                get(record_alert_rules_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);

        let output = client
            .alert_rules(&AlertRulesInput { limit: Some(1) }.validate().unwrap())
            .await
            .unwrap();
        let record = record.lock().unwrap();
        assert_eq!(record.method, "GET");
        assert_eq!(record.path, "/api/v1/provisioning/alert-rules");
        assert_eq!(record.authorization, "Bearer grafana-secret");
        assert!(record.traceparent.is_empty());
        assert!(record.parameter_pairs.is_empty());
        assert_eq!(output["result"].as_array().unwrap().len(), 1);
        assert_eq!(output["result"][0]["title"], "API errors");
        task.abort();
    }

    #[tokio::test]
    async fn sends_only_server_constructed_alert_instance_filters() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/alertmanager/grafana/api/v2/alerts",
                get(record_alert_instances_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        let query = AlertInstancesInput {
            matchers: vec![
                LabelMatcher {
                    name: "severity".to_owned(),
                    operator: MatcherOperator::Equal,
                    value: "critical".to_owned(),
                },
                LabelMatcher {
                    name: "job".to_owned(),
                    operator: MatcherOperator::RegexNotEqual,
                    value: "api\"canary".to_owned(),
                },
            ],
            limit: Some(1),
        }
        .validate()
        .unwrap();

        client.alert_instances(&query).await.unwrap();
        let record = record.lock().unwrap();
        assert_eq!(record.method, "GET");
        assert_eq!(record.path, "/api/alertmanager/grafana/api/v2/alerts");
        assert_eq!(record.authorization, "Bearer grafana-secret");
        assert_eq!(
            record.parameter_pairs,
            vec![
                ("filter".to_owned(), "severity=\"critical\"".to_owned()),
                ("filter".to_owned(), "job!~\"api\\\"canary\"".to_owned()),
            ]
        );
        assert!(!record.parameters.contains_key("limit"));
        task.abort();
    }

    #[tokio::test]
    async fn sends_exact_silence_list_request_and_filters_before_limiting() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/alertmanager/grafana/api/v2/silences",
                get(record_silences_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        let query = ListSilencesInput {
            state: Some(SilenceState::Expired),
            limit: Some(1),
        }
        .validate()
        .unwrap();

        let output = client.list_silences(&query).await.unwrap();
        let record = record.lock().unwrap();
        assert_eq!(record.method, "GET");
        assert_eq!(record.path, "/api/alertmanager/grafana/api/v2/silences");
        assert_eq!(record.authorization, "Bearer grafana-secret");
        assert!(record.parameter_pairs.is_empty());
        assert_eq!(output["result"].as_array().unwrap().len(), 1);
        assert_eq!(output["result"][0]["silence_id"], "silence-expired");
        task.abort();
    }

    #[tokio::test]
    async fn sends_exact_silence_body_and_returns_only_safe_operation_result() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/alertmanager/grafana/api/v2/silences",
                post(record_silence_request),
            )
            .with_state(Arc::clone(&record));
        let (origin, task) = serve(router).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        let command = CreateSilenceInput {
            matchers: vec![
                LabelMatcher {
                    name: "alertname".to_owned(),
                    operator: MatcherOperator::RegexEqual,
                    value: "API.*".to_owned(),
                },
                LabelMatcher {
                    name: "severity".to_owned(),
                    operator: MatcherOperator::NotEqual,
                    value: "warning".to_owned(),
                },
            ],
            duration_seconds: 3600,
            comment: "maintenance".to_owned(),
        }
        .validate()
        .unwrap();

        let output = client.create_silence(&command).await.unwrap();
        let record = record.lock().unwrap();
        assert_eq!(record.method, "POST");
        assert_eq!(record.path, "/api/alertmanager/grafana/api/v2/silences");
        assert_eq!(record.authorization, "Bearer grafana-secret");
        assert!(record.parameter_pairs.is_empty());
        assert_eq!(record.body["createdBy"], "homelab-mcp");
        assert_eq!(record.body["comment"], "maintenance");
        assert_eq!(
            record.body["matchers"],
            json!([
                {"name":"alertname","value":"API.*","isRegex":true,"isEqual":true},
                {"name":"severity","value":"warning","isRegex":false,"isEqual":false}
            ])
        );
        let starts_at = record.body["startsAt"].as_str().unwrap();
        let ends_at = record.body["endsAt"].as_str().unwrap();
        let starts = DateTime::parse_from_rfc3339(starts_at).unwrap();
        let ends = DateTime::parse_from_rfc3339(ends_at).unwrap();
        assert_eq!(
            ends.signed_duration_since(starts),
            chrono::Duration::hours(1)
        );
        assert_eq!(output["silence_id"], "silence-123");
        assert_eq!(output["starts_at"], starts_at);
        assert_eq!(output["ends_at"], ends_at);
        assert_eq!(output.as_object().unwrap().len(), 3);
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
    async fn maps_silence_errors_without_details_and_marks_uncertain_outcomes() {
        for (status, expected) in [
            (StatusCode::BAD_REQUEST, Error::MutationRejected),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Error::MutationOutcomeUnknown,
            ),
        ] {
            let router = Router::new().route(
                "/api/alertmanager/grafana/api/v2/silences",
                post(move || async move { (status, "unsafe upstream mutation detail") }),
            );
            let (origin, task) = serve(router).await;
            let client = GrafanaClient::for_test(origin, TIMEOUT);
            let result = client
                .create_silence(&silence_input().validate().unwrap())
                .await;
            assert_eq!(result, Err(expected));
            assert!(!format!("{result:?}").contains("unsafe upstream mutation detail"));
            task.abort();
        }

        let malformed = Router::new().route(
            "/api/alertmanager/grafana/api/v2/silences",
            post(|| async { Json(json!({"message":"unsafe created maybe"})) }),
        );
        let (origin, task) = serve(malformed).await;
        let client = GrafanaClient::for_test(origin, TIMEOUT);
        assert_eq!(
            client
                .create_silence(&silence_input().validate().unwrap())
                .await,
            Err(Error::MutationOutcomeUnknown)
        );
        task.abort();

        let slow = Router::new().route(
            "/api/alertmanager/grafana/api/v2/silences",
            post(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Json(json!({"silenceID":"late"}))
            }),
        );
        let (origin, task) = serve(slow).await;
        let client = GrafanaClient::for_test(origin, Duration::from_millis(10));
        let result = client
            .create_silence(&silence_input().validate().unwrap())
            .await;
        assert_eq!(result, Err(Error::MutationOutcomeUnknown));
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
        assert_eq!(
            client
                .alert_rules(&AlertRulesInput { limit: None }.validate().unwrap())
                .await,
            Err(Error::CapacityExhausted)
        );
        assert_eq!(
            client
                .create_silence(&silence_input().validate().unwrap())
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
