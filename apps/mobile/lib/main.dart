import 'dart:io';
import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:timezone/data/latest.dart' as tzdata;

import 'src/rust/frb_generated.dart';
import 'state/task_store.dart';
import 'sync/sync_service.dart';
import 'ui/root_shell.dart';
import 'widget_manager.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  tzdata.initializeTimeZones();
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
  static const _reminderChannel = MethodChannel('r_todo/install');

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
      _startReminderScheduler();
      // 注册小组件交互回调（点击小组件跳转）
      await initWidget();
    });
  }

  void _startReminderScheduler() {
    Timer.periodic(const Duration(seconds: 30), (_) => _checkReminders());
    _checkReminders();
  }

  Future<void> _checkReminders() async {
    final tasks = ref.read(taskListProvider);
    final now = DateTime.now();
    for (final task in tasks) {
      if (task.isCompleted || task.remindAt == null) continue;
      final remindAt = DateTime.tryParse(task.remindAt!)?.toLocal();
      if (remindAt == null) continue;
      final due = remindAt.subtract(Duration(minutes: task.remindOffsetMin));
      if (now.isBefore(due) ||
          now.difference(due) > const Duration(minutes: 2)) {
        continue;
      }
      final prefs = await SharedPreferences.getInstance();
      final key = 'reminder.sent.${task.id}.${task.updatedAt}';
      if (prefs.getBool(key) == true) continue;
      try {
        final notifications = FlutterLocalNotificationsPlugin();
        const android = AndroidInitializationSettings('@mipmap/ic_launcher');
        await notifications.initialize(
          const InitializationSettings(android: android),
        );
        await notifications.show(
          task.id.hashCode,
          task.title,
          '任务提醒',
          const NotificationDetails(
            android: AndroidNotificationDetails(
              'rtools_reminders',
              '任务提醒',
              importance: Importance.high,
              priority: Priority.high,
            ),
          ),
        );
      } on MissingPluginException {
        // 桌面/测试环境没有通知插件。
      } catch (_) {
        try {
          await _reminderChannel.invokeMethod<void>('showReminder', {
            'title': task.title,
            'text': '任务提醒',
          });
        } catch (_) {
          // 通知失败不影响任务数据。
        }
      }
      await prefs.setBool(key, true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final brightness = MediaQuery.platformBrightnessOf(context);
    final theme = brightness == Brightness.dark
        ? FTheme.neutral.dark.touch
        : FTheme.neutral.light.touch;

    return MaterialApp(
      title: 'rtools',
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
