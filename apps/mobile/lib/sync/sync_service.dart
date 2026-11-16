import 'dart:async';
import 'dart:convert';

import 'package:shared_preferences/shared_preferences.dart';

import '../data/database.dart';
import '../data/task.dart';
import 'sync_api.dart';

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
  SyncApi? _api;
  Timer? _timer;
  Timer? _realtimeTimer;

  static const _kUrl = 'r-tools/sync/url';
  static const _kToken = 'r-tools/sync/token';
  static const _kDeviceId = 'r-tools/sync/device-id';
  static const _kDeviceName = 'r-tools/sync/device-name';
  static const _kCursor = 'r-tools/sync/cursor';
  static const _kLastSync = 'r-tools/sync/last-sync';

  void Function()? onRemoteChange;

  bool connected = false;
  bool syncing = false;
  bool flushing = false;
  String? deviceName;
  String? serverUrl;
  String? lastSyncAt;
  String? error;
  int cursor = 0;
  int pending = 0;

  final _statusController = StreamController<void>.broadcast();
  Stream<void> get statusStream => _statusController.stream;
  void _notify() => _statusController.add(null);

  bool get isLoggedIn => _api?.token != null;

  Future<void> restoreSession() async {
    final prefs = await SharedPreferences.getInstance();
    final url = prefs.getString(_kUrl);
    final token = prefs.getString(_kToken);
    final deviceId = prefs.getString(_kDeviceId);
    final name = prefs.getString(_kDeviceName);
    cursor = prefs.getInt(_kCursor) ?? 0;
    lastSyncAt = prefs.getString(_kLastSync);
    pending = (await _db.listPendingOps()).length;
    if (url == null || token == null || deviceId == null) {
      _notify();
      return;
    }
    serverUrl = url;
    deviceName = name;
    _api = SyncApi(baseUrl: url, token: token);
    connected = true;
    _notify();
    _startRealtimePolling();
    unawaited(syncNow());
  }

  Future<void> pair({
    required String url,
    required String code,
    required String name,
  }) async {
    syncing = true;
    error = null;
    _notify();
    try {
      final normalized = url.replaceAll(RegExp(r'/$'), '');
      final api = SyncApi(baseUrl: normalized);
      final response = await api.pair(code, name);
      final token = response['token'] as String;
      final deviceId = response['device_id'] as String;
      _api = SyncApi(baseUrl: normalized, token: token);
      serverUrl = normalized;
      deviceName = name;
      connected = true;
      final prefs = await SharedPreferences.getInstance();
      await prefs.setString(_kUrl, normalized);
      await prefs.setString(_kToken, token);
      await prefs.setString(_kDeviceId, deviceId);
      await prefs.setString(_kDeviceName, name);
      _startRealtimePolling();
      await syncNow();
    } catch (e) {
      connected = false;
      error = e.toString();
      rethrow;
    } finally {
      syncing = false;
      _notify();
    }
  }

  Future<void> logout() async {
    _timer?.cancel();
    _realtimeTimer?.cancel();
    _timer = null;
    _realtimeTimer = null;
    _api = null;
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_kUrl);
    await prefs.remove(_kToken);
    await prefs.remove(_kDeviceId);
    await prefs.remove(_kDeviceName);
    connected = false;
    serverUrl = null;
    deviceName = null;
    error = null;
    _notify();
  }

  Future<void> syncNow() async {
    if (_api == null) return;
    if (syncing) return;
    syncing = true;
    error = null;
    _notify();
    try {
      await flushPendingOps();
      await _pullChanges();
      connected = true;
      error = null;
    } catch (e) {
      connected = false;
      error = e.toString();
      if (e is SyncApiException && e.tokenInvalid) {
        _timer?.cancel();
        _realtimeTimer?.cancel();
      }
      rethrow;
    } finally {
      syncing = false;
      pending = (await _db.listPendingOps()).length;
      _notify();
    }
  }

  Future<void> flushPendingOps() async {
    final api = _api;
    if (api == null) return;
    final ops = await _db.listPendingOps();
    if (ops.isEmpty) {
      pending = 0;
      return;
    }
    final prefs = await SharedPreferences.getInstance();
    final deviceId = prefs.getString(_kDeviceId);
    if (deviceId == null) return;
    flushing = true;
    _notify();
    try {
      for (final op in ops) {
        final payload = jsonDecode(op.payload ?? '{}') as Map<String, dynamic>;
        var baseRevision = (payload['revision'] as num?)?.toInt() ?? 0;
        var operation = _buildOperation(
          operationId: 'mobile-${op.id}-${op.createdAt}',
          taskId: op.recordId,
          baseRevision: baseRevision,
          payload: payload,
        );
        var result = await _submitOperation(api, deviceId, operation);

        // 本地新建任务或旧数据库中的任务 revision 为 0。若服务端已经
        // 找不到该任务，说明本地元数据落后，使用新的操作 ID 重试创建。
        if (!result.accepted && result.error == 'TASK_NOT_FOUND') {
          baseRevision = 0;
          operation = _buildOperation(
            operationId:
                'mobile-create-${op.id}-${DateTime.now().microsecondsSinceEpoch}',
            taskId: op.recordId,
            baseRevision: baseRevision,
            payload: payload,
          );
          result = await _submitOperation(api, deviceId, operation);
        }

        if (result.accepted) {
          if (result.task != null) {
            final task = TodoTask.fromJson(result.task!);
            await _db.upsertTask(task.toCompanion());
          }
          await _db.removePendingOp(op.id);
        } else {
          await _db.markPendingOpError(
            op.id,
            result.error ?? 'UNKNOWN_OPERATION_ERROR',
          );
        }
      }
      cursor = ((await api.changes(
            deviceId: deviceId,
            after: cursor,
          ))['cursor'] as num?)
              ?.toInt() ??
          cursor;
    } catch (e) {
      error = e.toString();
      rethrow;
    } finally {
      flushing = false;
      _notify();
    }
  }

  Future<void> _pullChanges() async {
    final api = _api;
    if (api == null) return;
    final prefs = await SharedPreferences.getInstance();
    final deviceId = prefs.getString(_kDeviceId);
    if (deviceId == null) return;
    final response = await api.changes(deviceId: deviceId, after: cursor);
    final tasks =
        (response['tasks'] as List? ?? const []).cast<Map<String, dynamic>>();
    for (final json in tasks) {
      final task = TodoTask.fromJson(json);
      await _db.upsertTask(task.toCompanion());
    }
    cursor = (response['cursor'] as num?)?.toInt() ?? cursor;
    lastSyncAt = DateTime.now().toUtc().toIso8601String();
    await prefs.setInt(_kCursor, cursor);
    await prefs.setString(_kLastSync, lastSyncAt!);
    onRemoteChange?.call();
  }

  Future<void> pushTask(TodoTask task) async {
    await _db.enqueueOp(
      task.revision == 0 ? 'create' : 'update',
      task.id,
      payload: jsonEncode(task.toJson()),
    );
    pending = (await _db.listPendingOps()).length;
    _notify();
    if (connected) {
      unawaited(syncNow().catchError((_) {}));
    }
  }

  Future<void> deleteRemote(String id) async {
    final local = (await _db.getAllTasksIncludingDeleted()).firstWhere(
      (row) => row.id == id,
      orElse: () => throw StateError('task not found'),
    );
    final task = TodoTask.fromRow(local).copyWith(
      deleted: true,
      updatedAt: DateTime.now().toUtc().toIso8601String(),
    );
    await _db.enqueueOp('delete', id, payload: jsonEncode(task.toJson()));
    pending = (await _db.listPendingOps()).length;
    _notify();
    if (connected) {
      unawaited(syncNow().catchError((_) {}));
    }
  }

  Map<String, dynamic> _operationPayload(Map<String, dynamic> payload) => {
        'title': payload['title'],
        'priority': payload['priority'],
        'remind_at': payload['remind_at'],
        'remind_offset_min': payload['remind_offset_min'],
        'repeat_rule': payload['repeat_rule'],
        'sort_order': payload['sort_order'],
        'completed_at': payload['completed_at'],
        'deleted_at': payload['deleted'] == true
            ? payload['updated_at']
            : payload['deleted_at'],
        'updated_at': DateTime.now().toUtc().toIso8601String(),
      };

  Map<String, dynamic> _buildOperation({
    required String operationId,
    required String taskId,
    required int baseRevision,
    required Map<String, dynamic> payload,
  }) {
    final common = <String, dynamic>{
      'operation_id': operationId,
      'task_id': taskId,
      'base_revision': baseRevision,
      'client_updated_at': DateTime.now().toUtc().toIso8601String(),
    };
    if (baseRevision == 0) {
      return {
        ...common,
        'create': {
          'id': taskId,
          'title': payload['title'],
          'priority': payload['priority'],
          'remind_at': payload['remind_at'],
          'remind_offset_min': payload['remind_offset_min'],
          'repeat_rule': payload['repeat_rule'],
          'sort_order': payload['sort_order'],
          'completed_at': payload['completed_at'],
          'deleted_at': payload['deleted'] == true
              ? payload['updated_at']
              : payload['deleted_at'],
          'created_at': payload['created_at'],
          'updated_at': payload['updated_at'],
          'revision': 0,
        },
      };
    }
    return {...common, 'patch': _operationPayload(payload)};
  }

  Future<_OperationResult> _submitOperation(
    SyncApi api,
    String deviceId,
    Map<String, dynamic> operation,
  ) async {
    final response = await api.operations(deviceId, {
      'device_id': deviceId,
      'operations': [operation],
    });
    final results =
        (response['results'] as List? ?? const []).cast<Map<String, dynamic>>();
    if (results.isEmpty) {
      return const _OperationResult(
        accepted: false,
        error: 'EMPTY_OPERATION_RESPONSE',
      );
    }
    final result = results.first;
    return _OperationResult(
      accepted: result['accepted'] == true,
      task: result['task'] is Map
          ? Map<String, dynamic>.from(result['task'] as Map)
          : null,
      error: result['error']?.toString(),
    );
  }

  void _startRealtimePolling() {
    _realtimeTimer?.cancel();
    _realtimeTimer = Timer.periodic(const Duration(seconds: 3), (_) {
      if (connected && !syncing) {
        unawaited(syncNow().catchError((_) {}));
      }
    });
    _timer?.cancel();
    _timer = Timer(const Duration(seconds: 1), () {
      unawaited(syncNow().catchError((_) {}));
    });
  }
}

class _OperationResult {
  const _OperationResult({
    required this.accepted,
    this.task,
    this.error,
  });

  final bool accepted;
  final Map<String, dynamic>? task;
  final String? error;
}
