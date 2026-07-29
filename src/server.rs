use crate::config::Config;
use crate::session;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// Run the TCP server, accepting connections and spawning session tasks.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let addr = format!("{}:{}", config.server.bind, config.server.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("TCP server listening on {}", addr);

    let config = Arc::new(config);
    let banned: Arc<Mutex<HashMap<SocketAddr, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::error!("Accept error: {}", e);
                continue;
            }
        };

        // Check ban list
        {
            let ban_map = banned.lock().await;
            if let Some(until) = ban_map.get(&addr) {
                if Instant::now() < *until {
                    tracing::warn!("Rejected banned client: {}", addr);
                    continue;
                }
            }
        }

        tracing::info!("New connection from {}", addr);

        let config = config.clone();
        let banned = banned.clone();

        tokio::spawn(async move {
            if let Err(e) = session::handle(stream, addr, config, banned).await {
                tracing::error!("Session error [{}]: {}", addr, e);
            }
        });
    }
}
