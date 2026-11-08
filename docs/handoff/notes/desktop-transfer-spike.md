# Spike 记录 · 桌面端局域网互传（Phase 0）

> 日期：2026-11-08
> 环境：Windows / Rust 1.99.1 / cargo 1.99.1 / Node 26.12.1 / pnpm 10.33.0
> 结论：**可以复用 LocalSend core**，但不能用 git 依赖，改为 vendor 源码。

## 1. 上游版本

| 项 | 值 |
|---|---|
| 仓库 | `https://github.com/localsend/localsend` |
| 路径 | `packages/core`（crate `localsend`） |
| Rev | `58955fd54ef1e18e89ec30e60f46754084a80942`（master，2026-11-08） |
| 协议 | v2.2（`PROTOCOL_VERSION_V2`），同时存在 v3 客户端实现 |
| feature | `discovery` = `http` + `crypto` + `multicast` |

## 2. 关键坑：git 依赖不可用

尝试 1：`localsend = { git = "https://github.com/localsend/localsend", rev = "...", features = ["discovery"] }`

```
failed to update submodule `support/submodules/flutter`
  caused by: revision 00b0c91... not found / network failure
```

上游仓库带 `support/submodules/flutter` 子模块，cargo 解析 git 依赖时会递归 fetch
子模块（Flutter 仓库，GB 级），本机网络必失败。

尝试 2：`CARGO_NET_GIT_FETCH_WITH_CLI=false`（built-in gitoxide）— 同上。

尝试 3：保留 git 依赖 + `[patch."https://github.com/localsend/localsend"] localsend = { path = ... }`
— **cargo 仍会先 fetch 原仓库**，patch 不能绕过。

尝试 4（采用）：源码 vendor 到 `crates/vendor/localsend-core`，path 依赖。
未改一行代码，保留 `LICENSE`，来源与 rev 写在 `VENDORED.md`。

## 3. `pub mod webrtc` 是否会导致编译失败

不会。`src/lib.rs` 里 `pub mod webrtc;` 无门控，但 `src/webrtc/mod.rs` 内部是：

```rust
pub mod signaling;
#[cfg(feature = "webrtc")]
pub mod webrtc;
```

`signaling` 依赖的 `tungstenite` / `futures-util` 在 Cargo.toml 里是非 optional 依赖，
因此 `features = ["discovery"]` 可正常编译（有 8 条 dead_code/unused 警告，来自未启用的
WebRTC 路径，无害）。**不需要 patch / vendor 改动。**

## 4. 依赖版本要求（会影响现有桌面端锁文件）

`packages/core` 要求：`anyhow 1.0.104`、`hyper 1.11`、`rustls 0.23.43`、`reqwest 0.13.4`、
`tokio 1.53.1`、`tokio-util 0.7.19`、`tokio-stream 0.1.19`、`time 0.3.54`、`sha2 0.11`、
`rcgen 0.14.8`、`x509-parser 0.18`、`lru 0.18`、`if-addrs 0.15`、`base64 0.23`。

桌面端原有 `apps/desktop/src-tauri/vendor/`（离线源，659 个包）版本落后且缺 `rcgen` /
`x509-parser` / `lru` / `if-addrs` / `tungstenite` / `tokio-tungstenite` / `tracing-subscriber`。
已用 `cargo vendor --versioned-dirs` 重新生成（现 699 个包），离线构建恢复。
`vendor/` 是 gitignore 的本地产物，不入库。

## 5. 实际 API（用到的部分）

