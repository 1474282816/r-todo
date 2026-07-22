/// Task 域模型（与 packages/schema/task.schema.json 一致）
///
/// 这是 UI/业务层使用的纯 Dart 模型，与 Drift 生成的行类型 (Task) 通过
/// mapper 转换。为避免命名冲突，Drift 生成的类叫 `Task`，这里域模型叫 `TodoTask`。
library;

import 'package:drift/drift.dart' show Value;

import '../data/database.dart';

enum Priority { high, medium, low }

Priority priorityFromString(String s) {
  switch (s) {
    case 'high':
      return Priority.high;
    case 'medium':
      return Priority.medium;
    default:
      return Priority.low;
  }
}

String priorityToString(Priority p) => p.name;

class TodoTask {
  final String id;
  final String title;
  final Priority priority;
  final String? remindAt;
  final int remindOffsetMin;
  final String? repeatRule;
  final double sortOrder;
  final bool deleted;
  final String? completedAt;
  final String createdAt;
  final String updatedAt;

  const TodoTask({
    required this.id,
    required this.title,
    required this.priority,
    required this.createdAt,
    required this.updatedAt,
    this.remindAt,
    this.remindOffsetMin = 15,
    this.repeatRule,
    this.sortOrder = 0,
    this.deleted = false,
    this.completedAt,
  });

  bool get isCompleted => completedAt != null;

  TodoTask copyWith({
    String? title,
    Priority? priority,
    String? Function()? remindAt,
    int? remindOffsetMin,
    String? Function()? repeatRule,
    double? sortOrder,
    bool? deleted,
    String? Function()? completedAt,
    String? updatedAt,
  }) {
    return TodoTask(
      id: id,
      title: title ?? this.title,
      priority: priority ?? this.priority,
      remindAt: remindAt != null ? remindAt() : this.remindAt,
      remindOffsetMin: remindOffsetMin ?? this.remindOffsetMin,
      repeatRule: repeatRule != null ? repeatRule() : this.repeatRule,
      sortOrder: sortOrder ?? this.sortOrder,
      deleted: deleted ?? this.deleted,
      completedAt: completedAt != null ? completedAt() : this.completedAt,
      createdAt: createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
    );
  }

  /// Drift 行 → 域模型
  factory TodoTask.fromRow(Task r) => TodoTask(
        id: r.id,
        title: r.title,
        priority: priorityFromString(r.priority),
        remindAt: r.remindAt,
        remindOffsetMin: r.remindOffsetMin,
        repeatRule: r.repeatRule,
        sortOrder: r.sortOrder,
        deleted: r.deleted,
        completedAt: r.completedAt,
        createdAt: r.createdAt,
        updatedAt: r.updatedAt,
      );

  /// 域模型 → Drift 写入 companion
  TasksCompanion toCompanion() => TasksCompanion.insert(
        id: id,
        title: title,
        priority: priorityToString(priority),
        remindAt: Value(remindAt),
        remindOffsetMin: Value(remindOffsetMin),
        repeatRule: Value(repeatRule),
        sortOrder: Value(sortOrder),
        deleted: Value(deleted),
        completedAt: Value(completedAt),
        createdAt: createdAt,
        updatedAt: updatedAt,
      );
}
