import 'dart:io';

import 'package:drift/drift.dart';
import 'package:drift/native.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

part 'database.g.dart';

class Tasks extends Table {
  TextColumn get id => text()();
  TextColumn get title => text().withLength(min: 1, max: 500)();
  TextColumn get priority => text()();
  TextColumn get remindAt => text().nullable()();
  IntColumn get remindOffsetMin => integer().withDefault(const Constant(15))();
  TextColumn get repeatRule => text().nullable()();
  RealColumn get sortOrder => real().withDefault(const Constant(0))();
  BoolColumn get deleted => boolean().withDefault(const Constant(false))();
  TextColumn get completedAt => text().nullable()();
  TextColumn get createdAt => text()();
  TextColumn get updatedAt => text()();

  @override
  Set<Column> get primaryKey => {id};
}

class PendingOps extends Table {
  IntColumn get id => integer().autoIncrement()();
  TextColumn get opType => text()();
  TextColumn get collection => text().withDefault(const Constant('tasks'))();
  TextColumn get recordId => text()();
  TextColumn get payload => text().nullable()();
  IntColumn get createdAt => integer()();
  IntColumn get attempts => integer().withDefault(const Constant(0))();
  TextColumn get lastError => text().nullable()();
}

@DriftDatabase(tables: [Tasks, PendingOps])
class AppDatabase extends _$AppDatabase {
  AppDatabase() : super(_openConnection());

  @override
  int get schemaVersion => 2;

  @override
  MigrationStrategy get migration {
    return MigrationStrategy(
      onCreate: (m) async {
        await m.createAll();
      },
      onUpgrade: (m, from, to) async {
        if (from == 1) {
          await m.addColumn(tasks, tasks.deleted);
          await m.createTable(pendingOps);
        }
      },
    );
  }

  Future<List<Task>> getAllTasks() =>
      (select(tasks)..where((t) => t.deleted.equals(false))).get();

  Future<void> upsertTask(TasksCompanion task) =>
      into(tasks).insertOnConflictUpdate(task);

  Future<void> softDeleteTaskById(String id) async {
    final now = DateTime.now().toUtc().toIso8601String();
    await (update(tasks)..where((t) => t.id.equals(id)))
        .write(const TasksCompanion(deleted: Value(true)));
    await (update(tasks)..where((t) => t.id.equals(id)))
        .write(TasksCompanion(updatedAt: Value(now)));
  }

  Future<void> deleteTaskById(String id) =>
      (delete(tasks)..where((t) => t.id.equals(id))).go();

  Future<void> enqueueOp(String opType, String recordId, {String? payload}) =>
      into(pendingOps).insert(PendingOpsCompanion.insert(
        opType: opType,
        recordId: recordId,
        payload: Value(payload),
        createdAt: DateTime.now().millisecondsSinceEpoch,
      ));

  Future<List<PendingOp>> listPendingOps() => select(pendingOps).get();
  Future<void> removePendingOp(int id) =>
      (delete(pendingOps)..where((t) => t.id.equals(id))).go();

  Future<void> markPendingOpError(int id, String error) async {
    await (update(pendingOps)..where((t) => t.id.equals(id))).write(
        PendingOpsCompanion(attempts: Value(1), lastError: Value(error)));
  }

  Future<List<Task>> completedBetween(String startIso, String endIso) =>
      (select(tasks)
            ..where((t) =>
                t.completedAt.isNotNull() &
                t.completedAt.isBiggerOrEqualValue(startIso) &
                t.completedAt.isSmallerThanValue(endIso)))
          .get();
}

LazyDatabase _openConnection() {
  return LazyDatabase(() async {
    final dir = await getApplicationDocumentsDirectory();
    final file = File(p.join(dir.path, 'r-todo.db'));
    return NativeDatabase.createInBackground(file);
  });
}