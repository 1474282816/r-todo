import 'dart:io';

import 'package:drift/drift.dart';
import 'package:drift/native.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

import '../features/chat/chat_message.dart';
import '../features/chat/chat_store.dart';

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
  IntColumn get revision => integer().withDefault(const Constant(0))();

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
class AppDatabase extends _$AppDatabase implements ChatStore {
  AppDatabase() : super(_openConnection());

  @override
  int get schemaVersion => 4;

  @override
  MigrationStrategy get migration {
    return MigrationStrategy(
      onCreate: (m) async {
        await m.createAll();
        await createChatTables();
      },
      onUpgrade: (m, from, to) async {
        // 待办的迁移逻辑一行不动；v3 只新增会话表（Q44）。
        if (from == 1) {
          await m.addColumn(tasks, tasks.deleted);
          await m.createTable(pendingOps);
        }
        if (from < 3) {
          await createChatTables();
        }
        if (from < 4) {
          await m.addColumn(tasks, tasks.revision);
        }
      },
    );
  }

  /// 会话表用 DDL 直接建，不新增 Drift Table 类，因此无需重新生成 database.g.dart，
  /// 也完全不触碰 tasks / pending_ops。
  @override
  Future<void> createChatTables() async {
    for (final sql in chatTableDdl) {
      await customStatement(sql);
    }
  }

  Future<List<Task>> getAllTasks() =>
      (select(tasks)
            ..where((t) => t.deleted.equals(false))
            ..orderBy([
              (t) => OrderingTerm(
                    expression: t.completedAt.isNull(),
                    mode: OrderingMode.desc,
                  ),
              (t) => OrderingTerm(expression: t.sortOrder),
            ]))
          .get();

  Future<List<Task>> getAllTasksIncludingDeleted() => select(tasks).get();

  Future<void> upsertTask(TasksCompanion task) =>
      into(tasks).insertOnConflictUpdate(task);

