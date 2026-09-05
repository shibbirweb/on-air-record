//! On Air Record: audio broadcast and DVR service.
//!
//! `main` does four things and nothing else: set up logging, resolve the configuration, build the
//! application, and serve it until asked to stop. Everything interesting happens behind [`AppState`].

use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use on_air_record::app::AppState;
use on_air_record::config::{AppConfig, CliArgs};
use on_air_record::routes;

#[tokio::main]
async fn main() {
    let config = AppConfig::resolve(CliArgs::parse());
    init_tracing(&config.log_level);

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %config.data_dir.display(),
        static_dir = %config.static_dir.display(),
        "starting on air record"
    );

    let state = match AppState::bootstrap(config.clone()) {
        Ok(state) => state,
        Err(error) => {
            tracing::error!(%error, "could not start the service");
            std::process::exit(1);
        }
    };

    // One watch channel tells every background task to wind down, so shutdown stays a single signal
    // rather than a handle per task.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    tokio::spawn(state.retention.clone().run(shutdown_rx));

    if let Err(error) = state.capture.start_if_configured().await {
        tracing::error!(%error, "auto start failed");
    }

    let address = config.socket_addr();
    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, %address, "could not bind the http listener");
            std::process::exit(1);
        }
    };

    tracing::info!(%address, "control room ready on http://{address}");

    let router = routes::build(state.clone());
    let server = axum::serve(listener, router).with_graceful_shutdown(wait_for_shutdown());

    if let Err(error) = server.await {
        tracing::error!(%error, "the http server stopped unexpectedly");
    }

    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
    // Stopping capture last flushes the segment that is still open, so the final seconds are indexed and
    // playable rather than left as an orphan file.
    state.capture.shutdown().await;
    tracing::info!("stopped");
}

fn init_tracing(log_level: &str) {
    // `RUST_LOG` wins when it is set, so an operator can turn on a noisy target without editing the
    // service configuration.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "on_air_record={log_level},tower_http=warn,axum=warn"
        ))
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();
}

/// Resolve on Ctrl+C, and on SIGTERM where the platform has one, so a service manager can stop the
/// process cleanly instead of killing it and losing the open segment.
async fn wait_for_shutdown() {
    let interrupt = async {
        if tokio::signal::ctrl_c().await.is_err() {
            tracing::error!("could not listen for ctrl+c");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(error) => tracing::error!(%error, "could not listen for sigterm"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => {}
        _ = terminate => {}
    }
}
