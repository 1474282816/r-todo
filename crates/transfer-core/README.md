# rtodo-transfer-core

r-todo 局域网互传的共享 Rust 核心（LocalSend 协议兼容）。

## 当前状态

**Skeleton**：类型、事件、错误码、API 已冻结；协议实现尚未接入。

- `TransferService::start` 只保存配置和事件通道，不绑定端口
- `send` / `respond_*` / `reject` / `cancel` 返回 `TransferError::NotImplemented`
- `devices()` 返回空列表
- 契约测试全部通过，壳层可以先用它打通 Tauri/FRB/Dart/Kotlin 链路

Desktop agent 负责把 `localsend` core 接进来；Mobile agent 只消费这里的 API。

## 契约

- `SCHEMA_VERSION = 1`
- 事件：`devices` / `incoming` / `progress` / `completed` / `error` / `session_ended`
- 错误码：`PORT_IN_USE` / `FIREWALL` / `MULTICAST_UNAVAILABLE` / `TLS` / `PIN_REQUIRED` / `PIN_INVALID` / `REJECTED` / `CHECKSUM_MISMATCH` / `CANCELLED` / `IO` / `UNKNOWN`
- 字段/错误码改动必须同步 `docs/handoff/2026-11-08-desktop-transfer.md`、`docs/handoff/2026-11-08-mobile-transfer.md` 和 `tests/contract.rs`

## 测试

```powershell
cd crates/transfer-core
cargo test
```

## 被谁使用

```toml
# apps/desktop/src-tauri/Cargo.toml
rtodo-transfer-core = { path = "../../../crates/transfer-core" }

# apps/mobile/rust/Cargo.toml
rtodo-transfer-core = { path = "../../../crates/transfer-core" }
```

## 许可证

本 crate 为 r-todo 原创代码。接入 LocalSend 的 `packages/core` / `localsend_isolates` 代码时，保留其 Apache-2.0 声明与修改说明。
