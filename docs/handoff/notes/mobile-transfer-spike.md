# Spike 记录 · 手机端互传（Phase 0）

> 日期：2026-11-08 · 分支 `feat/mobile-transfer` · 机器：Windows 11（本机开发机）

## 1. 版本矩阵

| 组件 | 版本 | 说明 |
|---|---|---|
| Flutter / Dart | 3.47.8 / 3.13.7 | pub 镜像 `pub.flutter-io.cn` |
| Rust | 1.99.1（stable, x86_64-pc-windows-msvc） | |
| flutter_rust_bridge | 2.13.0（Rust crate + Dart 包 + codegen 同版本） | Dart SDK 要求 ≥3.9.2，满足 |
| cargo-ndk | 4.1.2 | |
| Android NDK | 28.2.13676358 | Flutter 3.47.8 默认 `flutter.ndkVersion`；另装有 30.0.14904198 但不用 |
| Android SDK | 36.1.0（platform android-37.0） | AGP 9.0.1 |
| Gradle / Kotlin | 9.1.0 / 2.3.20 | |
| JDK | Temurin 21.0.12 | |

Rust target（`rustup target add`）：`aarch64-linux-android`、`armv7-linux-androideabi`、`x86_64-linux-android`。

## 2. 采用的构建方式

`apps/mobile/rust` 是独立 crate（非 cargo workspace，仓库根无 `Cargo.toml`），path 依赖 `../../../crates/transfer-core`。

**不用 cargokit**，在 `apps/mobile/android/app/build.gradle.kts` 注册 `buildRustLibs`（`Exec` 任务）：

```text
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o <app>/src/main/jniLibs build
```

- `preBuild` 依赖它，`flutter build apk` 自动触发
- 默认 debug profile；`-PrtodoRustProfile=release` 切 release
- ABI 可用 `-PrtodoAbis=arm64-v8a,x86_64` 覆盖（默认三 ABI，与 Flutter 打包 ABI 一致）
- `src/main/jniLibs/` 是生成产物，已加入 `.gitignore`

产物：`librtodo_mobile_transfer.so`，三 ABI 均存在；debug 原始 41MB，AGP strip 后 2.1MB（arm64）/ 1.1MB（v7a）/ 2.3MB（x64）。

## 3. 命令与实测耗时

```powershell
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install cargo-ndk --locked
cargo install flutter_rust_bridge_codegen --version 2.13.0 --locked
D:\Android\cmdline-tools\latest\bin\sdkmanager.bat --sdk_root=D:\Android "ndk;28.2.13676358"

cd apps/mobile
flutter pub add flutter_rust_bridge:2.13.0
flutter_rust_bridge_codegen generate          # 生成 rust/src/frb_generated.rs + lib/src/rust/
flutter build apk --debug                     # 首次 66s（Gradle 预热另算），增量 ~20s
```

验证结果：

- `cargo test --lib` 16 passed
- `flutter analyze` 仅 2 条既有 info（`database.dart` / `home_page.dart`）
- `flutter test` 23 passed
- `flutter build apk --debug` 通过；APK 内三 ABI 均有 `librtodo_mobile_transfer.so`

## 4. 踩坑与解决

1. **Gradle 路径**：`rootProject` 是 `apps/mobile/android`，Rust 目录是 `rootProject.file("../rust")`（不是 `../../rust`）。
2. **Kotlin 增量缓存报错**（`:xxx:compileDebugKotlin` → "Could not close incremental caches"）：本机 AGP 9 + Kotlin 2.3.20 组合复现，`android/gradle.properties` 加 `kotlin.incremental=false` 后稳定通过（只影响构建速度）。
3. **sqlite3 3.5.0 native assets 从 GitHub 下载被墙**（`libsqlite3.*.android.so` / `sqlite3.x64.windows.dll`）：构建前用镜像把文件放进
   `.dart_tool/hooks_runner/shared/sqlite3/build/download-<sha256 前 8 位>/<库名>`，hook 校验哈希后直接复用。
   - Android：`libsqlite3.{arm,arm64,x64}.android.so` → `download-*/libsqlite3.so`
   - Windows 主机（`flutter test` 需要）：`sqlite3.x64.windows.dll` → `download-*/sqlite3.dll`
   - 这是本机网络限制，不是项目缺陷；换到能访问 GitHub 的机器可省略。
