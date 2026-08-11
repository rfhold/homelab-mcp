use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use reqwest::{Client, Method, StatusCode, Url, redirect::Policy};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};

use crate::config::Secret;

use super::{
    Error,
    actions::{MarkState, ScrubKind},
    normalize,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const API_V1: &str = "application/vnd.ceph.api.v1.0+json";
const API_V1_1: &str = "application/vnd.ceph.api.v1.1+json";

#[derive(Clone, Copy)]
enum Operation {
    Query,
    Mutation,
}

struct RequestOptions<'a> {
    query: &'a [(&'a str, String)],
    body: Option<Value>,
    accept: &'a str,
}

struct UpstreamResponse {
    status: StatusCode,
    body: Value,
}

pub(crate) type DispatchProgress = Arc<AtomicBool>;

pub(crate) fn new_dispatch_progress() -> DispatchProgress {
    Arc::new(AtomicBool::new(false))
}

#[derive(Clone)]
pub struct CephClient {
    origin: Url,
    username: Secret,
    password: Secret,
    http: Client,
    token: Arc<Mutex<Option<Secret>>>,
    permits: Arc<Semaphore>,
    timeout: Duration,
}

impl CephClient {
    pub fn new(origin: Url, username: Secret, password: Secret) -> Result<Self, String> {
        validate_origin(&origin, false)?;
        if username.expose().is_empty() || password.expose().is_empty() {
            return Err("Ceph Dashboard credentials must not be empty".into());
        }
        Self::build(origin, username, password, TIMEOUT)
    }

