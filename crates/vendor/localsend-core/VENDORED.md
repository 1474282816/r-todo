# Vendored: LocalSend core (Rust)

本目录是 **LocalSend 官方 Rust core** 的原样副本，用于 r-todo 的局域网互传。

| 项 | 值 |
|---|---|
| 上游 | https://github.com/localsend/localsend |
| 子目录 | `packages/core`（crate 名 `localsend`） |
| Rev | `58955fd54ef1e18e89ec30e60f46754084a80942`（master，2026-11-08） |
| 许可证 | Apache-2.0，见本目录 `LICENSE` |
| 修改 | 新增 rtools 会话端点，并为 v2 register / prepare-upload 增加可选的 `host` 字段。旧客户端省略该字段，行为不变 |

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

## rtools 会话式扩展

会话式互传需要一条文本/元数据通道，而 v2 路由表是硬编码 `match`，壳层没有注入点。
为继续复用同一端口、同一套 mTLS 证书与证书钉住，这里做了**纯加法**：

| 文件 | 改动 |
|---|---|
| `src/http/server/chat.rs` | 新增：`GET /api/rtodo/v1/capabilities`、`POST /api/rtodo/v1/message`，把 JSON 交给壳层注册的 sink；发送方 fingerprint 取自 mTLS 证书 |
| `src/http/server/mod.rs` | 新增 `pub mod chat;` 与两条 match arm，其余分支不动 |
| `src/http/client/v2.rs` | 新增 `post_json()`，复用已有 reqwest client / TLS 配置 |
| `src/http/dto.rs`、`src/http/dto_v2.rs` | 新增可选 `host` 兼容字段，让手机在 NAT/端口转发后仍可上报可达局域网地址 |

既有 `/api/localsend/v2/*` 路由和单会话槽逻辑未改；`host` 使用可选字段，官方客户端
不发送时仍使用 TCP 来源地址，兼容性不变。升级上游时按上面「升级步骤」覆盖后，
重新应用本节改动即可。
