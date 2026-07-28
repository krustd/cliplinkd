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
const MAX_PAYLOAD_SIZE: usize = 10 * 1024;
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
    let svc_name = &config.service.name;

    tracing::info!("[{}] waiting for authentication", addr);

    loop {
        line.clear();
        match timeout(Duration::from_secs(AUTH_TIMEOUT_SECS), reader.read_line(&mut line)).await {
            Ok(Ok(0)) => { tracing::info!("[{}] closed before auth", addr); return Ok(()); }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => { tracing::warn!("[{}] read error during auth: {}", addr, e); return Err(e.into()); }
            Err(_) => { tracing::warn!("[{}] auth timeout", addr); return Ok(()); }
        }

        let msg: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if msg.get("type").and_then(|v| v.as_str()) != Some("auth") {
            send_json(&mut writer, &serde_json::json!({
                "type":"auth_fail","name":svc_name,"message":"请先认证"
            })).await?;
            continue;
        }

        let pin = msg.get("pin").and_then(|v| v.as_str()).unwrap_or("");

        if config.auth.pin.is_empty() || pin == config.auth.pin {
            send_json(&mut writer, &serde_json::json!({
                "type":"auth_ok","name":svc_name
            })).await?;
            tracing::info!("[{}] authenticated", addr);
            break;
        }

        auth_attempts += 1;
        if auth_attempts >= MAX_AUTH_ATTEMPTS {
            send_json(&mut writer, &serde_json::json!({
                "type":"auth_fail","name":svc_name,"message":"认证失败次数过多，请30秒后重试"
            })).await?;
            banned.lock().await.insert(addr, Instant::now() + Duration::from_secs(BAN_DURATION_SECS));
            return Ok(());
        }

        send_json(&mut writer, &serde_json::json!({
            "type":"auth_fail","name":svc_name,
            "message":format!("PIN码错误 (剩余尝试次数: {})", MAX_AUTH_ATTEMPTS - auth_attempts)
        })).await?;
    }

    loop {
        line.clear();
        match timeout(Duration::from_secs(IDLE_TIMEOUT_SECS), reader.read_line(&mut line)).await {
            Ok(Ok(0)) => { tracing::info!("[{}] closed", addr); return Ok(()); }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => { tracing::warn!("[{}] read error: {}", addr, e); return Err(e.into()); }
            Err(_) => { tracing::info!("[{}] idle timeout", addr); return Ok(()); }
        }

        let msg: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match msg.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "send" => {
                let payload = msg.get("payload").and_then(|v| v.as_str()).unwrap_or("");
                let id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if payload.is_empty() {
                    send_json(&mut writer, &serde_json::json!({
                        "type":"nack","id":id,"status":"empty","message":"不能发送空文本"
                    })).await?;
                    continue;
                }
                if payload.len() > MAX_PAYLOAD_SIZE {
                    send_json(&mut writer, &serde_json::json!({
                        "type":"nack","id":id,"status":"too_large",
                        "message":format!("文本过长 (最大 {} 字节)", MAX_PAYLOAD_SIZE)
                    })).await?;
                    continue;
                }
                tracing::info!("[{}] received payload ({} bytes)", addr, payload.len());
                if let Err(_) = clipboard::set_text(payload) {
                    send_json(&mut writer, &serde_json::json!({
                        "type":"nack","id":id,"status":"clipboard_error","message":"写入剪贴板失败"
                    })).await?;
                    continue;
                }
                match paste::simulate_paste() {
                    Ok(()) => {
                        tracing::info!("[{}] sent", addr);
                        send_json(&mut writer, &serde_json::json!({
                            "type":"ack","id":id,"status":"sent"
                        })).await?;
                    }
                    Err(e) => {
                        send_json(&mut writer, &serde_json::json!({
                            "type":"nack","id":id,"status":"error",
                            "message":format!("发送失败: {}", e)
                        })).await?;
                    }
                }
            }
            "ping" => {
                send_json(&mut writer, &serde_json::json!({"type":"pong"})).await?;
            }
            _ => {}
        }
    }
}

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
