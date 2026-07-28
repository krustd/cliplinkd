use crate::config::Config;
use tokio::net::UdpSocket;

/// UDP broadcast responder for device discovery.
/// Listens on `config.port + 1` for "discover" broadcasts and responds
/// with the daemon's name and TCP port.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let port = config.server.port + 1;
    let addr = format!("0.0.0.0:{}", port);
    let socket = UdpSocket::bind(&addr).await?;
    socket.set_broadcast(true)?;

    tracing::info!("Discovery service listening on UDP {}", addr);

    let announce = serde_json::json!({
        "type": "announce",
        "name": config.service.name,
        "tcp_port": config.server.port,
    });
    let announce_bytes = serde_json::to_vec(&announce)?;

    let mut buf = [0u8; 512];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((_len, src)) => {
                tracing::debug!("Discovery request from {}", src);
                if let Err(e) = socket.send_to(&announce_bytes, src).await {
                    tracing::warn!("Failed to send discovery response to {}: {}", src, e);
                }
            }
            Err(e) => {
                tracing::error!("Discovery socket error: {}", e);
            }
        }
    }
}
