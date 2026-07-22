import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';

import 'state/task_store.dart';
import 'sync/sync_service.dart';
import 'ui/home_page.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
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
      home: const HomePage(),
    );
  }
}