    fn build(
        origin: Url,
        username: Secret,
        password: Secret,
        timeout: Duration,
    ) -> Result<Self, String> {
        let http = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| "failed to initialize Ceph Dashboard client".to_owned())?;
        Ok(Self {
            origin,
            username,
            password,
            http,
            token: Arc::new(Mutex::new(None)),
            permits: Arc::new(Semaphore::new(4)),
            timeout,
        })
    }

    #[cfg(test)]
    pub(crate) fn disabled_for_test() -> Self {
        Self::for_test(
            Url::parse("http://127.0.0.1:9/").unwrap(),
            Duration::from_millis(100),
        )
    }

    #[cfg(test)]
    fn for_test(origin: Url, timeout: Duration) -> Self {
        validate_origin(&origin, true).unwrap();
        Self::build(
            origin,
            Secret::for_test("dashboard-user"),
            Secret::for_test("dashboard-password"),
            timeout,
        )
        .unwrap()
    }

    pub(crate) fn operation_timeout(&self) -> Duration {
        self.timeout
    }

    pub async fn status(&self, cluster: &str) -> Result<Value, Error> {
        normalize::status(
            cluster,
            &self
                .query(Method::GET, "api/health/minimal", &[], None, API_V1)
                .await?,
        )
    }

    pub async fn metrics_summary(&self, cluster: &str) -> Result<Value, Error> {
        normalize::metrics(
            cluster,
            &self
                .query(Method::GET, "api/health/minimal", &[], None, API_V1)
                .await?,
        )
    }

    pub async fn osds(&self, cluster: &str, limit: u16) -> Result<Value, Error> {
        let requested = u32::from(limit) + 1;
        let value = self
            .query(
                Method::GET,
                "api/osd",
                &[("offset", "0".into()), ("limit", requested.to_string())],
                None,
                API_V1_1,
            )
            .await?;
        normalize::osd_list(cluster, &value, limit)
    }

    pub async fn osd(&self, cluster: &str, osd_id: u32) -> Result<Value, Error> {
        let value = self
            .query(Method::GET, &format!("api/osd/{osd_id}"), &[], None, API_V1)
            .await?;
        normalize::osd(cluster, &value)
    }

    pub async fn safe_to_destroy(&self, cluster: &str, osd_id: u32) -> Result<Value, Error> {
        let ids = format!("[{osd_id}]");
        let value = self
            .query(
                Method::GET,
                "api/osd/safe_to_destroy",
                &[("ids", ids)],
                None,
                API_V1,
            )
            .await?;
        normalize::safe_to_destroy(cluster, osd_id, &value)
    }

    pub async fn devices(&self, cluster: &str, osd_id: u32, limit: u16) -> Result<Value, Error> {
        let value = self
            .query(
                Method::GET,
                &format!("api/osd/{osd_id}/devices"),
                &[],
                None,
                API_V1,
            )
            .await?;
        normalize::devices(cluster, osd_id, &value, limit)
    }

    pub async fn device(
        &self,
        cluster: &str,
        osd_id: u32,
        device_id: &str,
    ) -> Result<Value, Error> {
        let value = self
            .query(
                Method::GET,
                &format!("api/osd/{osd_id}/devices"),
                &[],
                None,
                API_V1,
            )
            .await?;
        normalize::device(cluster, osd_id, normalize::find_device(&value, device_id)?)
    }

    pub async fn flags(&self, cluster: &str) -> Result<Value, Error> {
        normalize::flags(
            cluster,
            &self
                .query(Method::GET, "api/osd/flags", &[], None, API_V1)
                .await?,
        )
    }

    pub async fn tasks(&self, cluster: &str, limit: u16) -> Result<Value, Error> {
        normalize::tasks(
            cluster,
            &self
                .query(Method::GET, "api/task", &[], None, API_V1)
                .await?,
            limit,
        )
    }

    pub(crate) async fn mark_with_progress(
        &self,
        cluster: &str,
        osd_id: u32,
        state: MarkState,
        progress: &DispatchProgress,
    ) -> Result<Value, Error> {
        let action = match state {
            MarkState::In => "in",
            MarkState::Out => "out",
            MarkState::Down => "down",
        };
        let response = self
            .mutate(
                Method::PUT,
                &format!("api/osd/{osd_id}/mark"),
                &[],
                Some(json!({"action":action})),
                progress,
            )
            .await?;
        if response.status != StatusCode::OK {
            return self.mutation_result(cluster, "osd.mark", Some(osd_id), response, None);
        }
        self.mutation_result(
            cluster,
            "osd.mark",
            Some(osd_id),
            response,
            self.read_back_osd(cluster, osd_id).await,
        )
    }

    pub(crate) async fn reweight_with_progress(
        &self,
        cluster: &str,
        osd_id: u32,
        weight: f64,
        progress: &DispatchProgress,
    ) -> Result<Value, Error> {
        let response = self
            .mutate(
                Method::POST,
                &format!("api/osd/{osd_id}/reweight"),
                &[],
                Some(json!({"weight":weight})),
                progress,
            )
            .await?;
        if response.status != StatusCode::OK {
            return self.mutation_result(cluster, "osd.reweight", Some(osd_id), response, None);
        }
        self.mutation_result(
            cluster,
            "osd.reweight",
            Some(osd_id),
            response,
            self.read_back_osd(cluster, osd_id).await,
        )
    }

    pub(crate) async fn scrub_with_progress(
        &self,
        cluster: &str,
        osd_id: u32,
        kind: ScrubKind,
        progress: &DispatchProgress,
    ) -> Result<Value, Error> {
        let deep = matches!(kind, ScrubKind::Deep).to_string();
        let response = self
            .mutate(
                Method::POST,
                &format!("api/osd/{osd_id}/scrub"),
                &[("deep", deep)],
                Some(Value::Null),
                progress,
            )
            .await?;
        self.mutation_result(cluster, "osd.scrub", Some(osd_id), response, None)
    }

    pub(crate) async fn destroy_with_progress(
        &self,
        cluster: &str,
        osd_id: u32,
        progress: &DispatchProgress,
    ) -> Result<Value, Error> {
        self.require_safe(cluster, osd_id).await?;
        let response = self
            .mutate(
                Method::POST,
                &format!("api/osd/{osd_id}/destroy"),
                &[],
                Some(Value::Null),
                progress,
            )
            .await?;
        self.mutation_result(cluster, "osd.destroy", Some(osd_id), response, None)
    }

    pub(crate) async fn purge_with_progress(
        &self,
        cluster: &str,
        osd_id: u32,
        progress: &DispatchProgress,
    ) -> Result<Value, Error> {
        self.require_safe(cluster, osd_id).await?;
        let response = self
            .mutate(
                Method::POST,
                &format!("api/osd/{osd_id}/purge"),
                &[],
                Some(Value::Null),
                progress,
            )
            .await?;
        self.mutation_result(cluster, "osd.purge", Some(osd_id), response, None)
    }

    async fn require_safe(&self, cluster: &str, osd_id: u32) -> Result<(), Error> {
        let result = self.safe_to_destroy(cluster, osd_id).await?;
        if result.get("safe_to_destroy").and_then(Value::as_bool) != Some(true) {
            return Err(Error::UnsafeToDestroy);
        }
        Ok(())
    }

    async fn read_back_osd(&self, cluster: &str, osd_id: u32) -> Option<Value> {
        self.osd(cluster, osd_id).await.ok()
    }

    fn mutation_result(
        &self,
        cluster: &str,
        action: &str,
        osd_id: Option<u32>,
        response: UpstreamResponse,
        read_back: Option<Value>,
    ) -> Result<Value, Error> {
        let mut result = match response.status {
            StatusCode::OK => json!({
                "cluster":cluster,"action":action,"osd_id":osd_id,"status":"completed",
            }),
            StatusCode::ACCEPTED => json!({
                "cluster":cluster,"action":action,"osd_id":osd_id,"status":"accepted",
                "task":normalize::mutation_task(&response.body, action, osd_id)
                    .map_err(|_| Error::MutationOutcomeUnknown)?,
            }),
            _ => return Err(Error::MutationOutcomeUnknown),
        };
        if let Some(read_back) = read_back {
            result["read_back"] = read_back;
        }
        Ok(result)
    }

    async fn query(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
        accept: &str,
    ) -> Result<Value, Error> {
        let mut token = self.authenticate().await?;
        let first = self
            .send(
                method.clone(),
                path,
                RequestOptions {
                    query,
                    body: body.clone(),
                    accept,
                },
                token.expose(),
                Operation::Query,
                None,
            )
            .await;
        match first {
            Err(Error::Unauthorized) => {}
            result => return result.map(|response| response.body),
        }
        {
            let mut cached = self.token.lock().await;
            if cached
                .as_ref()
                .is_some_and(|cached| cached.expose() == token.expose())
            {
                *cached = None;
            }
        }
        token = self.authenticate().await?;
        self.send(
            method,
            path,
            RequestOptions {
                query,
                body,
                accept,
            },
            token.expose(),
            Operation::Query,
            None,
        )
        .await
        .map(|response| response.body)
    }

    async fn mutate(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
        progress: &DispatchProgress,
    ) -> Result<UpstreamResponse, Error> {
        let token = self.authenticate().await?;
        self.send(
            method,
            path,
            RequestOptions {
                query,
                body,
                accept: API_V1,
            },
            token.expose(),
            Operation::Mutation,
            Some(progress),
        )
        .await
    }

    async fn authenticate(&self) -> Result<Secret, Error> {
        let mut cached = self.token.lock().await;
        if let Some(token) = cached.as_ref() {
            return Ok(token.clone());
        }
        let url = self.url("api/auth", &[])?;
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::CapacityExhausted)?;
        let operation = async {
            let response = self
                .http
                .post(url)
                .header("Accept", API_V1)
                .json(&json!({
                    "username":self.username.expose(),"password":self.password.expose()
                }))
                .send()
                .await
                .map_err(|_| Error::UpstreamUnavailable)?;
            if response.status() == StatusCode::UNAUTHORIZED
                || response.status() == StatusCode::FORBIDDEN
            {
                return Err(Error::Unauthorized);
            }
            if !response.status().is_success() {
                return Err(Error::UpstreamUnavailable);
            }
            let value = parse_bytes(
                read_limited(response, Operation::Query).await?,
                Operation::Query,
            )?;
            let token = value
                .get("token")
                .and_then(Value::as_str)
                .filter(|token| {
                    !token.is_empty()
                        && token.len() <= 16 * 1024
                        && !token.chars().any(char::is_control)
                })
                .ok_or(Error::InvalidResponse)?;
            Ok(Secret(token.to_owned()))
        };
        let token = tokio::time::timeout(self.timeout, operation)
            .await
            .map_err(|_| Error::Timeout)??;
        drop(permit);
        *cached = Some(token.clone());
        Ok(token)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        options: RequestOptions<'_>,
        token: &str,
        operation: Operation,
        dispatch_progress: Option<&DispatchProgress>,
    ) -> Result<UpstreamResponse, Error> {
        let url = self
            .url(path, options.query)
            .map_err(|_| failure(operation))?;
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::CapacityExhausted)?;
        let request = async {
            let mut request = self
                .http
                .request(method, url)
                .header("Accept", options.accept)
                .bearer_auth(token);
            if let Some(body) = options.body {
                request = request.json(&body);
            }
            if let Some(progress) = dispatch_progress {
                progress.store(true, Ordering::SeqCst);
            }
            let response = request.send().await.map_err(|_| failure(operation))?;
            let status = response.status();
            if status == StatusCode::UNAUTHORIZED {
                return Err(Error::Unauthorized);
            }
            if status == StatusCode::FORBIDDEN {
                return Err(match operation {
                    Operation::Query => Error::Unauthorized,
                    Operation::Mutation => Error::MutationRejected,
                });
            }
            if status == StatusCode::NOT_FOUND {
                return Err(Error::NotFound);
            }
            if !status.is_success() {
                return Err(match operation {
                    Operation::Query if status.is_client_error() => Error::QueryRejected,
                    Operation::Query => Error::UpstreamUnavailable,
                    Operation::Mutation if status.is_client_error() => Error::MutationRejected,
                    Operation::Mutation => Error::MutationOutcomeUnknown,
                });
            }
            let body = parse_bytes(read_limited(response, operation).await?, operation)?;
            Ok(UpstreamResponse { status, body })
        };
        let result =
            tokio::time::timeout(self.timeout, request)
                .await
                .map_err(|_| match operation {
                    Operation::Query => Error::Timeout,
                    Operation::Mutation => Error::MutationOutcomeUnknown,
                })?;
        drop(permit);
        result
    }

    fn url(&self, path: &str, query: &[(&str, String)]) -> Result<Url, Error> {
        let mut url = self
            .origin
            .join(path)
            .map_err(|_| Error::InvalidArguments)?;
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(key, value)| (*key, value)));
        }
        Ok(url)
    }
}

