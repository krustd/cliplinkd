# ClipLink Daemon

**`cliplinkd`** — 桌面端守护进程，接收来自手机 ClipLink APP 的文本，自动写入剪贴板并粘贴。

## 特性

- **剪贴板拉取**：手机一键获取电脑剪贴板内容（文本/图片/文件），支持大文件确认和进度条
- **多播发现**：224.0.0.167 多播 + Burst 首发，手机自动发现，无需手动输入 IP
- **PIN 认证**：可选的 PIN 码保护，3 次失败后自动封禁 IP 30 秒
- **后台运行**：`cliplinkd start` / `stop` 一键启停，不占用终端窗口

## 安装

### crates.io

```bash
cargo install krust-cliplinkd
```

安装后二进制文件名为 `cliplinkd`。

### GitHub Releases

从 [Releases](https://github.com/krustd/cliplinkd/releases) 下载预编译二进制：

- `cliplinkd-aarch64-apple-darwin` — macOS Apple Silicon
- `cliplinkd-x86_64-apple-darwin` — macOS Intel
- `cliplinkd-x86_64-unknown-linux-gnu` — Linux
- `cliplinkd-x86_64-pc-windows-msvc.exe` — Windows

### 从源码编译

```bash
git clone https://github.com/krustd/cliplinkd.git
cd cliplinkd
cargo build --release
# 二进制位于 ./target/release/cliplinkd
```

## 配置

### 快速设置（推荐）

```bash
cliplinkd init
```

交互式向导，依次设置：
- **Service name** — 手机端显示的设备名（默认取 hostname）
- **PIN code** — 认证密码，留空则不验证
- **TCP port** — 监听端口（默认 9527，UDP 发现端口自动为 port + 1）

配置保存在 `~/.config/cliplinkd/cliplinkd.toml`。

### 手动编辑

配置文件加载顺序（优先级从高到低）：

1. `~/.config/cliplinkd/cliplinkd.toml`
2. `./cliplinkd.toml`（本地覆盖）

```toml
[server]
bind = "0.0.0.0"    # 监听地址
port = 9527          # TCP 端口（UDP 发现端口 = port + 1）

[auth]
pin = "123456"       # PIN 码；留空则跳过认证

[service]
name = "My Computer" # 手机端显示的设备名，默认取 hostname

[clipboard]
max_fetch_size = 524288  # 文本自动获取阈值（字节），超过此大小需确认；默认 512KB
```
## 运行

```bash
# 后台运行（推荐）
cliplinkd start       # 启动后台守护进程
cliplinkd stop        # 停止后台守护进程
cliplinkd status      # 查看运行状态

# 前台运行（调试用）
cliplinkd

# 设置日志级别
RUST_LOG=debug cliplinkd
```
### 开机自启

**macOS** — LaunchAgent：

```xml
<!-- ~/Library/LaunchAgents/com.cliplink.daemon.plist -->
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.cliplink.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/cliplinkd</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

**Linux** — systemd user service：

```ini
# ~/.config/systemd/user/cliplinkd.service
[Unit]
Description=ClipLink Daemon

[Service]
ExecStart=%h/.local/bin/cliplinkd
Restart=on-failure

[Install]
WantedBy=default.target
```

```bash
systemctl --user enable --now cliplinkd.service
```

**Windows** — 将 `cliplinkd.exe` 放入 `shell:startup` 目录，或注册为 Windows Service。

## 协议

通信格式为 JSON over TCP，`\n` 分隔。

| 方向 | 消息 | 说明 |
|------|------|------|
| 手机→电脑 | `{"type":"auth","pin":"...","device_name":"..."}` | 认证 |
| 电脑→手机 | `{"type":"auth_ok","name":"..."}` | 认证成功（含设备名） |
| 电脑→手机 | `{"type":"auth_fail","name":"...","message":"..."}` | 认证失败（含设备名） |
| 手机→电脑 | `{"type":"send","payload":"...","id":"uuid"}` | 发送文本 |
| 电脑→手机 | `{"type":"ack","id":"uuid","status":"sent"}` | 已发送 |
| 电脑→手机 | `{"type":"nack","id":"uuid","status":"error","message":"..."}` | 发送失败 |
| 手机→电脑 | `{"type":"key","key":"enter","id":"uuid"}` | 发送按键（目前仅支持 enter） |
| 电脑→手机 | `{"type":"ack","id":"uuid","status":"sent"}` | 按键已执行 |
| 电脑→手机 | `{"type":"nack","id":"uuid","status":"error","message":"..."}` | 按键执行失败 |
| 手机→电脑 | `{"type":"ping"}` | 心跳 |
| 电脑→手机 | `{"type":"pong"}` | 心跳响应 |
| 手机→电脑 | `{"type":"clipboard_query"}` | 查询电脑剪贴板 |
| 电脑→手机 | `{"type":"clipboard_info","content_type":"text\|image\|file\|none",...}` | 剪贴板内容信息 |
| 手机→电脑 | `{"type":"clipboard_fetch","content_type":"text\|image\|file",...}` | 请求获取内容 |
| 电脑→手机 | `{"type":"clipboard_data","content_type":"text\|image\|file","payload_base64":"...",...}` | 返回内容数据 |

## 许可

MIT License
