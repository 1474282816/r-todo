# rtodo-transfer-core

r-todo 局域网互传的共享 Rust 核心（LocalSend 协议兼容）。

## 当前状态

**已实现 MVP**：身份、发现、发送、接收、进度、取消/拒绝、错误码映射。

- 协议：LocalSend **HTTP(S) v2**（`2.2`），端口默认 `53317`，HTTPS 自签证书 + fingerprint 固定
- 发现：UDP 组播（IPv4 `224.0.0.167` + IPv6 组播）+ 手动 IP + 已知网卡 `/24` 兜底扫描
- 发送：`register` → `prepare-upload` → 逐文件 `upload`（累计字节进度，100ms 节流）
- 接收：`PrepareUpload` → 人工确认 → 每个文件由核心生成安全落盘路径（先 `.part` 再 rename）
- 协议实现来自 LocalSend 官方 core：`crates/vendor/localsend-core`（Apache-2.0，pin rev，见该目录 `VENDORED.md`）

## 契约

- `SCHEMA_VERSION = 1`
- 事件：`devices` / `incoming` / `progress` / `completed` / `error` / `session_ended`
- 错误码：`PORT_IN_USE` / `FIREWALL` / `MULTICAST_UNAVAILABLE` / `TLS` / `PIN_REQUIRED` / `PIN_INVALID` / `REJECTED` / `CHECKSUM_MISMATCH` / `CANCELLED` / `IO` / `UNKNOWN`
- 字段/错误码改动必须同步 `docs/handoff/2026-11-08-desktop-transfer.md`、`docs/handoff/2026-11-08-mobile-transfer.md` 和 `tests/contract.rs`

## API

```rust
let (tx, mut rx) = tokio::sync::mpsc::channel(64);
let service = TransferService::start(TransferConfig { /* alias, port, receive_dir, identity_dir, pin, .. */ }, tx).await?;

service.refresh().await?;                       // 主动发现（结果走 Devices 事件）
service.add_manual_ip("192.168.1.9", 53317).await?;
let session = service.send(files, fingerprint, pin).await?;
service.pending_incoming().await?;                              // 等待确认的请求（窗口重开后恢复）
service.respond_prepare(&session, accepted_file_ids).await?;   // 接收确认
service.respond_file(&session, file_id, &path).await?;         // 可选：指定落盘路径
service.reject(&session).await?;
service.cancel(&session).await?;
service.stop().await?;
```

`TransferConfig` 在 skeleton 基础上**新增了两个字段**（均带 `serde(default)`，向后兼容）：

| 字段 | 说明 |
|---|---|
| `identity_dir` | 证书/私钥持久化目录；空则退化为临时目录。桌面端传 `app_data_dir/transfer`，手机端传 App 私有目录 |
| `verify_checksums` | 是否校验发送方给出的 SHA-256，默认 `true` |

## 测试

```powershell
cd crates/transfer-core
cargo test
```

- `tests/contract.rs`：事件/错误码 JSON 形状冻结
- `tests/loopback.rs`：两个实例通过 HTTPS v2 真实互传一个文件（含 TLS、prepare-upload、落盘、事件顺序）
- `src/path.rs`：文件名清洗（路径穿越、`..`、中文名、重名）

## 调试工具（examples）

```powershell
# 探测本机能不能做组播发现（换不同端口对比，能定位 Windows 端口排除段问题）
cargo run --example multicast_probe -- 53317

# 主动向某个对端发文件：验收对方的接收确认弹窗 / 排查自动发现失败
cargo run --example send -- 127.0.0.1 53317 .\a.pdf

# 起一个“自动接受”的对端：验收本端发送链路（落盘路径会打印出来）
cargo run --example serve -- E:\tmp\inbox 53319
```

## 被谁使用

```toml
# apps/desktop/src-tauri/Cargo.toml
rtodo-transfer-core = { path = "../../../crates/transfer-core" }

# apps/mobile/rust/Cargo.toml
rtodo-transfer-core = { path = "../../../crates/transfer-core" }
```

## 许可证

本 crate 为 r-todo 原创代码。协议实现来自 LocalSend `packages/core`（Apache-2.0），
保留其 `LICENSE` 与来源说明，见 `crates/vendor/localsend-core/VENDORED.md`。
