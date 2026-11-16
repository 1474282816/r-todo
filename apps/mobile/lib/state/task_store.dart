import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:rrule/rrule.dart';

import '../data/database.dart';
import '../data/task.dart';
import '../sync/sync_service.dart';
import '../widget_manager.dart';

// 全局单例数据库
final databaseProvider = Provider<AppDatabase>((ref) {
  final db = AppDatabase();
  ref.onDispose(db.close);
  return db;
});

// 任务列表状态
class TaskListNotifier extends StateNotifier<List<TodoTask>> {
  TaskListNotifier(this._db) : super([]) {
    _load();
  }

  final AppDatabase _db;

  Future<void> _load() async {
    final rows = await _db.getAllTasks();
    state = rows.map(TodoTask.fromRow).toList();
    _updateWidget();
  }

  Future<void> reload() => _load();
  Future<void> refreshFromDb() => _load();

  Future<void> _updateWidget() {
    return updateWidgetData(state).catchError((_) {});
  }

  String _now() => DateTime.now().toUtc().toIso8601String();

  String _newId() {
    const chars = 'abcdefghijklmnopqrstuvwxyz0123456789';
    var seed = DateTime.now().microsecondsSinceEpoch;
    final buf = StringBuffer();
    for (var i = 0; i < 15; i++) {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      buf.write(chars[seed % chars.length]);
    }
    return buf.toString();
  }

  Future<void> add({
    required String title,
    Priority priority = Priority.low,
    String? remindAt,
    int remindOffsetMin = 15,
    String? repeatRule,
  }) async {
    final minOrder = state
        .where((t) => !t.isCompleted)
        .fold<double>(0, (m, t) => t.sortOrder < m ? t.sortOrder : m);
    final now = _now();
    final task = TodoTask(
      id: _newId(),
      title: title.trim(),
      priority: priority,
      remindAt: remindAt,
      remindOffsetMin: remindOffsetMin,
      repeatRule: repeatRule,
      sortOrder: minOrder - 1000,
      createdAt: now,
      updatedAt: now,
    );
    await _db.upsertTask(task.toCompanion());
    state = [...state, task];
    SyncService.maybe?.pushTask(task);
    _updateWidget();
  }

  Future<void> update(String id, TodoTask updated) async {
    final withTime = updated.copyWith(updatedAt: _now());
    await _db.upsertTask(withTime.toCompanion());
    state = [
      for (final t in state) t.id == id ? withTime : t,
    ];
    SyncService.maybe?.pushTask(withTime);
  }

  Future<void> remove(String id) async {
    await _db.softDeleteTaskById(id);
    state = state.where((t) => t.id != id).toList();
    SyncService.maybe?.deleteRemote(id);
  }

  Future<void> toggleComplete(String id) async {
    final idx = state.indexWhere((t) => t.id == id);
    if (idx < 0) return;
    final t = state[idx];
    final completing = !t.isCompleted;
    final updated = t.copyWith(
      completedAt: () => completing ? _now() : null,
      updatedAt: _now(),
    );
    await _db.upsertTask(updated.toCompanion());
    state = [
      for (final x in state) x.id == id ? updated : x,
    ];
    SyncService.maybe?.pushTask(updated);
    if (completing && t.repeatRule != null) {
      final next = nextRepeatInstance(t);
      if (next != null) {
        await add(
          title: next.title,
          priority: next.priority,
          remindAt: next.remindAt,
          remindOffsetMin: next.remindOffsetMin,
          repeatRule: next.repeatRule,
        );
      }
    }
  }

  TodoTask? nextRepeatInstance(TodoTask task) {
    final ruleText = task.repeatRule;
    if (ruleText == null) return null;
    try {
      final base = DateTime.tryParse(task.remindAt ?? '') ??
          DateTime.now().toUtc();
      final normalized = ruleText.contains('RRULE:')
          ? ruleText
          : 'RRULE:$ruleText';
      final next = RecurrenceRule.fromString(normalized)
          .getInstances(start: base.toUtc(), after: base.toUtc())
          .firstOrNull;
      if (next == null) return null;
      return task.copyWith(
        remindAt: () => task.remindAt == null ? null : next.toIso8601String(),
        completedAt: () => null,
        updatedAt: DateTime.now().toUtc().toIso8601String(),
      );
    } catch (_) {
      return null;
    }
  }

  Future<void> reorderActive(List<String> orderedIds) async {
    final now = _now();
    final updates = <TodoTask>[];
    for (var i = 0; i < orderedIds.length; i++) {
      final id = orderedIds[i];
      final t = state.firstWhere((x) => x.id == id);
      updates.add(t.copyWith(sortOrder: (i + 1) * 1000.0, updatedAt: now));
    }
    // 先同步更新内存 state（乐观），避免松手后闪回旧顺序
    state = [
      for (final t in state)
        updates.firstWhere((u) => u.id == t.id, orElse: () => t),
    ];
    // 再异步落库 + 推送
    for (final u in updates) {
      await _db.upsertTask(u.toCompanion());
    }
    for (final u in updates) {
      SyncService.maybe?.pushTask(u);
    }
  }
}

final taskListProvider =
    StateNotifierProvider<TaskListNotifier, List<TodoTask>>((ref) {
  final db = ref.watch(databaseProvider);
  return TaskListNotifier(db);
});

// 排序 + 过滤：未完成在上（按 sortOrder），已完成在下（按完成时间倒序），
// 已完成任务次日 0:00 自动隐藏
List<TodoTask> sortAndFilter(List<TodoTask> tasks) {
  final now = DateTime.now();
  final midnight = DateTime(now.year, now.month, now.day);

  bool hiddenCompleted(TodoTask t) {
    if (t.completedAt == null) return false;
    final c = DateTime.tryParse(t.completedAt!)?.toLocal();
    return c != null && c.isBefore(midnight);
  }

  final visible = tasks.where((t) => !hiddenCompleted(t)).toList();

  visible.sort((a, b) {
    final ac = a.isCompleted ? 1 : 0;
    final bc = b.isCompleted ? 1 : 0;
    if (ac != bc) return ac - bc;
    if (!a.isCompleted) {
      if (a.sortOrder != b.sortOrder) return a.sortOrder.compareTo(b.sortOrder);
    } else {
      final cmp = (b.completedAt ?? '').compareTo(a.completedAt ?? '');
      if (cmp != 0) return cmp;
    }
    return a.createdAt.compareTo(b.createdAt);
  });
  return visible;
}
