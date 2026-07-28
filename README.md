# ClipLink Daemon

**`cliplinkd`** — 桌面端守护进程，接收来自手机 ClipLink APP 的文本，自动写入剪贴板并在聚焦输入框时粘贴。

## 特性

- **零配置发现**：UDP 广播自动被手机端发现，无需手动输入 IP
- **PIN 认证**：可选的 PIN 码保护，3 次失败后自动封禁 IP 30 秒
- **焦点感知粘贴**：检测当前焦点是否为输入框，是则自动 `Ctrl/Cmd+V`，否则仅写剪贴板并回传状态
- **跨平台焦点检测**：

| 平台 | 检测方式 |
|------|---------|
| macOS | AXUIElement Accessibility API |
| Linux X11 | XGetInputFocus + XGetClassHint |
| Linux Wayland | AT-SPI2 via D-Bus |
| Windows | GetGUIThreadInfo + UIAutomation |

- **极低资源占用**：Rust 编写，release 二进制约 1.2 MB，空闲 CPU < 0.1%，内存 < 15 MB

## 安装

### crates.io

```bash
cargo install krust-cliplinkd
```

安装后二进制文件名为 `cliplinkd`。

### GitHub Releases

从 [Releases](https://github.com/krustd/cliplinkd/releases) 下载预编译二进制：

- `cliplinkd-x86_64-apple-darwin` — macOS Intel
- `cliplinkd-aarch64-apple-darwin` — macOS Apple Silicon
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

首次运行时自动生成默认配置。配置文件搜索路径（优先级从高到低）：

1. `./cliplinkd.toml`
2. `~/.config/cliplinkd/cliplinkd.toml`

```toml
[server]
bind = "0.0.0.0"    # 监听地址
port = 9527          # TCP 端口（UDP 发现端口 = port + 1）

[auth]
pin = "123456"       # PIN 码；留空则跳过认证

[service]
name = "My Computer" # mDNS 广播名称，默认取 hostname
```

## 运行

```bash
# 前台运行
cliplinkd

# 使用自定义配置
cliplinkd --config /path/to/config.toml

# 设置日志级别
RUST_LOG=debug cliplinkd
```

### 开机自启

**macOS** — 创建 LaunchAgent：

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

```bash
launchctl load ~/Library/LaunchAgents/com.cliplink.daemon.plist
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

详见 `cliplinkd.toml` 和源码 `src/session.rs`。通信格式为 JSON over TCP，`\n` 分隔。

| 方向 | 消息 | 说明 |
|------|------|------|
| 手机→电脑 | `{"type":"auth","pin":"...","device_name":"..."}` | 认证 |
| 电脑→手机 | `{"type":"auth_ok"}` | 认证成功 |
| 电脑→手机 | `{"type":"auth_fail","message":"..."}` | 认证失败 |
| 手机→电脑 | `{"type":"send","payload":"...","id":"uuid"}` | 发送文本 |
| 电脑→手机 | `{"type":"ack","id":"uuid","status":"pasted"}` | 已粘贴 |
| 电脑→手机 | `{"type":"ack","id":"uuid","status":"clipboard_only"}` | 仅写剪贴板 |
| 电脑→手机 | `{"type":"nack","id":"uuid","status":"no_focus","message":"..."}` | 无聚焦输入框 |
| 手机→电脑 | `{"type":"ping"}` | 心跳 |
| 电脑→手机 | `{"type":"pong"}` | 心跳响应 |

UDP 发现：手机广播 `{"type":"discover"}` 到 `port+1`，电脑响应 `{"type":"announce","name":"...","tcp_port":9527}`。

## 权限要求

| 平台 | 权限 | 说明 |
|------|------|------|
| macOS | 辅助功能权限 | 系统设置 → 隐私与安全性 → 辅助功能，添加终端或 cliplinkd |
| Linux X11 | 无 | — |
| Linux Wayland | AT-SPI2 总线 | 大多数桌面环境默认启用 |
| Windows | 无 | — |

若未授权辅助功能权限（macOS），焦点检测退化为 `None`，仅写入剪贴板不自动粘贴。

## 许可

MIT License
