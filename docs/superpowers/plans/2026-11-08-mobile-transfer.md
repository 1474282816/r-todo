# 实施计划 · 手机端局域网互传（Android）

> 设计：`docs/handoff/notes/mobile-transfer-design.md`
> 规格：`docs/handoff/2026-11-08-mobile-transfer.md`
> 分支：`feat/mobile-transfer`（worktree `.worktrees/mobile`）
> 纪律：每个任务先写失败测试（TDD），再写最少实现；每个任务单独可验证、可提交。

## 进度

| 项 | 状态 |
|---|---|
| Phase 0（T0.1~T0.6） | ✅ 完成，见 `docs/handoff/notes/mobile-transfer-spike.md` |
| T1.1 配置校验（Rust 5 例） | ✅ |
| T1.2 事件桥（Rust 4 例） | ✅ |
| T1.3 FRB API 门面（Rust 6 例，含 two-phase subscribe） | ✅ |
| T1.4 Dart 事件模型（Dart 9 例） | ✅ |
| T1.5 Dart 服务门面 + Riverpod（Dart 12 例） | ✅ |
| T1.6 错误文案（Dart 5 例） | ✅ |
| Phase 2 Kotlin 平台层 | ✅ 代码完成、编译通过；运行态待真机 |
| Phase 3 Flutter UI（页面/入口/设置） | ✅ 代码完成；widget 测试 2 例；雷电模拟器实测通过（见 spike §6） |
| Phase 4 互通与边界 | ⏳ core 已接入、release 包已产出；双向互通仍需真机（雷电 NAT 只能出站） |
| 文档（spike / handoff 实现记录 / CHANGES） | ✅ |

## Phase 0 · Spike（先证明构建链能跑）

### T0.1 安装 Rust Android 工具链
- 命令：
  ```powershell
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
  cargo install cargo-ndk --locked
  cargo install flutter_rust_bridge_codegen --version 2.13.0 --locked
  D:\Android\cmdline-tools\latest\bin\sdkmanager.bat "ndk;28.2.13676358"
  ```
- 验证：`rustup target list --installed` 含三个 target；`cargo ndk --version`；`D:\Android\ndk` 出现 28.2.13676358。

### T0.2 新建 FRB crate
- 文件：`apps/mobile/rust/Cargo.toml`
  ```toml
  [package]
  name = "rtodo_mobile_transfer"
  version = "0.1.0"
  edition = "2021"
  publish = false

  [lib]
  crate-type = ["cdylib", "staticlib", "rlib"]

  [dependencies]
  rtodo-transfer-core = { path = "../../../crates/transfer-core" }
  flutter_rust_bridge = "2.13.0"
  tokio = { version = "1", features = ["sync", "rt", "macros"] }
  serde_json = "1"
  ```
- 文件：`apps/mobile/rust/src/lib.rs`（`mod api; mod frb_generated;`）、`apps/mobile/rust/src/api/mod.rs`、`apps/mobile/rust/src/api/transfer.rs`（先放 `pub fn ping() -> String`）
- 验证：`cargo check --manifest-path apps/mobile/rust/Cargo.toml`
- 验证（交叉编译）：`cargo ndk -t arm64-v8a -o tmp/jniLibs check --manifest-path apps/mobile/rust/Cargo.toml`（失败则记录到 spike 文档并按 D1 回退）

### T0.3 FRB 代码生成
- 文件：`apps/mobile/rust/flutter_rust_bridge.yaml`
  ```yaml
  rust_input: crate::api
  rust_root: rust/
  dart_output: lib/src/rust
  ```
- 命令（在 `apps/mobile`）：`flutter_rust_bridge_codegen generate`
- 验证：`lib/src/rust/` 下出现 `api/transfer.dart`、`frb_generated.dart`；`flutter analyze` 通过。

### T0.4 Gradle 集成 Rust 产物
- 文件：`apps/mobile/android/app/build.gradle.kts`
  - 注册 `buildRustLibs`（`Exec`，调用 `cargo ndk -o <app>/src/main/jniLibs build`），`preBuild` 依赖它；ABI 默认 `arm64-v8a,x86_64`，可用 `-PrtodoAbis=` 覆盖。
- 验证：`flutter build apk --debug`；`unzip -l app-debug.apk | findstr librust_mobile_transfer` 出现 `arm64-v8a` 与 `x86_64`。

### T0.5 Dart 启动与最小验证
- 文件：`apps/mobile/lib/main.dart`：`await RustLib.init()`（`main` 改 `async`，`WidgetsFlutterBinding.ensureInitialized()` 之后）
- 验证：`flutter analyze` 通过；无设备时不跑运行时，写入待实测清单。

### T0.6 Spike 记录
- 文件：`docs/handoff/notes/mobile-transfer-spike.md`
- 内容：FRB/NDK/Rust/AGP 版本、构建命令、构建耗时、踩坑、未实测项。

## Phase 1 · Rust/FRB 服务 + Dart 映射（TDD）

### T1.1 配置校验（Rust 单测先行）
- `apps/mobile/rust/src/core/config.rs`：`alias` 去空白后非空、`port != 0`、`receive_dir` 非空；失败返回结构化错误 JSON。
- 测试：`cargo test -p rtodo_mobile_transfer` 三条断言（空 alias / port 0 / 正常）。

