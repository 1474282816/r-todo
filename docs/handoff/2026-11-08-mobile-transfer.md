# Handoff · 手机端局域网互传（Flutter + Rust core）

> 基线：`master@9591256`（2026-11-08）
> 目标读者：接手实现的 agent
> 目标平台：Android（iOS 明确不做）
> 预计工作量：MVP 2~3 周，可日常使用 3~4 周
> 配套文档：`docs/handoff/2026-11-08-desktop-transfer.md`
> 共享 crate：`crates/transfer-core`（skeleton 已合入 master，契约测试通过）

---

## 1. 任务目标

在现有 r-todo Flutter 手机端加入**局域网文件互传**，与桌面端共用同一套 Rust core，协议兼容官方 LocalSend：

- 发现同一局域网内的设备（组播 + 手动 IP）
- 发送文件 / 多文件，显示进度，可取消
- 接收文件，接收前人工确认，保存到系统下载目录
- 与 r-todo 桌面端、官方 LocalSend 双向互通

### 非目标（本期不要做）

- iOS / iPad（不新增任何 Apple 代码）
- 手环导航 / MiNavigateRing 合并（另开 handoff；本期不要改 `MainActivity` 的既有业务，只在必要时注册 `TransferPlugin`）
- 用 Compose 或 Rust UI 重写手机端
- WebRTC、浏览器 Web 分享页
- 修改桌面端、updater、签名密钥、发版脚本
- 改造现有待办、同步、桌面小组件

---

## 2. 必读现状

### 2.1 手机端技术栈

- Flutter + Dart，状态管理 Riverpod，DB 用 Drift，UI 组件 ForUI
- 版本：`apps/mobile/pubspec.yaml` 为 `0.0.8+8`
- 本机工具链：**Flutter 3.47.8 / Dart 3.13.7**，Rust 1.99.1
- Android：`applicationId = dev.rtodo.r_todo_mobile`
  - AGP `9.0.1`、Gradle `9.1.0`、Kotlin `2.3.20`
  - `minSdk = flutter.minSdkVersion`（当前由 Flutter 模板决定）
- 已有能力，**不要破坏**：
  - 待办列表 / 编辑器：`lib/ui/home_page.dart`、`lib/ui/task_editor_sheet.dart`
  - 同步：`lib/sync/sync_service.dart`
  - 状态：`lib/state/task_store.dart`
  - 桌面小组件：`android/app/src/main/kotlin/.../widget/TodoWidget*Provider.kt`
  - 本地通知、应用内更新
- 当前**完全没有 Rust 桥**：仓库里没有 `flutter_rust_bridge` / `cargokit` / `rust_builder`

### 2.2 关键文件

```text
apps/mobile/
  pubspec.yaml
  lib/main.dart
  lib/state/task_store.dart
  lib/sync/sync_service.dart
  lib/ui/home_page.dart
  lib/ui/settings_page.dart
  android/app/build.gradle.kts
  android/app/src/main/AndroidManifest.xml
```

### 2.3 共享 core skeleton

`crates/transfer-core` 已合入 master，包含：

- `TransferConfig`、`DeviceInfo`、`DeviceChannel`、`DeviceSummary`
- `TransferEvent` + `EventEnvelope`（`SCHEMA_VERSION = 1`）
- `ErrorCode` / `ErrorScope` / `TransferError`
- `TransferService` 骨架：`start` 只保存配置与事件通道，`send` / `respond_*` 返回 `NotImplemented`
- 契约测试：`cargo test` 通过

手机端可以先用它打通 FRB → Dart → Kotlin 链路；协议实现由 Desktop agent 完成。

### 2.4 不要动的部分

- `lib/sync/`、`lib/state/`、`lib/data/`
- `android/.../widget/` 四个小组件
- 应用内更新、通知、桌面同步
- `apps/mobile/ios/`
- `apps/desktop/`、`apps/desktop/src-tauri/tauri.conf.json`
- `scripts/release-desktop.ps1`、`apps/desktop/latest.json`

---

## 3. 已定决策（不要重新讨论）

