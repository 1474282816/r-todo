# 设计文档 · 手机端局域网互传

> 输入规格：`docs/handoff/2026-11-08-mobile-transfer.md`
> 基线：`feat/mobile-transfer`（worktree `.worktrees/mobile`，2026-11-08）
> 范围：Android only；iOS / 桌面端 / updater 不改

## 1. 分层与数据流

```text
Flutter UI (lib/features/transfer/)
  TransferPage / IncomingSheet / SettingsSection
        │  Riverpod providers
        ▼
TransferService facade (Dart)
  ├─ FRB: MobileTransfer (Rust 实例)   ── 事件 JSON 字符串流
  └─ MethodChannel r_todo/transfer     ── 文件与平台能力
        ▼
apps/mobile/rust/  (FRB crate，path 依赖 crates/transfer-core)
  只做：配置校验、事件转发(EventEnvelope → JSON)、错误码映射
        ▼
crates/transfer-core (Desktop agent 独占实现，本端只消费)
```

Kotlin 不参与协议与事件：只提供 SAF、MediaStore、前台服务、通知、MulticastLock。

## 2. 决策（含备选与取舍）

### D1 构建集成：自建 Gradle `cargo ndk` 任务，不用 cargokit（已按此落地，见 spike 记录）

- 备选 A：`flutter_rust_bridge_codegen integrate` 生成 `rust_builder` + cargokit。
- 备选 B：`app/build.gradle.kts` 注册 `buildRustLibs` 任务，调用 `cargo ndk -o src/main/jniLibs`。
- 采用 **B**：当前工程是 AGP 9.0.1 / Gradle 9.1.0 / Kotlin 2.3.20，cargokit 依赖的旧 Gradle API 风险高；B 只依赖 `cargo-ndk`，行为可预测、可增量。
- 代价：开发者需装 `cargo-ndk` 与三个 rust target；写进 README/spike 记录。
- 回退：若 `cargo ndk` 在 Windows/NDK 下失败，再评估 A。

### D2 Rust↔Dart 边界用「JSON 字符串 + Dart 类型化映射」

- 备选：用 FRB 镜像 core 的 `TransferEvent` 枚举（Dart sealed class 直出）。
- 采用 JSON：`EventEnvelope` 的 JSON 形状已被 `crates/transfer-core/tests/contract.rs` 冻结，Rust 侧只需透传；避免 `PathBuf`、`serde` tag 与 FRB 类型系统的耦合，也避免桌面端改 core 时本端跟着重生成。
- Dart 侧 `TransferEventMapper` 负责类型化（单测覆盖），语义字段一个不改。

### D3 FRB 只暴露一个 opaque 实例

`MobileTransfer` 内部持 `RwLock<Option<Arc<TransferService>>>`：

- 普通方法取读锁 → clone Arc → 释放锁后再 `await`（`send` 可长跑，不阻塞 `cancel`/`respond_*`）。
- `stop` 取写锁 → take → `Arc::try_unwrap` 成功则调用 core 的 `stop(self)`。
- 不用全局单例：Dart 侧由 Riverpod 持有实例，生命周期清晰，也便于测试。
- 若 Desktop agent 给 `TransferService` 加了非 `Send + Sync` 字段，FRB 会编译报错，届时改为在本 crate 内包一层 `Mutex`（不改 core）。

### D4 事件桥

`start` 时创建 `tokio::sync::mpsc::channel(256)` → `TransferService::start(config, tx)`；随后 `tokio::spawn` 一个转发任务：

```text
while let Some(env) = rx.recv().await { sink.add(env.to_json_string())?; }
```

- 同时把最近一次 `devices` 事件缓存在 `MobileTransfer` 里，`devices_json()` 供新页面首屏读取（避免先开页后开流丢设备）。
- `stop` 时 drop sender 侧 / 关闭 sink，转发任务自然退出。

### D5 设置持久化放 Flutter

alias / port / pin / autoAccept 用已有的 `shared_preferences`，`start()` 时传入；Rust 不落设置。
证书与私钥属于 core（Desktop agent 实现），本端只提供 App 私有目录路径，落盘交给 core —— **当前阻塞项**。

### D6 Kotlin 桥

单 `MethodChannel("r_todo/transfer")`，按 handoff §5.3 的方法表实现；传输事件不走 Kotlin。
前台服务只在传输期间存在，进度文案由 Dart 调 `startForeground` 反复更新同一条通知（不新增方法）。

### D7 UI 入口与风格

设置页新增「互传」行 → `TransferPage`；风格复用 `FScaffold` + `FHeader.nested` + 现有列表行密度，不引入 Material AppBar/FAB/第二套图标与配色，标题「互传」，不出现 LocalSend 字样。

### D8 ABI

默认构建 `arm64-v8a` + `armeabi-v7a` + `x86_64`，与 Flutter 默认打包的 ABI 集合一致——避免 APK 里某个 ABI 只有 sqlite3 没有 `librtodo_mobile_transfer.so`（native assets 不受 `abiFilters` 约束，实测会出现这种半套 ABI）。
需要缩短构建时间时用 `-PrtodoAbis=arm64-v8a,x86_64`（此时 `abiFilters` 与实际产物同步收窄）。

## 3. 阻塞与依赖

1. **`crates/transfer-core` 仍是 skeleton**：`send`/`respond_*`/`add_manual_ip` 返回 `NotImplemented`，`devices()` 为空。本端先打通链路 + UI + 测试，真实收发与互通验收必须等 Desktop agent 实现后回归。
2. **本机无 Android 设备/模拟器**（`adb devices` 空，无 AVD）：Phase 0 只能验证到 `flutter build apk --debug` 与 Rust 交叉编译，运行时验证需用户插真机或装 AVD。
3. **`crates/transfer-core` 无 workspace**（仓库根无 `Cargo.toml`）：`apps/mobile/rust` 用 path 依赖即可，但要自带 `Cargo.lock`，且 `target/` 不能进库。

## 4. 风险

| 风险 | 应对 |
|---|---|
| FRB 2.13 + Flutter 3.47.8/Dart 3.13.7 不匹配 | 版本锁定 2.13.0（Dart SDK 要求 ≥3.9.2 满足），生成代码与运行时同版本 |
| NDK 版本：Flutter 默认 28.2.13676358，本机只有 30.0.14904198 | 用 sdkmanager 装 28.2；不改 `ndkVersion` 以免影响既有构建 |
| AGP 9 与 jniLibs 目录/abiFilters 交互 | 先只跑 `assembleDebug` 验证产物，再调 abiFilters |
| `transfer-core` 若引入 `ring`/`rustls`，Android 交叉编译失败 | 由 Desktop agent 负责依赖可编译性；本端在 CI 里跑三 target `cargo check` 尽早暴露 |
| 前台服务 / 通知权限在 Android 13~16 行为差异 | Manifest 全量声明 + 运行时申请；无设备时至少 build 通过并在文档标注待实测 |
| 生命周期：Flutter 页面销毁 ≠ 服务停止 | provider 常驻 + 显式 stop；页面 initState 只在未启动时 start |

## 5. 未决（记录，不擅自改产品行为）

- 默认接收目录：沿用 `Downloads/r-todo`（handoff 已定）。
- 自动接受收藏设备：默认关闭（core 默认 `auto_accept_favorites = false`）。
- PIN：默认为空（core 默认 `None`）。
