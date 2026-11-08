# Vendored: LocalSend core (Rust)

本目录是 **LocalSend 官方 Rust core** 的原样副本，用于 r-todo 的局域网互传。

| 项 | 值 |
|---|---|
| 上游 | https://github.com/localsend/localsend |
| 子目录 | `packages/core`（crate 名 `localsend`） |
| Rev | `58955fd54ef1e18e89ec30e60f46754084a80942`（master，2026-11-08） |
| 许可证 | Apache-2.0，见本目录 `LICENSE` |
| 修改 | **无**。仅整体拷贝，未改任何一行代码 |

## 为什么 vendor 而不是 git 依赖

上游仓库带 `support/submodules/flutter` 子模块。cargo 在解析任何 git 依赖时都会递归
fetch 该子模块（Flutter 仓库，GB 级），本机网络必然失败；`[patch]` 指向本地路径也
无法跳过这一步（cargo 仍会先 fetch 原仓库）。

因此源码 vendor 到本目录，由 `crates/transfer-core/Cargo.toml` 以 path 依赖消费：

```toml
localsend = { path = "../vendor/localsend-core", features = ["discovery"] }
```

## 升级步骤

1. 取上游新 rev，只检出 `packages/core`：
   `git clone --depth 1 --filter=blob:none --sparse <repo> && git sparse-checkout set packages/core`
2. 用 `packages/core` 覆盖本目录（保留 `LICENSE` 与本文件）
3. 同步更新 `crates/transfer-core/Cargo.toml` 注释里的 rev
4. `cd crates/transfer-core && cargo test`（含 `tests/loopback.rs` 回环互传）

## 使用范围

只启用 `features = ["discovery"]`（= `http` + `crypto` + `multicast`），不使用 WebRTC
与 Web 分享页。`lib.rs` 里无条件 `pub mod webrtc;` 已确认内部被 feature 门控，编译通过。