1. **保留 Flutter UI**，不迁 Compose / Rust UI。
2. **Rust 只做协议/传输核心**，Flutter 负责 UI，Kotlin 负责 Android 平台能力。
3. 与桌面端共用 `crates/transfer-core`（master 已提供 skeleton）：
   - 手写一个 FRB crate 通过 path 依赖它
   - 不复制一套协议实现
4. **Android only**：
   - Rust target：`aarch64-linux-android`、`armv7-linux-androideabi`、`x86_64-linux-android`
   - 不做 iOS target，不加 Apple 权限/entitlement
5. **LocalSend 协议**：
   - 上游 `https://github.com/localsend/localsend` 的 `packages/core`
   - git 依赖 pin rev，`features = ["discovery"]`
   - HTTP(S) v2，必要时 v3；端口默认 **53317**；HTTPS 自签 + fingerprint
   - 不做 WebRTC / Web 分享
6. **文件访问策略**：
   - Kotlin 负责 SAF / `content://` / MediaStore
   - **Rust 只接收/输出普通文件路径**
   - 发送：Kotlin 把选中的 URI 复制到 App cache，再把 cache path 交给 Rust
   - 接收：Rust 先写 App cache，再由 Kotlin 复制到 `Downloads/r-todo`（MediaStore）
   - 这是为了避开 `content://` 与 Rust 的耦合；代价是大文件多一次拷贝，先接受
7. **UI 入口**：新增“互传”页面，从首页/设置进入；不改现有待办主流程。
8. **接收确认**：未知设备必须人工确认；收藏设备自动接受默认关闭。
9. **许可证**：LocalSend 为 Apache-2.0；复制/改写其 Rust 代码需保留声明与修改说明；不要使用 LocalSend 名称/图标做产品名。

---

## 4. 目标架构

```text
crates/transfer-core/                 # master 已提供 skeleton，手机端 path 依赖
  Cargo.toml
  src/lib.rs
  src/event.rs
  ...

apps/mobile/rust/                       # FRB crate（本任务新建）
  Cargo.toml                            # 依赖 ../../../crates/transfer-core
  src/lib.rs
  src/api/transfer.rs
  flutter_rust_bridge.yaml

apps/mobile/lib/features/transfer/
  transfer_service.dart                 # 调 FRB + MethodChannel
  transfer_page.dart                    # 设备列表 / 发送 / 接收
  widgets/...

apps/mobile/android/app/src/main/kotlin/dev/rtodo/r_todo_mobile/transfer/
  TransferPlugin.kt                     # MethodChannel / EventChannel
  FileBridge.kt                         # SAF / 缓存 / MediaStore
  TransferForegroundService.kt          # 前台服务 + 通知
```

依赖写法：

```toml
# apps/mobile/rust/Cargo.toml
rtodo-transfer-core = { path = "../../../crates/transfer-core" }
```

### 4.1 分层

| 层 | 职责 |
|---|---|
| Flutter UI | 页面、设备列表、发送/接收交互、进度、设置 |
| FRB API | Rust ↔ Dart：启动/停止、发现、发送、接收决策、事件流 |
| `transfer-core` | LocalSend 协议、发现、TLS、会话、进度 |
| Kotlin 插件 | SAF、MediaStore、前台服务、通知、MulticastLock、权限 |
| Rust | 只处理路径与字节流，不碰 `content://` |

---

## 5. 对外契约

### 5.1 Dart API（建议）

```dart
class TransferConfig {
  final String alias;
  final int port;              // 53317
  final String receiveDirName; // "r-todo"
  final String? pin;
  final bool autoAcceptFavorites;
}

class TransferDevice {
  final String alias;
  final String fingerprint;
  final String? deviceModel;
  final String? deviceType;
  final List<TransferChannel> channels;
  final bool download;
}

sealed class TransferEvent {}
class TransferIncoming extends TransferEvent { ... }
class TransferProgress extends TransferEvent { ... }
class TransferCompleted extends TransferEvent { ... }
class TransferError extends TransferEvent { ... }
class TransferSessionEnded extends TransferEvent { ... }

abstract class TransferService {
  Future<void> start(TransferConfig config);
  Future<void> stop();
  Stream<List<TransferDevice>> get devices;
  Stream<TransferEvent> get events;
  Future<void> refresh();
  Future<void> addManualIp(String host, {int port = 53317});
  Future<String> send(List<String> stagedPaths, String fingerprint, {String? pin});
  Future<void> respondPrepare(String sessionId, List<String> acceptedFileIds);
  Future<void> respondFile(String sessionId, String fileId, String savePath);
  Future<void> reject(String sessionId);
  Future<void> cancel(String sessionId);
}
```

