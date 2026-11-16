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
  final int revision;

  const TodoTask({
    required this.id,
    required this.title,
    required this.priority,
    required this.createdAt,
    required this.updatedAt,
    this.revision = 0,
    this.remindAt,
    this.remindOffsetMin = 15,
    this.repeatRule,
    this.sortOrder = 0,
    this.deleted = false,
    this.completedAt,
  });

  bool get isCompleted => completedAt != null;

  factory TodoTask.fromJson(Map<String, dynamic> json) => TodoTask(
        id: json['id'] as String,
        title: json['title'] as String,
        priority: priorityFromString(json['priority'] as String? ?? 'medium'),
        remindAt: _nullableString(json['remind_at']),
        remindOffsetMin: (json['remind_offset_min'] as num?)?.toInt() ?? 15,
        repeatRule: _nullableString(json['repeat_rule']),
        sortOrder: (json['sort_order'] as num?)?.toDouble() ?? 0,
        deleted: json['deleted_at'] != null,
        completedAt: _nullableString(json['completed_at']),
        createdAt: json['created_at'] as String,
        updatedAt: json['updated_at'] as String,
        revision: (json['revision'] as num?)?.toInt() ?? 0,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'title': title,
        'priority': priorityToString(priority),
        'remind_at': remindAt,
        'remind_offset_min': remindOffsetMin,
        'repeat_rule': repeatRule,
        'sort_order': sortOrder,
        'completed_at': completedAt,
        'deleted': deleted,
        'deleted_at': deleted ? updatedAt : null,
        'created_at': createdAt,
        'updated_at': updatedAt,
        'revision': revision,
      };

  static String? _nullableString(Object? value) {
    final text = value?.toString();
    return text == null || text.isEmpty ? null : text;
  }

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
    int? revision,
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
      revision: revision ?? this.revision,
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
        revision: r.revision,
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
        revision: Value(revision),
      );
}
