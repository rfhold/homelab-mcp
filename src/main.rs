use axum::{Router, http::StatusCode, routing::get};
use tokio::net::TcpListener;

const LISTEN_ADDR: &str = "0.0.0.0:14333";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind(LISTEN_ADDR).await?;

    axum::serve(listener, router())
        .with_graceful_shutdown(shutdown_signal())
        .await
}

fn router() -> Router {
    Router::new()
        .route("/health", get(healthy))
        .route("/ready", get(healthy))
}

async fn healthy() -> StatusCode {
    StatusCode::OK
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_routes_are_available() {
        for path in ["/health", "/ready"] {
            let response = router()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK, "path: {path}");
        }
    }
}