### 5.2 事件与错误码（必须与桌面端一致）

```text
transfer-devices
transfer-incoming
transfer-progress
transfer-completed
transfer-error
transfer-session-ended
```

Payload 使用 `schema_version: 1`，字段见桌面 handoff §5.2。手机端 Dart 层可以做类型化映射，但语义不要改。

错误码：

```text
PORT_IN_USE | FIREWALL | MULTICAST_UNAVAILABLE | TLS |
PIN_REQUIRED | PIN_INVALID | REJECTED | CHECKSUM_MISMATCH |
CANCELLED | IO | UNKNOWN
```

### 5.3 Kotlin MethodChannel

| 方法 | 入参 | 返回 | 说明 |
|---|---|---|---|
| `pickFiles` | `{multiple: true}` | `[{path, name, size, mime}]` | SAF 选择并复制到 cache |
| `saveToDownloads` | `{tempPath, fileName, mime}` | `{uri, displayName}` | MediaStore 写入 `Downloads/r-todo` |
| `acquireMulticastLock` | `{}` | `{}` | 发现期间持有 |
| `releaseMulticastLock` | `{}` | `{}` | 停止发现/退出释放 |
| `startForeground` | `{title, text}` | `{}` | 传输期间前台服务 |
| `stopForeground` | `{}` | `{}` | 传输结束停止 |
| `requestNotificationPermission` | `{}` | `{granted}` | Android 13+ |
| `cleanupCache` | `{paths}` | `{}` | 失败/取消后清理 |

> 文件选择也可以直接用 `file_picker`，但必须确认返回的是**可读的缓存路径**；如果拿到 `content://`，仍要走 Kotlin 复制到 cache。不要直接把 `content://` 传给 Rust。

---

## 6. 分阶段任务

### Phase 0 · Spike（1~2 天）

- [ ] 在 `apps/mobile/rust/` 建 FRB crate，path 依赖 `crates/transfer-core`
- [ ] 配置 `flutter_rust_bridge_codegen`，生成 Dart 绑定
- [ ] `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`
- [ ] 配置 Android NDK / cargokit（或 FRB 推荐的构建方式）
- [ ] `flutter build apk --debug` 通过
- [ ] 最小验证：手机起 server，桌面 r-todo / 官方 LocalSend 发一个文件
- [ ] 记录：FRB 版本、NDK 版本、Rust target、构建耗时、踩坑

**产出**：`docs/handoff/notes/mobile-transfer-spike.md`

### Phase 1 · Rust/FRB 服务（2~3 天）

- [ ] `TransferService` 封装 `transfer-core`：start/stop、设备流、事件流
- [ ] 身份持久化：证书/私钥存 App 私有目录
- [ ] 设置持久化：alias、port、PIN、自动接受
- [ ] 发送/接收/取消/拒绝 API
- [ ] 错误码映射
- [ ] Rust 单测：路径清洗、事件序列化
- [ ] Flutter 单测：Dart 层事件映射

### Phase 2 · Kotlin 平台层（2~3 天）

- [ ] `TransferPlugin` 注册 MethodChannel / EventChannel
- [ ] 在 `MainActivity.configureFlutterEngine` 注册 `TransferPlugin`（仅注册，不改既有业务）
- [ ] SAF 选择文件 → 复制到 `cacheDir/transfer/outbox/`
- [ ] 接收文件 → MediaStore `Downloads/r-todo`；API < 29 的回退
- [ ] 前台服务 + 通知（进度/完成/失败）
- [ ] `MulticastLock` 获取/释放
- [ ] Android 13+ 通知权限申请；Android 14 FGS 类型声明
- [ ] 清理 cache（成功/失败/取消/启动时）
- [ ] AndroidManifest 权限与 service 注册

