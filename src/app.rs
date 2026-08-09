use std::{sync::Arc, time::Instant};

use axum::{
    Router,
    body::Body,
    extract::{MatchedPath, State},
    http::{HeaderMap, Method, Request, StatusCode},
    middleware::{Next, from_fn},
    response::Response,
    routing::get,
};
use mcp::server::BoxFuture;
use opentelemetry::{KeyValue, global, propagation::Extractor};
use tracing::Instrument as _;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

#[derive(Clone)]
struct HttpMetrics {
    requests: opentelemetry::metrics::Counter<u64>,
    active_requests: opentelemetry::metrics::UpDownCounter<i64>,
    duration: opentelemetry::metrics::Histogram<f64>,
    #[cfg(test)]
    active_balance: Arc<std::sync::atomic::AtomicI64>,
    #[cfg(test)]
    active_updates: Arc<std::sync::atomic::AtomicU64>,
    #[cfg(test)]
    completed_requests: Arc<std::sync::atomic::AtomicU64>,
}

impl HttpMetrics {
    fn new() -> Self {
        let meter = global::meter("homelab-mcp.http");
        Self {
            requests: meter
                .u64_counter("http.server.request.count")
                .with_description("Completed HTTP server requests")
                .build(),
            active_requests: meter
                .i64_up_down_counter("http.server.active_requests")
                .with_description("Active HTTP server requests")
                .build(),
            duration: meter
                .f64_histogram("http.server.request.duration")
                .with_unit("s")
                .with_description("HTTP server request duration")
                .build(),
            #[cfg(test)]
            active_balance: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            #[cfg(test)]
            active_updates: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            #[cfg(test)]
            completed_requests: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    fn add_active(&self, value: i64, attributes: &[KeyValue]) {
        self.active_requests.add(value, attributes);
        #[cfg(test)]
        self.active_balance
            .fetch_add(value, std::sync::atomic::Ordering::Relaxed);
        #[cfg(test)]
        self.active_updates
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[cfg(test)]
    fn active_balance(&self) -> i64 {
        self.active_balance
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    fn completed_requests(&self) -> u64 {
        self.completed_requests
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    fn active_updates(&self) -> u64 {
        self.active_updates
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

struct RequestMetricsGuard {
    metrics: HttpMetrics,
    span: tracing::Span,
    method: String,
    route: String,
    started: Instant,
    finished: bool,
}

impl RequestMetricsGuard {
    fn new(metrics: HttpMetrics, span: tracing::Span, method: String, route: String) -> Self {
        metrics.add_active(
            1,
            &[
                KeyValue::new("http.request.method", method.clone()),
                KeyValue::new("http.route", route.clone()),
            ],
        );
        Self {
            metrics,
            span,
            method,
            route,
            started: Instant::now(),
            finished: false,
        }
    }

    fn complete(&mut self, status: StatusCode) {
        let outcome = if status.is_server_error() {
            "error"
        } else {
            "success"
        };
        self.finish(Some(status), outcome);
    }

    fn finish(&mut self, status: Option<StatusCode>, outcome: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        let elapsed = self.started.elapsed().as_secs_f64();
        self.span.record("http.outcome", outcome);
        if let Some(status) = status {
            self.span
                .record("http.response.status_code", status.as_u16());
            if status.is_server_error() {
                self.span.record("otel.status_code", "ERROR");
            }
        } else {
            self.span.record("otel.status_code", "ERROR");
        }

        let active_attributes = [
            KeyValue::new("http.request.method", self.method.clone()),
            KeyValue::new("http.route", self.route.clone()),
        ];
        let mut completed_attributes = vec![
            KeyValue::new("http.request.method", self.method.clone()),
            KeyValue::new("http.route", self.route.clone()),
            KeyValue::new("http.outcome", outcome),
        ];
        if let Some(status) = status {
            completed_attributes.push(KeyValue::new(
                "http.response.status_code",
                i64::from(status.as_u16()),
            ));
        }
        self.metrics.add_active(-1, &active_attributes);
        self.metrics.requests.add(1, &completed_attributes);
        #[cfg(test)]
        self.metrics
            .completed_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.metrics.duration.record(elapsed, &completed_attributes);
        emit_request_completion(
            &self.span,
            &self.method,
            &self.route,
            status,
            outcome,
            elapsed,
        );
    }
}

impl Drop for RequestMetricsGuard {
    fn drop(&mut self) {
        self.finish(None, "cancelled");
    }
}

pub trait ReadinessCheck: Send + Sync {
    fn check(&self) -> BoxFuture<bool>;
}

pub fn router(
    readiness: Arc<dyn ReadinessCheck>,
    oauth: Router,
    oidc: Router,
    mcp: Router,
) -> Router {
    router_with_metrics(readiness, oauth, oidc, mcp, HttpMetrics::new())
}

fn router_with_metrics(
    readiness: Arc<dyn ReadinessCheck>,
    oauth: Router,
    oidc: Router,
    mcp: Router,
    metrics: HttpMetrics,
) -> Router {
    Router::new()
        .route("/health", get(healthy))
        .route("/ready", get(ready))
        .with_state(readiness)
        .merge(oauth)
        .merge(oidc)
        .merge(mcp)
        .layer(from_fn(move |request, next| {
            instrument_request(request, next, metrics.clone())
        }))
}

async fn instrument_request(request: Request<Body>, next: Next, metrics: HttpMetrics) -> Response {
    if matches!(request.uri().path(), "/health" | "/ready") {
        return next.run(request).await;
    }

    let method = bounded_http_method(request.method()).to_owned();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or_else(|| stable_path_class(request.uri().path()))
        .to_owned();
    let parent = global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(request.headers()))
    });
    let span = tracing::info_span!(
        "http.server.request",
        http.request.method = %method,
        http.route = %route,
        http.response.status_code = tracing::field::Empty,
        http.outcome = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    );
    let _ = span.set_parent(parent);
    let mut metrics_guard = RequestMetricsGuard::new(metrics, span.clone(), method, route);
    let response = next.run(request).instrument(span.clone()).await;
    metrics_guard.complete(response.status());
    response
}

fn bounded_http_method(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        Method::CONNECT => "CONNECT",
        Method::TRACE => "TRACE",
        _ => "OTHER",
    }
}

fn emit_request_completion(
    span: &tracing::Span,
    method: &str,
    route: &str,
    status: Option<StatusCode>,
    outcome: &str,
    duration_seconds: f64,
) {
    let _entered = span.enter();
    if let Some(status) = status {
        tracing::info!(
            http.request.method = method,
            http.route = route,
            http.response.status_code = status.as_u16(),
            http.outcome = outcome,
            duration_seconds,
            "http request completed"
        );
    } else {
        tracing::info!(
            http.request.method = method,
            http.route = route,
            http.outcome = outcome,
            duration_seconds,
            "http request completed"
        );
    }
}

fn stable_path_class(path: &str) -> &'static str {
    match path {
        "/health" => "/health",
        "/ready" => "/ready",
        "/mcp" => "/mcp",
        _ if path.starts_with("/.well-known/") => "/.well-known/*",
        _ if path.starts_with("/oauth/") => "/oauth/*",
        _ if path.starts_with("/oidc/") => "/oidc/*",
        _ => "unmatched",
    }
}

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(axum::http::HeaderName::as_str).collect()
    }
}

