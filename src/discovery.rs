use crate::config::Config;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;

/// Multicast group for device discovery.
/// 224.0.0.167 is in the 224.0.0.0/24 Local Network Control Block —
/// compatible with Android's multicast whitelist and unlikely to be
/// filtered by consumer WiFi equipment.
const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
const MULTICAST_TTL: u32 = 1; // link-local only, don't cross routers

/// UDP multicast-based device discovery.
///
/// **Bidirectional**: daemon actively multicasts announcements AND passively
/// responds to phone discovery packets.
///
/// **Burst**: on startup, sends 3 rapid announcements (t=0, 100ms, 600ms)
/// before settling into a 5-second interval.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let discovery_port = config.server.port + 1;
    let multicast_target = format!("{}:{}", MULTICAST_ADDR, discovery_port);

    // ── Passive receiver socket: joins multicast group, responds unicast ─
    let recv_socket = {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.bind(&socket2::SockAddr::from(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            discovery_port,
        )))?;
        socket.join_multicast_v4(&MULTICAST_ADDR, &Ipv4Addr::UNSPECIFIED)?;
        socket.set_multicast_ttl_v4(MULTICAST_TTL)?;
        socket.set_multicast_loop_v4(true)?;

        let std_sock: std::net::UdpSocket = socket.into();
        std_sock.set_nonblocking(true)?;
        UdpSocket::from_std(std_sock)?
    };

    tracing::info!(
        "Discovery service listening on UDP {} (multicast {})",
        discovery_port,
        MULTICAST_ADDR
    );

    // ── Active announcer socket: sends to multicast group ───────────────
    let announce_socket = {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.bind(&socket2::SockAddr::from(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            0, // ephemeral
        )))?;
        socket.set_multicast_ttl_v4(MULTICAST_TTL)?;
        socket.set_multicast_loop_v4(true)?;

        let std_sock: std::net::UdpSocket = socket.into();
        std_sock.set_nonblocking(true)?;
        UdpSocket::from_std(std_sock)?
    };

    let announce = serde_json::json!({
        "type": "announce",
        "name": config.service.name,
        "tcp_port": config.server.port,
    });
    let announce_bytes = Arc::new(serde_json::to_vec(&announce)?);

    // ── Active broadcaster task (burst + periodic) ──────────────────────
    let broadcast_bytes = announce_bytes.clone();
    let broadcast_target = multicast_target.clone();
    tokio::spawn(async move {
        // Burst: rapid announcements for fast discovery
        send_burst(&announce_socket, &broadcast_bytes, &broadcast_target).await;

        // Settle into periodic announcements
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if let Err(e) = announce_socket
                .send_to(&broadcast_bytes, &broadcast_target)
                .await
            {
                tracing::warn!("Failed to multicast announcement: {}", e);
            } else {
                tracing::debug!("Sent announcement to {}", broadcast_target);
            }
        }
    });

    // ── Passive receive loop ────────────────────────────────────────────
    let mut buf = [0u8; 512];
    loop {
        match recv_socket.recv_from(&mut buf).await {
            Ok((_len, src)) => {
                tracing::debug!("Discovery request from {}", src);
                // Respond via unicast directly to the sender
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

/// Send announcement burst: 3 packets with backoff for fast discovery.
async fn send_burst(socket: &UdpSocket, data: &[u8], target: &str) {
    let delays_ms = [0u64, 100, 600]; // t=0, +100ms, +500ms = total 600ms
    for &delay_ms in &delays_ms {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        if let Err(e) = socket.send_to(data, target).await {
            tracing::warn!("Burst announcement failed: {}", e);
            return; // If one fails, likely network is down — stop bursting
        }
    }
    tracing::debug!("Announcement burst complete");
}