### Phase 3 · Flutter UI（3~5 天）

- [ ] “互传”页面：开关、设备列表、刷新、手动 IP、收藏
- [ ] 发送：选择文件、选择设备、进度、取消、完成后打开目录
- [ ] 接收：确认弹窗（设备/文件/大小）、保存、拒绝
- [ ] 设置：alias、端口、PIN、自动接受、接收目录说明
- [ ] 空态/错误态：无设备、组播失败、端口占用、权限拒绝、存储不足
- [ ] 与现有 ForUI 风格一致；不要引入第二套设计系统

### Phase 4 · 互通与边界（3~5 天）

- [ ] Android r-todo ↔ Windows r-todo 双向
- [ ] Android r-todo ↔ 官方 LocalSend（双向）
- [ ] 后台传输（App 退到后台，FGS 保持）
- [ ] 锁屏/切网/VPN/AP 隔离
- [ ] 0 字节、1GB+、中文名、重名、多文件
- [ ] 取消/拒绝/断线/存储不足
- [ ] 现有待办、同步、小组件、通知、更新不受影响

---

## 7. Android 关键细节

### 7.1 文件与存储

- 发送：`ACTION_OPEN_DOCUMENT` → `ContentResolver.openInputStream` → 复制到 `cacheDir/transfer/outbox/<uuid>`
- 接收：Rust 写 `cacheDir/transfer/inbox/<uuid>`，Kotlin 再写入 MediaStore：
  - API 29+：`MediaStore.Downloads`，相对路径 `Download/r-todo`
  - API 24~28：`Environment.getExternalStoragePublicDirectory(DIRECTORY_DOWNLOADS)/r-todo`，需要 `WRITE_EXTERNAL_STORAGE`
- 文件名清洗 + 重名加序号；不要覆盖已有文件
- 传输结束/失败/取消都要清理 cache；启动时清理超过 24h 的残留
- 大文件注意磁盘空间，开始前检查可用空间

### 7.2 前台服务

- 传输开始 → `startForeground`，通知显示当前文件/进度
- 传输结束/失败/取消 → `stopForeground` + 清理
- Android 14+ 声明 `android:foregroundServiceType="dataSync"`，并加 `FOREGROUND_SERVICE_DATA_SYNC` 权限
- 通知渠道单独创建，低打扰
- 不要常驻前台服务；只在传输期间

### 7.3 组播与网络

- 发现期间持有 `WifiManager.MulticastLock`，停止时释放；没有它部分机型收不到组播
- 需要 `CHANGE_WIFI_MULTICAST_STATE`
- AP 隔离/访客 Wi-Fi/VPN 会导致发现不到；UI 提供手动 IP
- 端口 53317 与官方 LocalSend 冲突时明确报错
- 按 targetSdk 适配新版 Android 的本地网络/附近设备权限；在目标机型上实测

### 7.4 OEM 后台限制

- 小米/HyperOS、华为等会杀后台；传输期间用前台服务 + 可选电池优化白名单引导
- 不要强制申请“忽略电池优化”，给出说明让用户选择

---

## 8. 构建与测试

```powershell
cd apps/mobile
flutter pub get
flutter analyze
flutter test

# Rust 侧
cd rust
cargo test

# Android 调试包
flutter build apk --debug

# 真机运行
flutter run
```

FRB 代码生成（具体命令以当前 FRB 版本为准）：

```powershell
flutter_rust_bridge_codegen generate
```

- 生成文件不要手改；提交生成结果前确认团队约定
- Rust target / NDK 版本写进 Spike 记录
- 优先 `arm64-v8a` 真机；模拟器用 `x86_64`
- 发布包用 ABI split 或 `abiFilters`，避免 APK 过大

---

## 9. 风险

