import 'package:flutter_test/flutter_test.dart';
import 'package:r_todo_mobile/data/task.dart';

void main() {
  test('TodoTask 保留服务端 revision，供后续增量更新使用', () {
    final task = TodoTask.fromJson({
      'id': 'task-1',
      'title': '同步测试',
      'priority': 'high',
      'remind_at': null,
      'remind_offset_min': 15,
      'repeat_rule': null,
      'sort_order': 1000,
      'completed_at': null,
      'deleted_at': null,
      'created_at': '2026-11-17T00:00:00Z',
      'updated_at': '2026-11-17T00:01:00Z',
      'revision': 7,
    });

    expect(task.revision, 7);
    expect(task.toJson()['revision'], 7);
  });

  test('TodoTask 修改字段时不会丢失 revision', () {
    final task = TodoTask.fromJson({
      'id': 'task-2',
      'title': '修改前',
      'priority': 'medium',
      'sort_order': 0,
      'created_at': '2026-11-17T00:00:00Z',
      'updated_at': '2026-11-17T00:01:00Z',
      'revision': 3,
    }).copyWith(title: '修改后');

    expect(task.title, '修改后');
    expect(task.revision, 3);
  });
}
