mod focus;
mod clipboard;
mod config;
mod discovery;
mod paste;
mod server;
mod session;

use std::io::{self, Write};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Check for subcommands before starting the daemon
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "init" {
        return run_init();
    }

    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

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
        tracing::warn!("No PIN configured — run 'cliplinkd init' to set one, or any device can connect");
    }

    let disc_config = config.clone();
    tokio::spawn(async move {
        if let Err(e) = discovery::run(disc_config).await {
            tracing::error!("Discovery service stopped: {}", e);
        }
    });

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

/// Interactive configuration wizard.
fn run_init() -> anyhow::Result<()> {
    println!();
    println!("  ╔══════════════════════════════════╗");
    println!("  ║   ClipLink Daemon — Setup       ║");
    println!("  ╚══════════════════════════════════╝");
    println!();

    let mut config = config::Config::default();

    // Service name
    print!("  Service name [{}]: ", config.service.name);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let name = input.trim().to_string();
    if !name.is_empty() {
        config.service.name = name;
    }

    // PIN
    print!("  PIN code [{}]: ", config.auth.pin);
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let pin = input.trim().to_string();
    if !pin.is_empty() {
        config.auth.pin = pin;
    }

    // Port
    print!("  TCP port [{}]: ", config.server.port);
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    if let Ok(port) = input.trim().parse::<u16>() {
        config.server.port = port;
    }

    // Summary
    println!();
    println!("  ──────────────────────────────────");
    println!("  Service name : {}", config.service.name);
    println!("  PIN          : {}", if config.auth.pin.is_empty() { "(none — insecure!)" } else { &config.auth.pin });
    println!("  TCP port     : {} (UDP discovery: {})", config.server.port, config.server.port + 1);
    println!("  ──────────────────────────────────");
    println!();

    print!("  Save? [Y/n]: ");
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    if input.trim().to_lowercase() == "n" {
        println!("  Aborted.");
        return Ok(());
    }

    let path = config.save()?;
    println!("  ✓ Configuration saved to {}", path.display());
    println!();
    println!("  Run 'cliplinkd' to start the daemon.");
    println!();

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
}