4. **FRB 2.13 的 `StreamSink` 不再从 crate 根导出**：`use crate::frb_generated::StreamSink;`。
5. **codegen 需要 rustfmt**：`rustup component add rustfmt`，否则只告警（生成文件不格式化）。
6. 生成代码（`lib/src/rust/`、`rust/src/frb_generated.rs`）**入库提交**，避免 CI/新机器必须装 codegen。

## 5. 未实测（本机无 Android 设备/AVD）

代码已按 Phase 1~3 完成（Rust 门面、Dart 控制器、Kotlin 插件、互传页面），但下列**运行态**只能在真机上验证：

- `RustLib.init()` 与 `start` / `subscribe` / 事件流
- 后端/前台服务、通知渠道与进度、Android 14 `dataSync` 类型
- SAF 选择 → cache/outbox 复制 → `send`
- 接收 → cache/inbox → MediaStore `Downloads/r-todo`（含 API < 29 分支）
- MulticastLock 对设备发现的影响、AP 隔离/访客网络
- 与桌面端 r-todo、官方 LocalSend 的互通
- 取消/拒绝/断线/存储不足/大文件

需要用户插真机（`adb devices` 当前为空）或装 AVD 后回归；验收清单见 `docs/handoff/2026-11-08-mobile-transfer.md` 第 10 节。

## 6. 雷电模拟器实测（2026-11-08）

本机装了 **雷电模拟器 14**（`G:\leidian\LDPlayer14`，实例 `leidian0`），可直接用来验壳层：

```powershell
& "G:\leidian\LDPlayer14\ldconsole.exe" launch --index 0
adb connect 127.0.0.1:5555          # 实例 0 默认端口；等启动完成再连
adb -s emulator-5554 install -r build\app\outputs\flutter-apk\app-debug.apk
```

环境：`x86_64` / Android 14（SDK 34）/ 2560x1440 @320dpi（当前旋转为竖屏 1440x2560）。

已验证：

- `RustLib.init()` 成功（日志无「互传 Rust 初始化失败」），FRB 调用链路可用
- 互传页渲染、开关启停、通知权限弹窗（Android 13+ 首次开启触发）
- 手动 IP 弹窗 → 错误横幅（core skeleton 返回 `UNKNOWN`）
- 互传设置改名保存 → 杀进程重启后仍生效
- 底部提示、空态文案
- 底部 Tab（待办 / 互传）切换、互传 Tab 右上角进互传设置、两个 Tab 按返回都直接退到桌面

**注意**：模拟器默认 NAT 网络，组播发现基本不可用、外部设备也无法直连模拟器；要测真实互通需要雷电的「桥接模式」，或直接用真机。

顺带发现（**既有问题，与互传无关**）：本机模拟器启动时 `home_widget` 抛
`PlatformException: null cannot be cast to non-null type kotlin.Number`
（`HomeWidgetPlugin.kt:176`，来自 `initWidget()` 的 `registerInteractivityCallback`），不影响待办主流程，但建议后续单开一条修。

## 7. 真机安装包（2026-11-08 晚）

真实 core 已同步到本 worktree（`crates/transfer-core` + `crates/vendor/localsend-core`，逐文件复制，未改协议代码）。

```powershell
cd apps/mobile
flutter build apk --release                 # app-release.apk，81.4MB，全 ABI
flutter build apk --release --split-per-abi # 每 ABI 一个，手机首选 arm64-v8a（29.6MB）
```

- `assembleRelease` 自动让 `buildRustLibs` 用 `cargo ndk ... build --release`
- `--split-per-abi` 与自定义 `ndk.abiFilters` 互斥，Gradle 里已按 `split-per-abi` 属性跳过后者
- 签名：`build.gradle.kts` release 用 debug signingConfig；`apksigner verify --verbose` 显示 v2 通过
- `aapt2 dump badging`：`dev.rtodo.r_todo_mobile` / versionName 0.0.8 / minSdk 24 / targetSdk 36 / 互传权限齐全
- release 包内 `lib/arm64-v8a/librtodo_mobile_transfer.so` 7.53MB（优化+strip 后），另含 `libflutter.so`、`libsqlite3.so`

**注意**：手机若已装过不同签名的 r-todo，先卸载再装；装完需允许通知权限与「本地网络/附近设备」（Android 16 视机型）。