async fn healthy() -> StatusCode {
    StatusCode::OK
}

async fn ready(State(readiness): State<Arc<dyn ReadinessCheck>>) -> StatusCode {
    if readiness.check().await {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use std::{
        collections::BTreeMap,
        fmt,
        sync::{
            Mutex,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
    };
    use tower::ServiceExt as _;
    use tracing::{Event, Subscriber, field::Visit};
    use tracing_subscriber::{
        Layer, layer::Context, layer::SubscriberExt as _, registry::LookupSpan,
    };

    struct TestReadiness(Arc<AtomicBool>);

    impl ReadinessCheck for TestReadiness {
        fn check(&self) -> BoxFuture<bool> {
            let ready = self.0.load(Ordering::Acquire);
            Box::pin(async move { ready })
        }
    }

    #[tokio::test]
    async fn health_is_unconditional_and_readiness_reflects_state() {
        let current = Arc::new(AtomicBool::new(false));
        let readiness: Arc<dyn ReadinessCheck> = Arc::new(TestReadiness(current.clone()));
        let router = Router::new()
            .route("/health", get(healthy))
            .route("/ready", get(ready))
            .with_state(readiness);

        let health = router
            .clone()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let ready = router
            .clone()
            .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);

        current.store(true, Ordering::Release);
        let ready = router
            .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(ready.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn probes_bypass_telemetry_while_normal_routes_are_instrumented() {
        let current = Arc::new(AtomicBool::new(false));
        let readiness: Arc<dyn ReadinessCheck> = Arc::new(TestReadiness(current));
        let metrics = HttpMetrics::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let request_spans = Arc::new(AtomicU64::new(0));
        let subscriber = tracing_subscriber::registry()
            .with(EventCapture(events.clone()))
            .with(RequestSpanCapture(request_spans.clone()));
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let router = router_with_metrics(
            readiness,
            Router::new().route("/normal", get(|| async { StatusCode::NO_CONTENT })),
            Router::new(),
            Router::new(),
            metrics.clone(),
        );

        for (path, expected) in [
            ("/health", StatusCode::OK),
            ("/ready", StatusCode::SERVICE_UNAVAILABLE),
        ] {
            let response = router
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }

        assert_eq!(request_spans.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.completed_requests(), 0);
        assert_eq!(metrics.active_updates(), 0);
        assert!(events.lock().unwrap().is_empty());

        let response = router
            .oneshot(Request::get("/normal").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(request_spans.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.completed_requests(), 1);
        assert_eq!(metrics.active_updates(), 2);
        assert_eq!(metrics.active_balance(), 0);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].fields["http.route"], "/normal");
        assert!(events[0].in_request_span);
    }

    #[test]
    fn fallback_path_classes_do_not_include_queries_or_identifiers() {
        assert_eq!(stable_path_class("/oauth/authorize"), "/oauth/*");
        assert_eq!(stable_path_class("/oidc/callback"), "/oidc/*");
        assert_eq!(stable_path_class("/unknown/user-123"), "unmatched");
    }

    #[test]
    fn http_methods_are_bounded_before_telemetry() {
        for (method, expected) in [
            (Method::GET, "GET"),
            (Method::HEAD, "HEAD"),
            (Method::POST, "POST"),
            (Method::PUT, "PUT"),
            (Method::PATCH, "PATCH"),
            (Method::DELETE, "DELETE"),
            (Method::OPTIONS, "OPTIONS"),
            (Method::CONNECT, "CONNECT"),
            (Method::TRACE, "TRACE"),
            (Method::from_bytes(b"CUSTOM").unwrap(), "OTHER"),
        ] {
            assert_eq!(bounded_http_method(&method), expected);
        }
    }

    #[test]
    fn successful_request_metrics_finalize_once() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(EventCapture(events.clone()));
        let metrics = HttpMetrics::new();
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("http.server.request");
            let mut guard = RequestMetricsGuard::new(
                metrics.clone(),
                span,
                "GET".to_owned(),
                "/oauth/*".to_owned(),
            );
            assert_eq!(metrics.active_balance(), 1);
            guard.complete(StatusCode::OK);
            guard.complete(StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(metrics.active_balance(), 0);
        });

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].in_request_span);
        assert_eq!(events[0].fields["http.request.method"], "GET");
        assert_eq!(events[0].fields["http.route"], "/oauth/*");
        assert_eq!(events[0].fields["http.response.status_code"], "200");
        assert_eq!(events[0].fields["http.outcome"], "success");
        assert!(events[0].fields.contains_key("duration_seconds"));
        for forbidden in ["query", "identifier", "headers", "body"] {
            assert!(!events[0].fields.contains_key(forbidden));
        }
    }

    #[test]
    fn dropped_request_metrics_finalize_once_as_cancelled() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(EventCapture(events.clone()));
        let metrics = HttpMetrics::new();
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("http.server.request");
            let guard = RequestMetricsGuard::new(
                metrics.clone(),
                span,
                "POST".to_owned(),
                "/mcp".to_owned(),
            );
            assert_eq!(metrics.active_balance(), 1);
            drop(guard);
            assert_eq!(metrics.active_balance(), 0);
        });

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].in_request_span);
        assert_eq!(events[0].fields["http.request.method"], "POST");
        assert_eq!(events[0].fields["http.route"], "/mcp");
        assert_eq!(events[0].fields["http.outcome"], "cancelled");
        assert!(events[0].fields.contains_key("duration_seconds"));
        assert!(!events[0].fields.contains_key("http.response.status_code"));
    }

    struct CapturedEvent {
        fields: BTreeMap<String, String>,
        in_request_span: bool,
    }

    type CapturedEvents = Arc<Mutex<Vec<CapturedEvent>>>;

    struct EventCapture(CapturedEvents);

    impl<S> Layer<S> for EventCapture
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
            let mut fields = CapturedFields::default();
            event.record(&mut fields);
            let in_request_span = context.event_scope(event).is_some_and(|scope| {
                scope
                    .from_root()
                    .any(|span| span.metadata().name() == "http.server.request")
            });
            self.0.lock().unwrap().push(CapturedEvent {
                fields: fields.0,
                in_request_span,
            });
        }
    }

    struct RequestSpanCapture(Arc<AtomicU64>);

    impl<S> Layer<S> for RequestSpanCapture
    where
        S: Subscriber,
    {
        fn on_new_span(
            &self,
            attributes: &tracing::span::Attributes<'_>,
            _id: &tracing::span::Id,
            _context: Context<'_, S>,
        ) {
            if attributes.metadata().name() == "http.server.request" {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    #[derive(Default)]
    struct CapturedFields(BTreeMap<String, String>);

    impl Visit for CapturedFields {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
            self.0.insert(
                field.name().to_owned(),
                format!("{value:?}").trim_matches('"').to_owned(),
            );
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.insert(field.name().to_owned(), value.to_owned());
        }

        fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
            self.0.insert(field.name().to_owned(), value.to_string());
        }

        fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
            self.0.insert(field.name().to_owned(), value.to_string());
        }
    }
}