```rust
// 身份
localsend::crypto::cert::generate_self_signed() -> SelfSignedCert { private_key_pem, certificate_pem, fingerprint, .. }

// 发现
localsend::discovery::start(DiscoveryConfig { group, group_v6, port, interface_filter, device, identity, timeout, event_tx }, stop_rx) -> DiscoveryHandle
handle.announce() / discover_staged(channels, interface_ips, port, protocol, grace)
handle.discover(host, port, protocol) -> Option<StatefulDevice>
handle.add_device(DiscoveredDevice) / devices() / wait_stopped()

// 服务端
localsend::http::server::start_with_port(port, Some(TlsConfig{cert, private_key}), ClientInfo{alias, version, device_model, device_type, token},
    None, Some(ServerConfigV2 { pin, verify_checksums, event_tx }),
    WebConfig { mode: WebMode::Disabled, i18n: WebI18n::default(), pages: WebPages::default() }, stop_rx)
  -> ServerHandle { port(), wait_stopped(), cancel_v2_session(session_id) }

// ServerEventV2: Register / PrepareUpload{session_id, ip, info, cert_fingerprint, files, decision_tx}
//                 FileUpload{session_id, file_id, file, target_tx} / SessionEnd{session_id, reason}
//                 PrepareUploadAborted / CancelReceived / ListenerFailed
// 目标：FileUploadTarget::Path { path, result_tx, progress_tx }

// 客户端
LsHttpClient::new(key_pem, cert_pem, LsHttpClientVersion::V2, expected_fingerprint, timeout)
register(protocol, ip, port, RegisterDto) -> ResultWithPublicKey { public_key, cert_fingerprint, body }
prepare_upload(protocol, ip, port, public_key, PrepareUploadRequestDto, pin, cancel) -> PrepareUploadResult { status_code, response }
upload(protocol, ip, port, public_key, session_id, file_id, token, FileContent::Path(p), progress_fn, cancel)
cancel(protocol, ip, port, session_id)
```

注意点：

- `upload` 的 progress 回调参数是**累计字节**。
- `prepare_upload` 401 = PIN 相关（无 PIN → `PIN_REQUIRED`，有 PIN → `PIN_INVALID`）、403 = 拒绝、409 = 对端忙、422 = 校验失败。
- v2 服务端**同时只允许一个上传会话**（单槽），UI 需要处理“对端忙”。
- `SessionEnd` 可能早于我们自己的 rename/Completed 事件；core 里用 `writing` 计数延后 `SessionEnded`。

## 6. 回环验证

`crates/transfer-core/tests/loopback.rs`：两个实例分别 `port = 0`（系统分配），用
`add_manual_ip("127.0.0.1", port)` 发现，HTTPS + mTLS + prepare-upload + upload，
断言收到的文件内容、落盘文件名、发送/接收两侧的 `Completed` 与 `SessionEnded(Completed)`。
`cargo test` 通过。

尚未验证（需要真机）：官方 LocalSend 双向互通、真实组播、Windows 防火墙弹窗、1GB+、
睡眠唤醒后 socket 失效。

## 7. 追加发现：Windows 端口排除段会“假装”组播不可用

用 53999 端口跑真实 app 时，互传窗口报
`No network interface available for multicast on port 53999`，但换成 53318 立刻正常。

原因：Windows（Hyper-V / WSL / WinNAT）会保留一段 UDP/TCP 端口，绑定这些端口直接被拒，
`multicast::start` 里每个网卡都绑定失败 → `sockets.is_empty()` → 报“没有可用网卡”。

```powershell
netsh int ipv4 show excludedportrange protocol=udp
# 本机：53532-53631、53976-54075、57566-57665 …（53999 正好在排除段里）
```

结论：**不要把“组播失败”简单当成网卡问题**。用户机器上 53317 也可能被排除，
所以 UI 必须给“端口被系统保留 → 换端口 / 用手动 IP”的提示（已实现）。
排查工具：`cargo run --example multicast_probe -- <port>`。

## 8. 自动化验收手段（无需人工点）

- 用 `TAURI_CONFIG` 覆盖 identifier（独立配置目录，不碰正式环境）+ 给主窗口
  `additionalBrowserArgs: "--remote-debugging-port=9222 --remote-allow-origins=*"`，
  debug 构建即可用 CDP（`http://127.0.0.1:9222/json/list`）读 DOM / 截图 / 调 IPC。
- 注意：`additional_browser_args` 必须给**主窗口**（浏览器进程参数在第一个 webview 创建时定死），
  只给后续窗口无效。
- 锁屏状态下 Tao 无法真正创建新窗口（`open_month_window` 同样复现），
  所以“开窗类”验收要等解锁后进行。
