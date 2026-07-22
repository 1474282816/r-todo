import 'dart:async';

import 'package:pocketbase/pocketbase.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'dart:convert';

import '../data/database.dart';
import '../data/task.dart';

class SyncService {
  SyncService._(this._db);

  static late SyncService instance;
  static SyncService? _maybe;

  static void init(AppDatabase db) {
    instance = SyncService._(db);
    _maybe = instance;
  }

  static SyncService? get maybe => _maybe;

  final AppDatabase _db;
  PocketBase? _pb;
  UnsubscribeFunc? _unsub;

  static const _kUrl = 'r-todo/sync/url';
  static const _kAuth = 'r-todo/sync/auth';

  void Function()? onRemoteChange;

  bool connected = false;
  bool syncing = false;
  bool flushing = false;
  String? username;
  String? serverUrl;
  String? lastSyncAt;
  String? error;

  final _statusController = StreamController<void>.broadcast();
  Stream<void> get statusStream => _statusController.stream;
  void _notify() => _statusController.add(null);

  bool get isLoggedIn => _pb?.authStore.isValid ?? false;

  Future<void> restoreSession() async {
    final prefs = await SharedPreferences.getInstance();
    final url = prefs.getString(_kUrl);
    final token = prefs.getString(_kAuth);
    if (url == null || token == null) return;
    try {
      _pb = PocketBase(url);
      _pb!.authStore.save(token, null);
      if (!_pb!.authStore.isValid) { _pb = null; return; }
      try { await _pb!.collection('users').authRefresh(); } catch (_) {}
      await _persist();
      connected = true;
      username = _currentUsername();
      serverUrl = url;
      _notify();
      unawaited(fullSync());
      unawaited(flushPendingOps());
      _subscribeRealtime();
    } catch (e) {
      _pb = null;
      error = e.toString();
      _notify();
    }
  }

  String? _currentUsername() {
    final rec = _pb?.authStore.record;
    return rec?.getStringValue('username').isNotEmpty == true
        ? rec!.getStringValue('username')
        : rec?.getStringValue('email');
  }