1. **Rust Android 构建链**：NDK、`ring`/`rustls`、cargokit、target 安装、Windows 路径/长路径；先用最小工程验证。若 cargokit 对 `crates/transfer-core` 的 workspace 外 path 依赖不友好，允许 vendor 到 `apps/mobile/rust/vendor/`，但不要复制两套协议实现。
2. **FRB 版本兼容**：Flutter 3.47.8 / Dart 3.13.7 与 FRB 版本要匹配；生成代码与运行时不匹配会崩。
3. **SAF 双拷贝**：大文件多一次磁盘写入；需要进度/空间检查/清理。
4. **前台服务与权限**：Android 13 通知权限、Android 14 FGS 类型、Android 16 本地网络限制；不同 OEM 行为不同。
5. **MulticastLock**：没有它部分设备发现不到；不要只在模拟器验证。
6. **App 生命周期**：Flutter 页面销毁不代表 Rust 服务停止；需要显式 stop/恢复状态，避免 UI 与后台状态不一致。
7. **端口冲突**：官方 LocalSend 同时运行时 53317 被占；UI 明确提示。
8. **不要破坏现有功能**：同步、Drift、小组件、通知、更新都要回归。
9. **iOS 不在范围**：不要为了“以后”抽象 Apple 代码，先保持 Android-only。

---

## 10. 验收清单

- [ ] r-todo Android → r-todo Windows 发送/接收
- [ ] r-todo Windows → r-todo Android 发送/接收
- [ ] r-todo Android ↔ 官方 LocalSend 双向
- [ ] 后台传输：App 切后台/锁屏，FGS 保持，通知进度正常
- [ ] 权限拒绝（通知/存储）有可理解的提示
- [ ] 0 字节 / 1GB+ / 中文名 / 重名 / 多文件
- [ ] 取消 / 拒绝 / 断线 / 存储不足
- [ ] 端口占用、组播失败、手动 IP 可用
- [ ] 现有待办、同步、四个小组件、通知、应用内更新正常
- [ ] `flutter analyze`、`flutter test`、`flutter build apk` 通过
- [ ] 没有修改 iOS / 桌面端 / updater / 发版脚本

---

## 11. 协作约定

- 分支：`feat/mobile-transfer`
- 与桌面端共用 `crates/transfer-core`：
  - 桌面 agent 先建；手机 agent 只 path 依赖，不复制协议代码
  - 事件/错误码变更必须同步桌面 handoff 的消费者
- 不要提交：`build/`、`.dart_tool/`、Rust `target/`、cache 样本、密钥
- 不要修改 `apps/desktop/src-tauri/tauri.conf.json` 的 updater / `latest.json` / `scripts/release-desktop.ps1`
- 完成后在本文档追加“实现记录”：FRB/NDK/Rust 版本、构建命令、已知差异、未完成项
- 需要用户决策的点（默认接收目录、自动接受、PIN 默认值）先记录，不要擅自改产品行为

---

## 12. 并行开发约定（桌面 + 手机）

两端可以同时开发，前提是**共享 core 单归属**：

| 范围 | Owner | 规则 |
|---|---|---|
| `crates/transfer-core` 内部实现 | Desktop agent | 只有 desktop agent 改 `service.rs` / 协议实现 |
| `apps/mobile`、FRB、Kotlin | Mobile agent | 只消费 core API；需要新 API 先提给 desktop agent |
| 事件/错误码/`SCHEMA_VERSION` | 双方 | 任何改动必须同步两端 + `tests/contract.rs` |

工作区：

```powershell
# 由主仓库统一创建，两个 agent 各进自己的目录
git worktree add .worktrees/desktop -b feat/desktop-transfer
git worktree add .worktrees/mobile  -b feat/mobile-transfer
```

合并顺序：

1. `crates/transfer-core` skeleton（已在 master）
2. Desktop 的 core 实现 + Tauri
3. Mobile rebase 后接真实 core（之前可用 skeleton/mock 开发 UI）

硬性禁止：

- 两个 agent 同时修改 `crates/transfer-core`
- 各自复制一套协议实现
- 单方面改事件字段/错误码

契约测试：

```powershell
cd crates/transfer-core
cargo test
```
