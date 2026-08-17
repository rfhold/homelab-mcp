use std::{error::Error, sync::Arc, time::Duration};

use ::mcp::server::BoxFuture;
use homelab_mcp::{app, config, mcp, oauth, observability, profiling, services::Services};
use sqlx::postgres::PgPoolOptions;
use tokio::{
    net::TcpListener,
    sync::watch,
    time::{MissedTickBehavior, interval},
};

const LISTEN_ADDR: &str = "0.0.0.0:14333";
const CLEANUP_BATCH_SIZE: usize = 1000;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| std::io::Error::other("failed to install AWS-LC Rustls crypto provider"))?;
    let telemetry_config = config::TelemetryConfig::from_env().map_err(std::io::Error::other)?;
    let observability = observability::init(&telemetry_config)?;
    let profiling = profiling::init(&telemetry_config)?;
    tracing::info!(listen.address = LISTEN_ADDR, "service startup started");
    let config = config::Config::from_env().map_err(std::io::Error::other)?;
    let pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await
        .map_err(|_| std::io::Error::other("failed to connect to PostgreSQL"))?;
    homelab_mcp::database::migrate(&pool)
        .await
        .map_err(|_| std::io::Error::other("failed to run application database migrations"))?;
    let runtime = Arc::new(
        oauth::initialize(pool.clone(), &config.oidc, &config.oauth)
            .await
            .map_err(std::io::Error::other)?,
    );
    let services = Arc::new(Services::production(&config, pool).map_err(std::io::Error::other)?);
    let mcp =
        mcp::router(&config.oauth, services, &runtime.server).map_err(std::io::Error::other)?;
    let router = app::router(
        runtime.clone(),
        runtime.server.router(),
        runtime.oidc.router(),
        mcp,
    );
    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    tracing::info!(listen.address = LISTEN_ADDR, "service listening");
    let (shutdown, cleanup_shutdown) = watch::channel(false);
    let cleanup_runtime = runtime.clone();
    let cleanup_task = tokio::spawn(cleanup_loop(
        cleanup_shutdown,
        CLEANUP_INTERVAL,
        move || {
            let runtime = cleanup_runtime.clone();
            Box::pin(async move { runtime.cleanup(CLEANUP_BATCH_SIZE).await })
        },
    ));
    let signal_shutdown = shutdown.clone();

    let server_result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            tracing::info!("shutdown signal received");
            signal_shutdown.send_replace(true);
        })
        .await;
    shutdown.send_replace(true);
    let _ = cleanup_task.await;
    tracing::info!("service shutdown started");
    profiling.shutdown();
    tracing::info!("service shutdown complete");
    observability.shutdown();
    server_result?;
    Ok(())
}

async fn cleanup_loop<F>(
    mut shutdown: watch::Receiver<bool>,
    cleanup_interval: Duration,
    mut cleanup: F,
) where
    F: FnMut() -> BoxFuture<()> + Send + 'static,
{
    let mut ticker = interval(cleanup_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        if *shutdown.borrow() {
            return;
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
            _ = ticker.tick() => {
                tokio::select! {
                    biased;
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            return;
                        }
                    }
                    () = cleanup() => {}
                }
            }
        }
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn cleanup_loop_stops_before_work_when_already_cancelled() {
        let (shutdown, receiver) = watch::channel(true);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_cleanup = calls.clone();

        cleanup_loop(receiver, CLEANUP_INTERVAL, move || {
            calls_for_cleanup.fetch_add(1, Ordering::Relaxed);
            Box::pin(async {})
        })
        .await;

        assert!(shutdown.is_closed());
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn cleanup_loop_cancels_an_inflight_cleanup() {
        let (shutdown, receiver) = watch::channel(false);
        let (started, started_receiver) = oneshot::channel();
        let mut started = Some(started);
        let task = tokio::spawn(cleanup_loop(receiver, CLEANUP_INTERVAL, move || {
            let started = started
                .take()
                .expect("cleanup runs once before cancellation");
            Box::pin(async move {
                let _ = started.send(());
                std::future::pending::<()>().await;
            })
        }));

        started_receiver.await.unwrap();
        shutdown.send_replace(true);
        task.await.unwrap();
    }
}