  Future<void> _persist() async {
    if (_pb == null) return;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_kUrl, _pb!.baseURL);
    await prefs.setString(_kAuth, _pb!.authStore.token);
  }

  Future<void> login(String url, String identity, String password) async {
    syncing = true;
    error = null;
    _notify();
    try {
      final normalized = url.replaceAll(RegExp(r'/$'), '');
      _pb = PocketBase(normalized);
      await _pb!.collection('users').authWithPassword(identity, password);
      await _persist();
      connected = true;
      username = _currentUsername();
      serverUrl = normalized;
      _notify();
      await fullSync();
      await flushPendingOps();
      _subscribeRealtime();
    } catch (e) {
      _pb = null;
      connected = false;
      error = e.toString();
      rethrow;
    } finally {
      syncing = false;
      _notify();
    }
  }

  Future<void> logout() async {
    _unsub?.call();
    _unsub = null;
    _pb?.authStore.clear();
    _pb = null;
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_kUrl);
    await prefs.remove(_kAuth);
    connected = false;
    username = null;
    serverUrl = null;
    lastSyncAt = null;
    error = null;
    _notify();
  }

  Map<String, dynamic> _toRemote(TodoTask t) {
    final rec = _pb!.authStore.record!;
    return {
      'id': t.id,
      'user': rec.id,
      'title': t.title,
      'priority': priorityToString(t.priority),
      'remind_at': t.remindAt ?? '',
      'remind_offset_min': t.remindOffsetMin,
      'repeat_rule': t.repeatRule ?? '',
      'sort_order': t.sortOrder,
      'deleted': t.deleted,
      'completed_at': t.completedAt ?? '',
    };
  }

  TodoTask _fromRemote(RecordModel r) {
    String? nn(String key) {
      final v = r.getStringValue(key);
      return v.isEmpty ? null : v;
    }
    return TodoTask(
      id: r.id,
      title: r.getStringValue('title'),
      priority: priorityFromString(r.getStringValue('priority')),
      remindAt: nn('remind_at'),
      remindOffsetMin: r.getIntValue('remind_offset_min'),
      repeatRule: nn('repeat_rule'),
      sortOrder: r.getDoubleValue('sort_order'),
      deleted: r.getBoolValue('deleted'),
      completedAt: nn('completed_at'),
      createdAt: r.getStringValue('created'),
      updatedAt: r.getStringValue('updated'),
    );
  }

  Future<void> fullSync() async {
    if (_pb == null || !_pb!.authStore.isValid) return;
    syncing = true;
    error = null;
    _notify();
    try {
      final localRows = await _db.getAllTasks();
      final local = localRows.map(TodoTask.fromRow).toList();
      final remote = await _pb!.collection('tasks').getFullList(batch: 500);
      final remoteMap = {for (final r in remote) r.id: r};
      final localMap = {for (final t in local) t.id: t};
      for (final t in local) {
        if (!remoteMap.containsKey(t.id)) {
          await _pb!.collection('tasks').create(body: _toRemote(t));
        }
      }
      for (final r in remote) {
        if (!localMap.containsKey(r.id)) {
          await _db.upsertTask(_fromRemote(r).toCompanion());
        }
      }
      for (final t in local) {
        final r = remoteMap[t.id];
        if (r == null) continue;
        final lt = DateTime.parse(t.updatedAt).millisecondsSinceEpoch;
        final rt = DateTime.parse(r.getStringValue('updated')).millisecondsSinceEpoch;
        if (lt > rt) {
          await _pb!.collection('tasks').update(t.id, body: _toRemote(t));
        } else if (rt > lt) {
          await _db.upsertTask(_fromRemote(r).toCompanion());
        }
      }
      lastSyncAt = DateTime.now().toIso8601String();
      error = null;
      onRemoteChange?.call();
    } catch (e) {
      error = e.toString();
      rethrow;
    } finally {
      syncing = false;
      _notify();
    }
  }

  Future<void> flushPendingOps() async {
    if (_pb == null || !_pb!.authStore.isValid) return;
    flushing = true;
    _notify();
    try {
      final ops = await _db.listPendingOps();
      if (ops.isEmpty) return;
      for (final op in ops) {
        try {
          if (op.opType == 'create') {
            await _pb!.collection('tasks').create(body: jsonDecode(op.payload ?? '{}'));
          } else if (op.opType == 'update') {
            await _pb!.collection('tasks').update(op.recordId, body: jsonDecode(op.payload ?? '{}'));
          } else if (op.opType == 'delete') {
            await _pb!.collection('tasks').update(op.recordId, body: {'deleted': true});
          }
          await _db.removePendingOp(op.id);
        } on ClientException catch (e) {
          if (e.statusCode == 0) break;
          if (e.statusCode == 400 || e.statusCode == 404) {
            await _db.markPendingOpError(op.id, e.toString());
            continue;
          }
          break;
        }
      }
    } catch (_) {
    } finally {
      flushing = false;
      _notify();
    }
  }

  Future<void> pushTask(TodoTask t) async {
    if (_pb == null || !_pb!.authStore.isValid) return;
    try {
      try {
        await _pb!.collection('tasks').update(t.id, body: _toRemote(t));
      } on ClientException catch (e) {
        if (e.statusCode == 404) {
          await _pb!.collection('tasks').create(body: _toRemote(t));
        } else {
          rethrow;
        }
      }
      lastSyncAt = DateTime.now().toIso8601String();
      _notify();
    } catch (e) {
      error = e.toString();
      _notify();
      await _db.enqueueOp('update', t.id, payload: jsonEncode(_toRemote(t)));
    }
  }

  Future<void> deleteRemote(String id) async {
    if (_pb == null || !_pb!.authStore.isValid) return;
    try {
      await _pb!.collection('tasks').update(id, body: {'deleted': true});
    } on ClientException catch (e) {
      if (e.statusCode == 404) {
        await _db.enqueueOp('delete', id);
        return;
      }
      await _db.enqueueOp('delete', id);
      error = e.toString();
      _notify();
    }
  }

  void _subscribeRealtime() {
    if (_pb == null) return;
    _unsub?.call();
    // PB_CONNECT: 每次连接/重连触发全量同步
_pb!.realtime.subscribe('PB_CONNECT', (e) async {
      await fullSync().catchError((_) {});
      await flushPendingOps().catchError((_) {});
    }).then((_) {}).catchError((_) {});
    _pb!.collection('tasks').subscribe('*', (e) async {
      try {
        if (e.action == 'delete') {
          await _db.softDeleteTaskById(e.record!.id);
        } else if (e.record != null) {
          final t = _fromRemote(e.record!);
          if (t.deleted) {
            final existing = (await _db.getAllTasks()).where((x) => x.id == t.id).toList();
            if (existing.isNotEmpty) {
              final localUpdated = DateTime.parse(existing.first.updatedAt).millisecondsSinceEpoch;
              final remoteUpdated = DateTime.parse(t.updatedAt).millisecondsSinceEpoch;
              if (remoteUpdated > localUpdated) {
                await _db.softDeleteTaskById(t.id);
              }
            } else {
              await _db.upsertTask(t.toCompanion());
            }
          } else {
            final existing = (await _db.getAllTasks()).where((x) => x.id == t.id).toList();
            if (existing.isEmpty) {
              await _db.upsertTask(t.toCompanion());
            } else {
              final localUpdated = DateTime.parse(existing.first.updatedAt).millisecondsSinceEpoch;
              final remoteUpdated = DateTime.parse(t.updatedAt).millisecondsSinceEpoch;
              if (remoteUpdated > localUpdated) {
                await _db.upsertTask(t.toCompanion());
              }
            }
          }
        }
        onRemoteChange?.call();
      } catch (_) {}
    }).then((unsub) {
      _unsub = unsub;
    }).catchError((Object _) {});
  }
}