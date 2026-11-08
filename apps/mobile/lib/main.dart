import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';

import 'src/rust/frb_generated.dart';
import 'state/task_store.dart';
import 'sync/sync_service.dart';
import 'ui/root_shell.dart';
import 'widget_manager.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // 互传的 Rust 动态库只在 Android 上随包分发；初始化失败不阻塞待办主流程。
  if (!kIsWeb && Platform.isAndroid) {
    try {
      await RustLib.init();
    } catch (error) {
      debugPrint('互传 Rust 初始化失败: $error');
    }
  }
  runApp(const ProviderScope(child: RTodoApp()));
}

class RTodoApp extends ConsumerStatefulWidget {
  const RTodoApp({super.key});

  @override
  ConsumerState<RTodoApp> createState() => _RTodoAppState();
}

class _RTodoAppState extends ConsumerState<RTodoApp> {
  @override
  void initState() {
    super.initState();
    // 初始化同步服务并恢复登录（延后到首帧后，确保 db provider 就绪）
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      final db = ref.read(databaseProvider);
      SyncService.init(db);
      SyncService.instance.onRemoteChange =
          ref.read(taskListProvider.notifier).refreshFromDb;
      await SyncService.instance.restoreSession();
      await ref.read(taskListProvider.notifier).refreshFromDb();
      // 启动时主动刷新小组件数据（任务无变更时 widget 不会自行更新，
      // 否则应用升级/重装后小组件仍显示旧数据）
      await updateWidgetData(ref.read(taskListProvider));
      // 注册小组件交互回调（点击小组件跳转）
      await initWidget();
    });
  }

  @override
  Widget build(BuildContext context) {
    final brightness = MediaQuery.platformBrightnessOf(context);
    final theme = brightness == Brightness.dark
        ? FTheme.neutral.dark.touch
        : FTheme.neutral.light.touch;

    return MaterialApp(
      title: 'r-todo',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(brightness: brightness),
      builder: (context, child) => FTheme(
        data: theme,
        child: child!,
      ),
      home: const RootShell(),
    );
  }
}
