mod clipboard;
mod clipboard_read;
mod config;
mod daemon;
mod discovery;
mod paste;
mod server;
mod session;

use std::io::{self, Write};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str());

    match subcommand {
        Some("init") => return run_init(),
        Some("start") => return run_start(),
        Some("stop") => return run_stop(),
        Some("status") => return run_status(),
        Some("--daemon") => {
            daemon::daemonize()?;
        }
        _ => {}
    }

    // Run daemon in foreground (default)
    run_daemon().await
}

async fn run_daemon() -> anyhow::Result<()> {
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

    daemon::remove_pid_file();
    Ok(())
}
fn run_start() -> anyhow::Result<()> {
    if let Some(pid) = daemon::get_running_pid() {
        println!("ClipLink daemon is already running (PID: {})", pid);
        return Ok(());
    }

    let pid = daemon::spawn_daemon()?;
    // Wait briefly for the child to initialize and write its PID file
    std::thread::sleep(std::time::Duration::from_secs(1));

    if daemon::get_running_pid().is_some() {
        println!("ClipLink daemon started (PID: {})", pid);
    } else {
        daemon::remove_pid_file();
        anyhow::bail!(
            "Daemon failed to start. Check logs or run 'cliplinkd' in foreground to debug."
        );
    }
    Ok(())
}

fn run_stop() -> anyhow::Result<()> {
    match daemon::get_running_pid() {
        Some(pid) => {
            print!("Stopping ClipLink daemon (PID: {})... ", pid);
            io::stdout().flush()?;
            daemon::kill_process(pid)?;
            // Give it a moment to clean up
            std::thread::sleep(std::time::Duration::from_millis(500));
            daemon::remove_pid_file();
            println!("stopped.");
        }
        None => {
            println!("ClipLink daemon is not running.");
        }
    }
    Ok(())
}

fn run_status() -> anyhow::Result<()> {
    match daemon::get_running_pid() {
        Some(pid) => {
            println!("ClipLink daemon is running (PID: {})", pid);
        }
        None => {
            println!("ClipLink daemon is not running.");
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
    println!("  Run 'cliplinkd start' to start the daemon in background.");
    println!();

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
}