async fn read_limited(
    mut response: reqwest::Response,
    operation: Operation,
) -> Result<Vec<u8>, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(too_large(operation));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| failure(operation))? {
        if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
            return Err(too_large(operation));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn parse_bytes(bytes: Vec<u8>, operation: Operation) -> Result<Value, Error> {
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes).map_err(|_| match operation {
        Operation::Query => Error::InvalidResponse,
        Operation::Mutation => Error::MutationOutcomeUnknown,
    })
}

fn failure(operation: Operation) -> Error {
    match operation {
        Operation::Query => Error::UpstreamUnavailable,
        Operation::Mutation => Error::MutationOutcomeUnknown,
    }
}
fn too_large(operation: Operation) -> Error {
    match operation {
        Operation::Query => Error::ResponseTooLarge,
        Operation::Mutation => Error::MutationOutcomeUnknown,
    }
}

fn validate_origin(origin: &Url, allow_http: bool) -> Result<(), String> {
    if (!allow_http && origin.scheme() != "https")
        || (allow_http && !matches!(origin.scheme(), "http" | "https"))
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
        || origin.path() != "/"
    {
        return Err("invalid Ceph Dashboard origin".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        extract::State,
        http::Request,
        response::{IntoResponse, Response},
    };
    use std::{
        collections::VecDeque,
        sync::{
            Mutex as StdMutex,
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        },
    };
    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;

    type RecordedRequest = (String, String, Value, Option<String>);

    struct MockState {
        requests: StdMutex<Vec<RecordedRequest>>,
        auth_count: AtomicUsize,
        unauthorized_once: AtomicBool,
        unsafe_destroy: AtomicBool,
        stall_mutation: AtomicBool,
        oversized_health: AtomicBool,
        request_delay_millis: AtomicU64,
        auth_delay_millis: AtomicU64,
        safe_to_destroy_delay_millis: AtomicU64,
        task_response: StdMutex<Value>,
        mutation_responses: StdMutex<VecDeque<(StatusCode, Value)>>,
    }

    async fn handler(State(state): State<Arc<MockState>>, request: Request<Body>) -> Response {
        let method = request.method().to_string();
        let uri = request.uri().to_string();
        let auth = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = to_bytes(request.into_body(), MAX_RESPONSE_BYTES + 1)
            .await
            .unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        state
            .requests
            .lock()
            .unwrap()
            .push((method.clone(), uri.clone(), body.clone(), auth));
        let delay = state.request_delay_millis.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if uri == "/api/auth" {
            state.auth_count.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                body,
                json!({"username":"dashboard-user","password":"dashboard-password"})
            );
            let delay = state.auth_delay_millis.load(Ordering::SeqCst);
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            return (
                StatusCode::CREATED,
                axum::Json(json!({"token":"eyJ-secret-token"})),
            )
                .into_response();
        }
        if state.unauthorized_once.swap(false, Ordering::SeqCst) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if state.stall_mutation.load(Ordering::SeqCst) && method != "GET" {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        if method != "GET"
            && let Some((status, body)) = state.mutation_responses.lock().unwrap().pop_front()
        {
            return (status, axum::Json(body)).into_response();
        }
        match (method.as_str(), request_path(&uri)) {
            ("GET", "/api/health/minimal") if state.oversized_health.load(Ordering::SeqCst) => {
                axum::Json(json!({"padding":"x".repeat(MAX_RESPONSE_BYTES)})).into_response()
            }
            ("GET", "/api/health/minimal") => axum::Json(health()).into_response(),
            ("GET", "/api/osd") => axum::Json(json!([{"id":1,"up":1,"in":1,"state":["exists","up"]},{"id":2,"up":0,"in":1,"state":["exists"]}])).into_response(),
            ("GET", "/api/osd/1") => axum::Json(json!({"osd_map":{"id":1,"up":1,"in":1,"state":["exists","up"],"weight":0.5}})).into_response(),
            ("GET", "/api/osd/safe_to_destroy") => {
                let delay = state.safe_to_destroy_delay_millis.load(Ordering::SeqCst);
                if delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                axum::Json(json!({"is_safe_to_destroy":!state.unsafe_destroy.load(Ordering::SeqCst),"active":[],"missing_stats":[],"stored_pgs":[]})).into_response()
            }
            ("GET", "/api/osd/1/devices") => axum::Json(json!([{"devid":"dev-a","daemons":["osd.1"]},{"devid":"dev-b","daemons":["osd.1"]}])).into_response(),
            ("GET", "/api/osd/flags") => axum::Json(json!(["sortbitwise","noout"])).into_response(),
            ("GET", "/api/task") => {
                axum::Json(state.task_response.lock().unwrap().clone()).into_response()
            }
            _ => axum::Json(Value::Null).into_response(),
        }
    }

    fn request_path(uri: &str) -> &str {
        uri.split('?').next().unwrap()
    }

    fn health() -> Value {
        json!({
            "health":{"status":"HEALTH_OK","checks":[]},
            "df":{"stats":{"total_bytes":100,"total_avail_bytes":60,"total_used_raw_bytes":40}},
            "client_perf":{"read_bytes_sec":1,"write_bytes_sec":2,"read_op_per_sec":3,"write_op_per_sec":4,"recovering_bytes_per_sec":5},
            "pg_info":{"object_stats":{"num_objects":10,"num_objects_degraded":0,"num_objects_misplaced":0,"num_objects_unfound":0}},
            "osd_map":{"osds":[{"up":1,"in":1}]}
        })
    }

    async fn mock() -> (CephClient, Arc<MockState>, JoinHandle<()>) {
        let state = Arc::new(MockState {
            requests: StdMutex::new(Vec::new()),
            auth_count: AtomicUsize::new(0),
            unauthorized_once: AtomicBool::new(false),
            unsafe_destroy: AtomicBool::new(false),
            stall_mutation: AtomicBool::new(false),
            oversized_health: AtomicBool::new(false),
            request_delay_millis: AtomicU64::new(0),
            auth_delay_millis: AtomicU64::new(0),
            safe_to_destroy_delay_millis: AtomicU64::new(0),
            task_response: StdMutex::new(json!({"executing_tasks":[],"finished_tasks":[]})),
            mutation_responses: StdMutex::new(VecDeque::new()),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(handler).with_state(server_state),
            )
            .await
            .unwrap();
        });
        (
            CephClient::for_test(origin, Duration::from_secs(2)),
            state,
            server,
        )
    }

    #[test]
    fn production_origins_are_strict() {
        for origin in [
            "http://ceph.example/",
            "https://user:pass@ceph.example/",
            "https://ceph.example/api",
            "https://ceph.example/?x=1",
        ] {
            assert!(
                validate_origin(&Url::parse(origin).unwrap(), false).is_err(),
                "accepted {origin}"
            );
        }
        assert!(validate_origin(&Url::parse("https://ceph.example:8443/").unwrap(), false).is_ok());
    }

    #[tokio::test]
    async fn auth_is_cached_and_queries_reauthenticate_once_after_401() {
        let (client, state, server) = mock().await;
        client.status("romulus").await.unwrap();
        client.metrics_summary("romulus").await.unwrap();
        assert_eq!(state.auth_count.load(Ordering::SeqCst), 1);
        state.unauthorized_once.store(true, Ordering::SeqCst);
        client.status("romulus").await.unwrap();
        assert_eq!(state.auth_count.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn paths_bodies_bounds_and_bearer_are_exact() {
        let (client, state, server) = mock().await;
        let list = client.osds("romulus", 1).await.unwrap();
        assert_eq!(list["truncated"], true);
        let mark = client
            .mark_with_progress("romulus", 1, MarkState::Out, &new_dispatch_progress())
            .await
            .unwrap();
        assert_eq!(mark["status"], "completed");
        assert!(mark.get("task").is_none());
        assert_eq!(mark["read_back"]["osd_id"], 1);
        client
            .reweight_with_progress("romulus", 1, 0.5, &new_dispatch_progress())
            .await
            .unwrap();
        client
            .scrub_with_progress("romulus", 1, ScrubKind::Deep, &new_dispatch_progress())
            .await
            .unwrap();
        let requests = state.requests.lock().unwrap();
        assert!(
            requests
                .iter()
                .any(|(method, uri, body, auth)| method == "GET"
                    && uri == "/api/osd?offset=0&limit=2"
                    && body.is_null()
                    && auth.as_deref() == Some("Bearer eyJ-secret-token"))
        );
        assert!(requests.iter().any(|(method, uri, body, _)| method == "PUT"
            && uri == "/api/osd/1/mark"
            && body == &json!({"action":"out"})));
        assert!(
            requests
                .iter()
                .any(|(method, uri, body, _)| method == "POST"
                    && uri == "/api/osd/1/reweight"
                    && body == &json!({"weight":0.5}))
        );
        assert!(
            requests
                .iter()
                .any(|(method, uri, body, _)| method == "POST"
                    && uri == "/api/osd/1/scrub?deep=true"
                    && body.is_null())
        );
        assert!(
            !requests
                .iter()
                .any(|(method, uri, _, _)| method == "PUT" && uri == "/api/osd/flags")
        );
        server.abort();
    }

    #[tokio::test]
    async fn destructive_actions_preflight_and_never_send_force() {
        let (client, state, server) = mock().await;
        state.unsafe_destroy.store(true, Ordering::SeqCst);
        assert_eq!(
            client
                .destroy_with_progress("romulus", 1, &new_dispatch_progress())
                .await
                .unwrap_err(),
            Error::UnsafeToDestroy
        );
        assert!(
            !state
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|(method, uri, _, _)| method == "POST" && uri == "/api/osd/1/destroy")
        );
        state.unsafe_destroy.store(false, Ordering::SeqCst);
        client
            .purge_with_progress("romulus", 1, &new_dispatch_progress())
            .await
            .unwrap();
        let requests = state.requests.lock().unwrap();
        assert!(requests.iter().any(|(method, uri, body, _)| method == "GET"
            && uri == "/api/osd/safe_to_destroy?ids=%5B1%5D"
            && body.is_null()));
        assert!(
            requests
                .iter()
                .any(|(method, uri, body, _)| method == "POST"
                    && uri == "/api/osd/1/purge"
                    && body.is_null())
        );
        assert!(
            !requests
                .iter()
                .any(|(_, uri, body, _)| uri.contains("force") || body.get("force").is_some())
        );
        server.abort();
    }

    #[tokio::test]
    async fn mutations_are_not_retried_and_unknown_outcomes_are_nonretryable() {
        let (mut client, state, server) = mock().await;
        state.unauthorized_once.store(true, Ordering::SeqCst);
        assert_eq!(
            client
                .mark_with_progress("romulus", 1, MarkState::Down, &new_dispatch_progress(),)
                .await
                .unwrap_err(),
            Error::Unauthorized
        );
        assert_eq!(
            state
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, uri, _, _)| uri == "/api/osd/1/mark")
                .count(),
            1
        );
        client.timeout = Duration::from_millis(20);
        state.stall_mutation.store(true, Ordering::SeqCst);
        assert_eq!(
            client
                .scrub_with_progress("romulus", 1, ScrubKind::Normal, &new_dispatch_progress(),)
                .await
                .unwrap_err(),
            Error::MutationOutcomeUnknown
        );
        assert_eq!(
            state
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, uri, _, _)| uri.starts_with("/api/osd/1/scrub"))
                .count(),
            1
        );
        let tool = Error::MutationOutcomeUnknown
            .into_tool_error("mutation")
            .into_mcp_result()
            .raw;
        assert_eq!(tool["structuredContent"]["error"]["retryable"], false);
        server.abort();
    }

    #[tokio::test]
    async fn responses_are_capped_and_client_rejections_are_definite() {
        let (client, state, server) = mock().await;
        state.oversized_health.store(true, Ordering::SeqCst);
        assert_eq!(
            client.status("romulus").await.unwrap_err(),
            Error::ResponseTooLarge
        );
        state.oversized_health.store(false, Ordering::SeqCst);
        state
            .mutation_responses
            .lock()
            .unwrap()
            .push_back((StatusCode::BAD_REQUEST, Value::Null));
        assert_eq!(
            client
                .mark_with_progress("romulus", 1, MarkState::In, &new_dispatch_progress())
                .await
                .unwrap_err(),
            Error::MutationRejected
        );
        assert_eq!(
            state
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, uri, _, _)| uri == "/api/osd/1/mark")
                .count(),
            1
        );
        server.abort();
    }

    #[tokio::test]
    async fn synchronous_success_and_async_identity_have_distinct_outcomes() {
        let (client, state, server) = mock().await;
        state
            .mutation_responses
            .lock()
            .unwrap()
            .push_back((StatusCode::OK, Value::Null));
        let result = client
            .scrub_with_progress("romulus", 1, ScrubKind::Normal, &new_dispatch_progress())
            .await
            .unwrap();
        assert_eq!(result["status"], "completed");
        assert!(result.get("task").is_none());

        for status in [
            StatusCode::CREATED,
            StatusCode::NO_CONTENT,
            StatusCode::PARTIAL_CONTENT,
            StatusCode::MULTI_STATUS,
            StatusCode::IM_USED,
        ] {
            state
                .mutation_responses
                .lock()
                .unwrap()
                .push_back((status, Value::Null));
            assert_eq!(
                client
                    .scrub_with_progress("romulus", 1, ScrubKind::Normal, &new_dispatch_progress(),)
                    .await,
                Err(Error::MutationOutcomeUnknown),
                "accepted unexpected synchronous status {status}"
            );
        }

        state
            .mutation_responses
            .lock()
            .unwrap()
            .push_back((StatusCode::ACCEPTED, Value::Null));
        assert_eq!(
            client
                .scrub_with_progress("romulus", 1, ScrubKind::Normal, &new_dispatch_progress(),)
                .await,
            Err(Error::MutationOutcomeUnknown)
        );

        state.mutation_responses.lock().unwrap().push_back((
            StatusCode::ACCEPTED,
            json!({"name":"osd/delete","metadata":{"svc_id":1,"token":"eyJ-secret"}}),
        ));
        let result = client
            .destroy_with_progress("romulus", 1, &new_dispatch_progress())
            .await
            .unwrap();
        assert_eq!(result["status"], "accepted");
        assert_eq!(
            result["task"],
            json!({"name":"osd/delete","metadata":{"svc_id":1}})
        );
        assert!(!result.to_string().contains("eyJ-secret"));

        for body in [
            json!({}),
            json!({"name":"osd/delete","metadata":{"svc_id":"not-a-number"}}),
            json!({"name":"osd/delete","metadata":{"svc_id":2}}),
            json!({"name":"unknown","metadata":{"svc_id":1}}),
        ] {
            state
                .mutation_responses
                .lock()
                .unwrap()
                .push_back((StatusCode::ACCEPTED, body));
            assert_eq!(
                client
                    .purge_with_progress("romulus", 1, &new_dispatch_progress())
                    .await,
                Err(Error::MutationOutcomeUnknown)
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn task_query_rejects_malformed_allowlisted_identity_without_leaking_it() {
        let (client, state, server) = mock().await;
        *state.task_response.lock().unwrap() = json!({
            "executing_tasks":[{
                "name":"osd/delete",
                "metadata":{"svc_id":"1.5","password":"dashboard-password"},
                "exception":"eyJ-secret-token"
            }],
            "finished_tasks":[]
        });

        let error = client.tasks("romulus", 10).await.unwrap_err();
        assert_eq!(error, Error::InvalidResponse);
        let result = error.into_tool_error("task").into_mcp_result().raw;
        let output = result.to_string();
        assert!(!output.contains("dashboard-password"));
        assert!(!output.contains("eyJ-secret-token"));
        server.abort();
    }

    #[tokio::test]
    async fn catalog_deadline_caps_complete_query_future() {
        use crate::integrations::ceph::CephCatalog;

        let (mut client, state, server) = mock().await;
        client.timeout = Duration::from_millis(25);
        state.request_delay_millis.store(15, Ordering::SeqCst);
        let catalog = CephCatalog::new([("romulus".into(), client)]).unwrap();
        assert_eq!(
            catalog
                .query(&super::super::actions::QueryCommand::StatusGet {
                    cluster: "romulus".into(),
                })
                .await,
            Err(Error::Timeout)
        );
        server.abort();
    }

    #[tokio::test]
    async fn execution_deadline_distinguishes_auth_preflight_and_post_dispatch_delays() {
        use crate::integrations::ceph::{CephCatalog, actions::ExecCommand};

        let (mut client, state, server) = mock().await;
        client.timeout = Duration::from_millis(25);
        state.auth_delay_millis.store(100, Ordering::SeqCst);
        let permit_observer = client.clone();
        let catalog = CephCatalog::new([("romulus".into(), client)]).unwrap();
        assert_eq!(
            catalog
                .execute(&ExecCommand::Mark {
                    cluster: "romulus".into(),
                    osd_id: 1,
                    state: MarkState::Down,
                })
                .await,
            Err(Error::Timeout)
        );
        assert_eq!(permit_observer.permits.available_permits(), 4);
        assert!(
            !state
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|(_, uri, _, _)| uri == "/api/osd/1/mark")
        );
        server.abort();

        let (mut client, state, server) = mock().await;
        client.status("romulus").await.unwrap();
        client.timeout = Duration::from_millis(25);
        state
            .safe_to_destroy_delay_millis
            .store(100, Ordering::SeqCst);
        let permit_observer = client.clone();
        let catalog = CephCatalog::new([("romulus".into(), client)]).unwrap();
        assert_eq!(
            catalog
                .execute(&ExecCommand::Destroy {
                    cluster: "romulus".into(),
                    osd_id: 1,
                })
                .await,
            Err(Error::Timeout)
        );
        assert_eq!(permit_observer.permits.available_permits(), 4);
        assert!(
            !state
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|(method, uri, _, _)| method == "POST" && uri == "/api/osd/1/destroy")
        );
        server.abort();

        let (mut client, state, server) = mock().await;
        client.status("romulus").await.unwrap();
        client.timeout = Duration::from_millis(25);
        state.stall_mutation.store(true, Ordering::SeqCst);
        let permit_observer = client.clone();
        let catalog = CephCatalog::new([("romulus".into(), client)]).unwrap();
        assert_eq!(
            catalog
                .execute(&ExecCommand::Scrub {
                    cluster: "romulus".into(),
                    osd_id: 1,
                    kind: ScrubKind::Normal,
                })
                .await,
            Err(Error::MutationOutcomeUnknown)
        );
        assert_eq!(permit_observer.permits.available_permits(), 4);
        assert!(
            state
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|(method, uri, _, _)| method == "POST" && uri.starts_with("/api/osd/1/scrub"))
        );
        server.abort();
    }
}
