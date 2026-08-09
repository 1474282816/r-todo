// 桌面小组件数据管理
// 在每次本地数据变更后调用 updateWidgetData 刷新小组件。
import 'dart:async';
import 'dart:convert';

import 'package:home_widget/home_widget.dart';

import '../data/task.dart';

/// 保存当前未完成任务到 widget 存储并刷新小组件
Future<void> updateWidgetData(List<TodoTask> tasks) async {
  // 存全部未完成任务（不截断），Provider 按 widget 高度决定显示行数，
  // 右上角"剩余 X 项"才能反映真实未完成数
  final active = tasks.where((t) => !t.isCompleted).toList();

  final jsonTasks = active.map((t) => {
    'id': t.id,
    'title': t.title,
    'priority': priorityToString(t.priority),
  }).toList();

  await HomeWidget.saveWidgetData('tasks', jsonEncode(jsonTasks));
  // 更新所有尺寸变体（2x2/3x2/4x2/4x3），否则未更新的变体不刷新
  await Future.wait([
    HomeWidget.updateWidget(
      qualifiedAndroidName: 'dev.rtodo.r_todo_mobile.widget.TodoWidgetProvider',
    ),
    HomeWidget.updateWidget(
      qualifiedAndroidName: 'dev.rtodo.r_todo_mobile.widget.TodoWidget2x2Provider',
    ),
    HomeWidget.updateWidget(
      qualifiedAndroidName: 'dev.rtodo.r_todo_mobile.widget.TodoWidget4x2Provider',
    ),
    HomeWidget.updateWidget(
      qualifiedAndroidName: 'dev.rtodo.r_todo_mobile.widget.TodoWidget4x3Provider',
    ),
  ]);
}

/// 在 App 启动时调用，注册 widget 交互回调（可选）
Future<void> initWidget() async {
  // 注册点击交互回调（当用户点击 widget 上的任务时触发）
  await HomeWidget.registerInteractivityCallback((uri) async {
    // 如果 future 需要，可以在这里处理跳转
    // 实际跳转由 Kotlin 侧的 HomeWidgetLaunchIntent 处理
  });
}