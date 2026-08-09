use std::sync::Arc;

use axum::{Router, extract::State, http::StatusCode, routing::get};
use mcp::server::BoxFuture;

pub trait ReadinessCheck: Send + Sync {
    fn check(&self) -> BoxFuture<bool>;
}

pub fn router(
    readiness: Arc<dyn ReadinessCheck>,
    oauth: Router,
    oidc: Router,
    mcp: Router,
) -> Router {
    Router::new()
        .route("/health", get(healthy))
        .route("/ready", get(ready))
        .with_state(readiness)
        .merge(oauth)
        .merge(oidc)
        .merge(mcp)
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use tower::ServiceExt as _;

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
}