  Future<void> softDeleteTaskById(String id) async {
    final now = DateTime.now().toUtc().toIso8601String();
    await (update(tasks)..where((t) => t.id.equals(id))).write(TasksCompanion(
      deleted: const Value(true),
      updatedAt: Value(now),
    ));
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
        PendingOpsCompanion(attempts: const Value(1), lastError: Value(error)));
  }

  Future<List<Task>> completedBetween(String startIso, String endIso) =>
      (select(tasks)
            ..where((t) =>
                t.completedAt.isNotNull() &
                t.completedAt.isBiggerOrEqualValue(startIso) &
                t.completedAt.isSmallerThanValue(endIso)))
          .get();

  // ─────────────── 会话式互传（新增表，不碰待办） ───────────────

  Future<List<Map<String, dynamic>>> chatQuery(
    String sql,
    List<Variable> args,
  ) async {
    final rows = await customSelect(sql, variables: args).get();
    return rows
        .map((row) => Map<String, dynamic>.from(row.data))
        .toList(growable: false);
  }

  Future<void> chatExec(String sql, List<Variable> args) =>
      customStatement(sql, args);

  @override
  Future<List<Map<String, dynamic>>> listChatConversations() => chatQuery(
        'SELECT * FROM conversations ORDER BY pinned DESC, last_message_at DESC',
        <Variable>[],
      );

  @override
  Future<void> upsertChatConversation(
    String id,
    String alias,
    int at, {
    int unreadDelta = 0,
  }) =>
      chatExec(
        'INSERT INTO conversations (id, alias, last_message_at, unread) VALUES (?, ?, ?, ?) '
        'ON CONFLICT(id) DO UPDATE SET alias = excluded.alias, '
        'last_message_at = MAX(last_message_at, excluded.last_message_at), '
        'unread = unread + excluded.unread',
        <Variable>[
          Variable<String>(id),
          Variable<String>(alias),
          Variable<int>(at),
          Variable<int>(unreadDelta),
        ],
      );

  @override
  Future<void> markChatRead(String id) => chatExec(
        'UPDATE conversations SET unread = 0 WHERE id = ?',
        <Variable>[Variable<String>(id)],
      );

  @override
  Future<List<Map<String, dynamic>>> listChatMessages(String conversationId) =>
      chatQuery(
        'SELECT * FROM messages WHERE conversation_id = ? ORDER BY created_at ASC LIMIT 300',
        <Variable>[Variable<String>(conversationId)],
      );

  @override
  Future<void> insertChatMessage(ChatMessage message, String direction) =>
      chatExec(
        'INSERT OR REPLACE INTO messages '
        '(id, conversation_id, sender, sender_alias, kind, body, file_path, '
        ' file_size, preview, forwarded_from, created_at, status, direction, progress) '
        'VALUES (?, ?, ?, ?, ?, ?, NULLIF(?, ""), ?, NULLIF(?, ""), NULLIF(?, ""), ?, ?, ?, ?)',
        <Variable>[
          Variable<String>(message.id),
          Variable<String>(message.conversationId),
          Variable<String>(message.sender),
          Variable<String>(message.senderAlias),
          Variable<String>(chatKindName(message.kind)),
          Variable<String>(message.body),
          Variable<String>(message.filePath ?? ''),
          Variable<int>(message.fileSize ?? 0),
          Variable<String>(message.preview ?? ''),
          Variable<String>(message.forwardedFrom ?? ''),
          Variable<int>(message.createdAt),
          Variable<String>(chatStatusName(message.status)),
          Variable<String>(direction),
          const Variable<double>(0),
        ],
      );

  @override
  Future<void> patchChatMessage(
    String id, {
    String? status,
    String? filePath,
    double? progress,
    String? forwardedFrom,
  }) async {
    final sets = <String>[];
    final args = <Variable>[];
    void add(String column, Variable value) {
      sets.add('$column = ?');
      args.add(value);
    }

    if (status != null) add('status', Variable<String>(status));
    if (filePath != null) add('file_path', Variable<String>(filePath));
    if (progress != null) add('progress', Variable<double>(progress));
    if (forwardedFrom != null) {
      add('forwarded_from', Variable<String>(forwardedFrom));
    }
    if (sets.isEmpty) return;
    args.add(Variable<String>(id));
    await chatExec(
      'UPDATE messages SET ${sets.join(", ")} WHERE id = ?',
      args,
    );
  }

  @override
  Future<void> deleteChatMessage(String id) => chatExec(
        'DELETE FROM messages WHERE id = ?',
        <Variable>[Variable<String>(id)],
      );

  @override
  Future<void> clearChatConversation(String id) async {
    await chatExec(
      'DELETE FROM messages WHERE conversation_id = ?',
      <Variable>[Variable<String>(id)],
    );
    await chatExec(
      'DELETE FROM conversations WHERE id = ?',
      <Variable>[Variable<String>(id)],
    );
  }

  @override
  Future<bool> isChatTrusted(String fingerprint) async {
    final rows = await chatQuery(
      'SELECT fingerprint FROM trusted_peers WHERE fingerprint = ?',
      <Variable>[Variable<String>(fingerprint.toUpperCase())],
    );
    return rows.isNotEmpty;
  }

  @override
  Future<void> trustChatPeer(String fingerprint) => chatExec(
        'INSERT OR IGNORE INTO trusted_peers (fingerprint, trusted_at) VALUES (?, ?)',
        <Variable>[
          Variable<String>(fingerprint.toUpperCase()),
          Variable<int>(DateTime.now().millisecondsSinceEpoch),
        ],
      );

  @override
  Future<void> revokeChatTrust(String fingerprint) => chatExec(
        'DELETE FROM trusted_peers WHERE fingerprint = ?',
        <Variable>[Variable<String>(fingerprint.toUpperCase())],
      );

  @override
  Future<void> enqueueChatOutbox(String target, String payload, String id) =>
      chatExec(
        'INSERT OR REPLACE INTO outbox (id, payload, target, created_at) VALUES (?, ?, ?, ?)',
        <Variable>[
          Variable<String>(id),
          Variable<String>(payload),
          Variable<String>(target.toUpperCase()),
          Variable<int>(DateTime.now().millisecondsSinceEpoch),
        ],
      );

  @override
  Future<List<Map<String, dynamic>>> listChatOutbox() => chatQuery(
        'SELECT * FROM outbox ORDER BY created_at ASC',
        <Variable>[],
      );

  @override
  Future<void> removeChatOutbox(String id) => chatExec(
        'DELETE FROM outbox WHERE id = ?',
        <Variable>[Variable<String>(id)],
      );

  @override
  Future<void> bumpChatOutbox(String id, String error) => chatExec(
        'UPDATE outbox SET attempts = attempts + 1, last_error = ? WHERE id = ?',
        <Variable>[Variable<String>(error), Variable<String>(id)],
      );
}

LazyDatabase _openConnection() {
  return LazyDatabase(() async {
    final dir = await getApplicationDocumentsDirectory();
    final file = File(p.join(dir.path, 'r-todo.db'));
    return NativeDatabase.createInBackground(file);
  });
}

/// 与桌面 `db.rs` migration v4 保持一致。只新增表，tasks / pending_ops 一行不动。
const List<String> chatTableDdl = <String>[
  'CREATE TABLE IF NOT EXISTS conversations ('
      'id TEXT PRIMARY KEY NOT NULL, alias TEXT NOT NULL, '
      'last_message_at INTEGER NOT NULL DEFAULT 0, '
      'unread INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0)',
  'CREATE INDEX IF NOT EXISTS idx_conversations_recent ON conversations(last_message_at DESC)',
  'CREATE TABLE IF NOT EXISTS messages ('
      'id TEXT PRIMARY KEY NOT NULL, conversation_id TEXT NOT NULL, '
      'sender TEXT NOT NULL, sender_alias TEXT NOT NULL, kind TEXT NOT NULL, '
      'body TEXT NOT NULL, file_path TEXT, file_size INTEGER, preview TEXT, '
      'forwarded_from TEXT, created_at INTEGER NOT NULL, status TEXT NOT NULL, '
      'direction TEXT NOT NULL DEFAULT "receive", progress REAL NOT NULL DEFAULT 0)',
  'CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, created_at)',
  'CREATE TABLE IF NOT EXISTS outbox ('
      'id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL, target TEXT NOT NULL, '
      'attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT, created_at INTEGER NOT NULL)',
  'CREATE TABLE IF NOT EXISTS trusted_peers ('
      'fingerprint TEXT PRIMARY KEY NOT NULL, trusted_at INTEGER NOT NULL)',
];
