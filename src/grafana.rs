use std::{sync::Arc, time::Duration};

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::{Client, StatusCode, Url, redirect::Policy};
use serde_json::{Map, Value, json};
use tokio::sync::Semaphore;

use crate::{
    config::Secret,
    logql::{Mode, Query},
};

const TIMEOUT: Duration = Duration::from_secs(30);

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
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::CapacityExhausted)?;
        let operation = async {
            let path = match query.mode {
                Mode::Instant => "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                Mode::Range => "/api/datasources/proxy/uid/loki/loki/api/v1/query_range",
            };
            let url = self
                .origin
                .join(path)
                .map_err(|_| Error::UpstreamUnavailable)?;
            let mut form = vec![
                ("query", query.query.clone()),
                ("limit", query.limit.to_string()),
            ];
            if let Some(time) = query.time {
                form.push(("time", timestamp_parameter(time)));
            }
            if let (Some(start), Some(end), Some(direction)) =
                (query.start, query.end, query.direction)
            {
                form.extend([
                    ("start", timestamp_parameter(start)),
                    ("end", timestamp_parameter(end)),
                    ("direction", direction.as_str().to_owned()),
                ]);
            }
            let response = self
                .client
                .post(url)
                .bearer_auth(self.token.expose())
                .form(&form)
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
            let body = response
                .json::<Value>()
                .await
                .map_err(|_| Error::InvalidResponse)?;
            normalize(query.mode, query.limit, body)
        };
        let result = tokio::time::timeout(self.timeout, operation)
            .await
            .map_err(|_| Error::Timeout)?;
        drop(permit);
        result
    }
}

fn timestamp_parameter(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn normalize(mode: Mode, limit: u16, wrapper: Value) -> Result<Value, Error> {
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
        Form, Json, Router,
        extract::{OriginalUri, State},
        http::{HeaderMap, StatusCode},
        response::Redirect,
        routing::post,
    };
    use tokio::{net::TcpListener, task::JoinHandle};

    use crate::logql::{Direction, LogqlInput};

    use super::*;

    #[derive(Default)]
    struct RequestRecord {
        path: String,
        authorization: String,
        form: HashMap<String, String>,
    }

    async fn record_request(
        State(record): State<Arc<Mutex<RequestRecord>>>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
        Form(form): Form<HashMap<String, String>>,
    ) -> Json<Value> {
        *record.lock().unwrap() = RequestRecord {
            path: uri.path().to_owned(),
            authorization: headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
            form,
        };
        Json(json!({"status":"success","data":{"resultType":"scalar","result":[1786276800,"1"]}}))
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
            let normalized = normalize(Mode::Range, 5000, body).unwrap();
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
                normalize(Mode::Instant, 5000, body),
                Err(Error::InvalidResponse)
            );
        }
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

        let normalized = normalize(Mode::Range, 3, body).unwrap();
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
    async fn sends_only_bearer_auth_and_expected_instant_form() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                post(record_request),
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
        assert_eq!(record.form["query"], "{job=\"test\"}");
        assert_eq!(record.form["limit"], "12");
        assert_eq!(record.form["time"], "2026-08-09T12:00:00.000000000Z");
        assert!(!record.form.values().any(|value| value == "grafana-secret"));
        task.abort();
    }

    #[tokio::test]
    async fn sends_expected_range_path_and_parameters() {
        let record = Arc::new(Mutex::new(RequestRecord::default()));
        let router = Router::new()
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query_range",
                post(record_request),
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
        assert_eq!(record.form["start"], "2026-08-09T10:00:00.000000000Z");
        assert_eq!(record.form["end"], "2026-08-09T11:00:00.000000000Z");
        assert_eq!(record.form["direction"], "forward");
        assert!(!record.form.contains_key("time"));
        task.abort();
    }

    #[tokio::test]
    async fn refuses_redirects_without_forwarding_credentials() {
        let target_hit = Arc::new(AtomicBool::new(false));
        let target_probe = Arc::clone(&target_hit);
        let router = Router::new()
            .route(
                "/api/datasources/proxy/uid/loki/loki/api/v1/query",
                post(|| async { Redirect::temporary("/target") }),
            )
            .route(
                "/target",
                post(move || {
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
                post(move || async move { (status, "unsafe upstream detail") }),
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
            post(|| async {
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
            post(move || {
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