### T1.2 事件桥（Rust 单测先行）
- `src/core/bridge.rs`：`EventEnvelope` → JSON string；断言 `schema_version == 1`、`type` 字段与 `tests/contract.rs` 一致。
- 测试：progress/incoming/devices/error/session_ended 五种信封各一条。

### T1.3 API 封装
- `src/api/transfer.rs`：`MobileTransferConfig`、`MobileTransfer`（D3 的锁策略）、`start/stop/devices_json/refresh/add_manual_ip/send/respond_prepare/respond_file/reject/cancel`。
- 错误映射：`TransferError` → `{"code":"PIN_INVALID","scope":"receive","message":"..."}`（复用 core 的 `code()/scope()`）。
- 测试：skeleton 下 `send` 返回 `UNKNOWN` 码；`start` 后 `devices_json()` 为 `[]`；`stop` 后再次调用返回 `UNKNOWN`（服务未启动）。

### T1.4 Dart 事件模型（Dart 单测先行）
- `lib/features/transfer/transfer_event.dart`：sealed `TransferEvent` + 六个子类 + `parseTransferEvent(String json)`。
- 测试：`test/transfer_event_test.dart` 用与 core 契约测试相同的 JSON 样例，断言类型与字段。

### T1.5 Dart 服务门面
- `lib/features/transfer/transfer_service.dart`：包装 FRB `MobileTransfer` + `shared_preferences` 设置；`Stream<TransferEvent> events`、`Stream<List<TransferDevice>> devices`。
- 测试：设置读写往返；事件流转换（注入假 JSON）。

### T1.6 错误文案
- `lib/features/transfer/transfer_error_text.dart`：11 个错误码 → 中文提示；测试覆盖每个码都有文案。

## Phase 2 · Kotlin 平台层（TDD 只覆盖纯逻辑）

### T2.1 文件名清洗（JVM 单测先行）
- `android/app/src/main/kotlin/.../transfer/FileNames.kt`：去非法字符、保留中文、重名 `(1)` 递增、长度截断。
- 测试：`android/app/src/test/kotlin/.../FileNamesTest.kt`（Gradle `testDebugUnitTest`）。

### T2.2 TransferPlugin + Manifest
- `TransferPlugin.kt`：`r_todo/transfer` MethodChannel，方法表按 handoff §5.3；`pickFiles` 用 `ActivityResultContracts.OpenMultipleDocuments`。
- `MainActivity.configureFlutterEngine` 只加一行注册（不动既有业务）。
- `AndroidManifest.xml`：INTERNET/ACCESS_NETWORK_STATE/ACCESS_WIFI_STATE/CHANGE_WIFI_MULTICAST_STATE/POST_NOTIFICATIONS/FOREGROUND_SERVICE/FOREGROUND_SERVICE_DATA_SYNC/WRITE_EXTERNAL_STORAGE(maxSdk 28) + service 声明（`foregroundServiceType="dataSync"`）。
- 验证：`flutter build apk --debug`。

### T2.3 FileBridge
- SAF → `cacheDir/transfer/outbox/`；`saveToDownloads` → MediaStore `Download/r-todo`（API 29+）/ 公共 Downloads（24~28）；`cleanupCache` + 启动清理 >24h。
- 验证：编译 + 真机待测（无设备时记录）。

### T2.4 前台服务与通知
- `TransferForegroundService.kt`：独立渠道、低打扰、`startForeground(..., FOREGROUND_SERVICE_TYPE_DATA_SYNC)`；`stopForeground` 结束清理。

### T2.5 MulticastLock / 通知权限
- `acquire/releaseMulticastLock`、`requestNotificationPermission`（13+）。

## Phase 3 · Flutter UI

- `transfer_page.dart`：开关、设备列表、刷新、手动 IP、收藏。
- `widgets/device_tile.dart`、`widgets/incoming_sheet.dart`（设备/文件/大小 + 接受/拒绝）、`widgets/progress_row.dart`。
- `settings_page.dart` 增加「互传」入口行；`transfer_page` 的 alias/端口/PIN/自动接受设置区块。
- 空态/错误态：无设备、组播失败、端口占用、权限拒绝、存储不足文案。
- 测试：`test/transfer_page_test.dart`（无设备空态、错误态渲染），以及上述纯逻辑单测。
- 验证：`flutter analyze` + `flutter test` + `flutter build apk --debug`。

## Phase 4 · 互通与验收（依赖 Desktop core + 真机）

- Android r-todo ↔ Windows r-todo；Android r-todo ↔ 官方 LocalSend（双向）。
- 后台传输/锁屏/切网/VPN；0 字节、1GB+、中文名、重名、多文件；取消/拒绝/断线/存储不足。
- 回归：待办、同步、四个小组件、通知、应用内更新。
- 在 handoff 追加「实现记录」。

## 提交节奏

每个 T 编号一次提交（测试+实现一起），消息格式 `feat(mobile-transfer): <任务>`；Phase 0 结束后立刻跑一次 `cargo test` + `flutter analyze`。
