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
  // 没有实际添加小组件时无需广播更新。home_widget 0.9.3 在部分 Android
  // 启动场景会对空 provider 抛空指针，跳过更新可避免无关的平台异常。
  final installed = await HomeWidget.getInstalledWidgets();
  if (installed.isEmpty) return;
  final installedClasses = installed
      .map((widget) => widget.androidClassName)
      .whereType<String>()
      .toSet();
  final providers = <String>[
    'dev.rtodo.r_todo_mobile.widget.TodoWidgetProvider',
    'dev.rtodo.r_todo_mobile.widget.TodoWidget2x2Provider',
    'dev.rtodo.r_todo_mobile.widget.TodoWidget4x2Provider',
    'dev.rtodo.r_todo_mobile.widget.TodoWidget4x3Provider',
  ];
  // 更新所有尺寸变体（2x2/3x2/4x2/4x3），否则未更新的变体不刷新
  await Future.wait(providers.where((provider) {
    final shortName = provider.split('.').last;
    return installedClasses.any(
      (name) => name == shortName || name == '.$shortName',
    );
  }).map((provider) => HomeWidget.updateWidget(
        qualifiedAndroidName: provider,
      )));
}
