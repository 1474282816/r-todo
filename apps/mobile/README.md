# Flutter 平台原生工程生成说明

**当前仓库不包含 `android/` `ios/` 目录**——它们由 `flutter create` 生成，包含大量平台文件（Gradle wrapper、Xcode 项目等），不适合手写。

## 生成步骤

在**已安装 Flutter SDK** 的机器上，进入本目录执行：

```powershell
cd apps\mobile
flutter create --platforms=android,ios --project-name=r_todo_mobile --org=dev.rtodo .
```

Flutter 会：
- 生成 `android/` 和 `ios/` 目录
- **保留** 已有的 `lib/`、`pubspec.yaml`、`test/` 等文件
- 装好插件所需的 gradle / pod 配置

生成后再执行：

```powershell
flutter pub get
```

## 小组件相关的额外配置

小组件不是 Flutter 层能搞定的，需要在生成后手动加：

### Android
- `android/app/src/main/kotlin/dev/rtodo/r_todo_mobile/` 下新建 `TodoWidgetProvider.kt`
- `android/app/src/main/res/xml/` 下加 `todo_widget_info.xml`
- `AndroidManifest.xml` 里注册 receiver

参考：<https://pub.dev/packages/home_widget#-android-setup>

### iOS
- Xcode 打开 `ios/Runner.xcworkspace` → File > New > Target > Widget Extension
- 命名如 `TodoWidget`
- 用 App Group 共享数据

参考：<https://pub.dev/packages/home_widget#-ios-setup>

以上两块**留到实现阶段做**，骨架阶段只保证 Dart 侧和 pubspec 就绪。
