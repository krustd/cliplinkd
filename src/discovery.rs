use crate::config::Config;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;

/// UDP broadcast-based device discovery.
///
/// **Bidirectional**: the daemon both passively responds to phone discovery
/// requests AND actively broadcasts announcements every 5 seconds. This dual
/// approach works even when WiFi AP isolation or firewalls block one direction.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let discovery_port = config.server.port + 1;
    let broadcast_addr = format!("255.255.255.255:{}", discovery_port);
    let listen_addr = format!("0.0.0.0:{}", discovery_port);

    // ── Passive socket: receives discovery requests, responds to sender ─
    let recv_socket = UdpSocket::bind(&listen_addr).await?;
    recv_socket.set_broadcast(true)?;
    tracing::info!("Discovery service listening on UDP {}", listen_addr);

    // ── Active broadcast socket: sends periodic announcements to LAN ───
    let announce_socket = UdpSocket::bind("0.0.0.0:0").await?;
    announce_socket.set_broadcast(true)?;

    let announce = serde_json::json!({
        "type": "announce",
        "name": config.service.name,
        "tcp_port": config.server.port,
    });
    let announce_bytes = Arc::new(serde_json::to_vec(&announce)?);

    // Spawn periodic active broadcaster
    let broadcast_bytes = announce_bytes.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = announce_socket
                .send_to(&broadcast_bytes, &broadcast_addr)
                .await
            {
                tracing::warn!("Failed to send announcement broadcast: {}", e);
            } else {
                tracing::debug!("Sent announcement broadcast to {}", broadcast_addr);
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    // Passive receive loop: respond to phone discovery packets
    let mut buf = [0u8; 512];
    loop {
        match recv_socket.recv_from(&mut buf).await {
            Ok((_len, src)) => {
                tracing::debug!("Discovery request from {}", src);
                if let Err(e) = recv_socket.send_to(&announce_bytes, src).await {
                    tracing::warn!("Failed to send discovery response to {}: {}", src, e);
                }
            }
            Err(e) => {
                tracing::error!("Discovery socket error: {}", e);
            }
        }
    }
}
