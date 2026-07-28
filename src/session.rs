use crate::clipboard;
use crate::config::Config;
use crate::paste;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

const AUTH_TIMEOUT_SECS: u64 = 10;
const IDLE_TIMEOUT_SECS: u64 = 30;
const MAX_PAYLOAD_SIZE: usize = 10 * 1024; // 10 KB
const MAX_AUTH_ATTEMPTS: u32 = 3;
const BAN_DURATION_SECS: u64 = 30;

pub async fn handle(
    stream: TcpStream,
    addr: SocketAddr,
    config: Arc<Config>,
    banned: Arc<Mutex<HashMap<SocketAddr, Instant>>>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut auth_attempts: u32 = 0;

    // ── Auth phase ──────────────────────────────────────────────────────────
    tracing::info!("[{}] waiting for authentication", addr);

    loop {
        line.clear();
        match timeout(
            Duration::from_secs(AUTH_TIMEOUT_SECS),
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) => {
                tracing::info!("[{}] connection closed before auth", addr);
                return Ok(());
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::warn!("[{}] read error during auth: {}", addr, e);
                return Err(e.into());
            }
            Err(_) => {
                tracing::warn!("[{}] auth timeout", addr);
                return Ok(());
            }
        }

        let msg: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match msg.get("type").and_then(|v| v.as_str()) {
            Some("auth") => {
                let pin = msg
                    .get("pin")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if config.auth.pin.is_empty() || pin == config.auth.pin {
                    send_json(&mut writer, &serde_json::json!({"type": "auth_ok"})).await?;
                    tracing::info!("[{}] authenticated", addr);
                    break;
                }

                auth_attempts += 1;
                if auth_attempts >= MAX_AUTH_ATTEMPTS {
                    send_json(
                        &mut writer,
                        &serde_json::json!({
                            "type": "auth_fail",
                            "message": "认证失败次数过多，请30秒后重试"
                        }),
                    )
                    .await?;
                    tracing::warn!("[{}] banned after {} failed auth attempts", addr, auth_attempts);
                    banned
                        .lock()
                        .await
                        .insert(addr, Instant::now() + Duration::from_secs(BAN_DURATION_SECS));
                    return Ok(());
                }

                send_json(
                    &mut writer,
                    &serde_json::json!({
                        "type": "auth_fail",
                        "message": format!("PIN码错误 (剩余尝试次数: {})", MAX_AUTH_ATTEMPTS - auth_attempts)
                    }),
                )
                .await?;
            }
            _ => {
                // Not an auth message — remind client to authenticate
                send_json(
                    &mut writer,
                    &serde_json::json!({
                        "type": "auth_fail",
                        "message": "请先认证"
                    }),
                )
                .await?;
            }
        }
    }

    // ── Message loop (authenticated) ─────────────────────────────────────────
    loop {
        line.clear();
        match timeout(
            Duration::from_secs(IDLE_TIMEOUT_SECS),
            reader.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(0)) => {
                tracing::info!("[{}] connection closed", addr);
                return Ok(());
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::warn!("[{}] read error: {}", addr, e);
                return Err(e.into());
            }
            Err(_) => {
                tracing::info!("[{}] idle timeout", addr);
                return Ok(());
            }
        }

        let msg: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("[{}] invalid JSON: {}", addr, e);
                continue;
            }
        };

        let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match msg_type {
            "send" => {
                let payload = msg
                    .get("payload")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("");

                // Validate payload
                if payload.is_empty() {
                    send_json(
                        &mut writer,
                        &serde_json::json!({
                            "type": "nack",
                            "id": id,
                            "status": "empty",
                            "message": "不能发送空文本"
                        }),
                    )
                    .await?;
                    continue;
                }

                if payload.len() > MAX_PAYLOAD_SIZE {
                    send_json(
                        &mut writer,
                        &serde_json::json!({
                            "type": "nack",
                            "id": id,
                            "status": "too_large",
                            "message": format!("文本过长 (最大 {} 字节)", MAX_PAYLOAD_SIZE)
                        }),
                    )
                    .await?;
                    continue;
                }

                tracing::info!("[{}] received payload ({} bytes)", addr, payload.len());

                // Step 1: Write to clipboard
                if let Err(e) = clipboard::set_text(payload) {
                    tracing::error!("[{}] clipboard write failed: {}", addr, e);
                    send_json(
                        &mut writer,
                        &serde_json::json!({
                            "type": "nack",
                            "id": id,
                            "status": "clipboard_error",
                            "message": "写入剪贴板失败"
                        }),
                    )
                    .await?;
                    continue;
                }
                // Always paste — focus only affects status message
                // Write to clipboard and paste — always report success
                match paste::simulate_paste() {
                    Ok(()) => {
                        tracing::info!("[{}] sent", addr);
                        send_json(&mut writer, &serde_json::json!({"type":"ack","id":id,"status":"sent"})).await?;
                    }
                    Err(e) => {
                        tracing::warn!("[{}] paste simulation failed: {}", addr, e);
                        send_json(&mut writer, &serde_json::json!({
                            "type":"nack","id":id,"status":"error",
                            "message":format!("发送失败: {}", e)
                        })).await?;
                    }
                }
            }

            "ping" => {
                send_json(&mut writer, &serde_json::json!({"type": "pong"})).await?;
            }

            _ => {
                tracing::debug!("[{}] unknown message type: {}", addr, msg_type);
            }
        }
    }
}

/// Serialize a value to JSON, append a newline, and write to the stream.
async fn send_json(
    writer: &mut (impl AsyncWriteExt + Unpin),
    value: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut data = serde_json::to_vec(value)?;
    data.push(b'\n');
    writer.write_all(&data).await?;
    writer.flush().await?;
    Ok(())
}
