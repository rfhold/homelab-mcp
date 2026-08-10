use http::Extensions;
use reqwest::{Method, Request, Response, StatusCode};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Result};
use reqwest_tracing::{ReqwestOtelSpanBackend, TracingMiddleware};
use tracing::Span;

pub(crate) fn traced_client(client: reqwest::Client) -> ClientWithMiddleware {
    ClientBuilder::new(client)
        .with(TracingMiddleware::<ApplicationSpanBackend>::new())
        .build()
}

struct ApplicationSpanBackend;

#[derive(Clone)]
struct ApplicationSpan(Span);

pub(crate) struct ClientRequestSpanGuard {
    span: Span,
    finished: bool,
}

impl ClientRequestSpanGuard {
    pub(crate) fn new(method: &Method) -> Self {
        Self {
            span: tracing::info_span!(
                target: "homelab_mcp::http_client",
                "http.client.request",
                otel.kind = "client",
                http.request.method = %method,
                http.response.status_code = tracing::field::Empty,
                http.outcome = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            ),
            finished: false,
        }
    }

    pub(crate) fn attach(
        &self,
        request: reqwest_middleware::RequestBuilder,
    ) -> reqwest_middleware::RequestBuilder {
        request.with_extension(ApplicationSpan(self.span.clone()))
    }

    pub(crate) fn record_status(&self, status: StatusCode) {
        self.span
            .record("http.response.status_code", u64::from(status.as_u16()));
    }

    pub(crate) fn finish_success(&mut self) {
        self.finish("success", false);
    }

    pub(crate) fn finish_http_error(&mut self, status: StatusCode) {
        self.finish(
            "http_error",
            status.is_client_error() || status.is_server_error(),
        );
    }

    pub(crate) fn finish_transport_error(&mut self) {
        self.finish("transport_error", true);
    }

    pub(crate) fn finish_response_error(&mut self) {
        self.finish("response_error", true);
    }

    fn finish(&mut self, outcome: &'static str, error: bool) {
        if self.finished {
            return;
        }
        self.span.record("http.outcome", outcome);
        if error {
            self.span.record("otel.status_code", "ERROR");
        }
        self.finished = true;
    }
}

impl Drop for ClientRequestSpanGuard {
    fn drop(&mut self) {
        self.finish("cancelled", true);
    }
}

impl ReqwestOtelSpanBackend for ApplicationSpanBackend {
    fn on_request_start(_request: &Request, extensions: &mut Extensions) -> Span {
        extensions
            .get::<ApplicationSpan>()
            .expect("Grafana requests must carry their application span")
            .0
            .clone()
    }

    fn on_request_end(_span: &Span, _outcome: &Result<Response>, _extensions: &mut Extensions) {}
}
