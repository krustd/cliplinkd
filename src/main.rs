mod clipboard;
mod config;
mod discovery;
mod focus;
mod paste;
mod server;
mod session;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // Load configuration
    let config = config::Config::load()?;

    tracing::info!(
        "ClipLink daemon v{} starting",
        env!("CARGO_PKG_VERSION")
    );
    tracing::info!(
        "Service name: \"{}\", listening on {}:{}",
        config.service.name,
        config.server.bind,
        config.server.port
    );
    if config.auth.pin.is_empty() {
        tracing::warn!("No PIN configured — any device can connect without authentication");
    }

    // Spawn UDP discovery service
    let disc_config = config.clone();
    tokio::spawn(async move {
        if let Err(e) = discovery::run(disc_config).await {
            tracing::error!("Discovery service stopped: {}", e);
        }
    });

    // Run TCP server (blocks until SIGINT/SIGTERM)
    tokio::select! {
        result = server::run(config) => {
            if let Err(e) = result {
                tracing::error!("Server stopped: {}", e);
            }
        }
        _ = shutdown_signal() => {
            tracing::info!("Shutting down...");
        }
    }

    Ok(())
}

/// Wait for a SIGINT (Ctrl+C) or SIGTERM signal.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
}
