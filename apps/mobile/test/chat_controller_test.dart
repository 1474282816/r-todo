import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:r_todo_mobile/features/chat/chat_controller.dart';
import 'package:r_todo_mobile/features/chat/chat_message.dart';
import 'package:r_todo_mobile/features/chat/chat_store.dart';
import 'package:r_todo_mobile/features/transfer/transfer_event.dart';
import 'package:r_todo_mobile/features/transfer/transfer_favorites.dart';
import 'package:r_todo_mobile/features/transfer/transfer_service.dart';
import 'package:r_todo_mobile/features/transfer/transfer_settings.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// 内存版会话存储：单测不必加载原生 SQLite。
class FakeChatStore implements ChatStore {
  final List<Map<String, dynamic>> conversations = [];
  final List<Map<String, dynamic>> messages = [];
  final Set<String> trusted = {};
  final List<Map<String, dynamic>> outbox = [];

  @override
  Future<void> createChatTables() async {}

  @override
  Future<List<Map<String, dynamic>>> listChatConversations() async =>
      List<Map<String, dynamic>>.from(conversations);

  @override
  Future<void> upsertChatConversation(
    String id,
    String alias,
    int at, {
    int unreadDelta = 0,
  }) async {
    final index = conversations.indexWhere((c) => c['id'] == id);
    if (index < 0) {
      conversations.add(<String, dynamic>{
        'id': id,
        'alias': alias,
        'last_message_at': at,
        'unread': unreadDelta,
        'pinned': 0,
      });
      return;
    }
    final row = conversations[index];
    row['alias'] = alias;
    row['last_message_at'] = at;
    row['unread'] = ((row['unread'] as int?) ?? 0) + unreadDelta;
  }

  @override
  Future<void> markChatRead(String id) async {
    final row = conversations.firstWhere(
      (c) => c['id'] == id,
      orElse: () => <String, dynamic>{},
    );
    row['unread'] = 0;
  }

  @override
  Future<List<Map<String, dynamic>>> listChatMessages(String conversationId) async =>
      messages
          .where((m) => m['conversation_id'] == conversationId)
          .map((m) => Map<String, dynamic>.from(m))
          .toList();

  @override
  Future<void> insertChatMessage(ChatMessage message, String direction) async {
    messages.add(<String, dynamic>{
      'id': message.id,
      'conversation_id': message.conversationId,
      'sender': message.sender,
      'sender_alias': message.senderAlias,
      'kind': chatKindName(message.kind),
      'body': message.body,
      'file_path': message.filePath ?? '',
      'file_size': message.fileSize ?? 0,
      'preview': message.preview ?? '',
      'forwarded_from': message.forwardedFrom ?? '',
      'created_at': message.createdAt,
      'status': chatStatusName(message.status),
      'direction': direction,
      'progress': 0,
    });
  }

  @override
  Future<void> patchChatMessage(
    String id, {
    String? status,
    String? filePath,
    double? progress,
    String? forwardedFrom,
  }) async {
    final row = messages.firstWhere(
      (m) => m['id'] == id,
      orElse: () => <String, dynamic>{},
    );
    if (status != null) row['status'] = status;
    if (filePath != null) row['file_path'] = filePath;
    if (progress != null) row['progress'] = progress;
    if (forwardedFrom != null) row['forwarded_from'] = forwardedFrom;
  }

  @override
  Future<void> deleteChatMessage(String id) async {
    messages.removeWhere((m) => m['id'] == id);
  }

  @override
  Future<void> clearChatConversation(String id) async {
    messages.removeWhere((m) => m['conversation_id'] == id);
    conversations.removeWhere((c) => c['id'] == id);
  }

  @override
  Future<bool> isChatTrusted(String fingerprint) async =>
      trusted.contains(fingerprint.toUpperCase());

  @override
  Future<void> trustChatPeer(String fingerprint) async {
    trusted.add(fingerprint.toUpperCase());
  }

  @override
  Future<void> revokeChatTrust(String fingerprint) async {
    trusted.remove(fingerprint.toUpperCase());
  }

  @override
  Future<void> enqueueChatOutbox(String target, String payload, String id) async {
    outbox.add(<String, dynamic>{
      'id': id,
      'payload': payload,
      'target': target,
      'attempts': 0,
      'created_at': DateTime.now().millisecondsSinceEpoch,
    });
  }

  @override
  Future<List<Map<String, dynamic>>> listChatOutbox() async =>
      List<Map<String, dynamic>>.from(outbox);

  @override
  Future<void> removeChatOutbox(String id) async {
    outbox.removeWhere((row) => row['id'] == id);
  }

  @override
  Future<void> bumpChatOutbox(String id, String error) async {
    final row = outbox.firstWhere(
      (r) => r['id'] == id,
      orElse: () => <String, dynamic>{},
    );
    row['attempts'] = ((row['attempts'] as int?) ?? 0) + 1;
    row['last_error'] = error;
  }
}

class FakeBackend implements TransferBackend {
  final StreamController<TransferEvent> _events =
      StreamController<TransferEvent>.broadcast();
  bool failStart = false;
  int refreshCalls = 0;

  @override
  Stream<TransferEvent> get events => _events.stream;

  void emit(TransferEvent event) => _events.add(event);

  @override
  Future<void> start(TransferLaunchConfig config) async {
    if (failStart) throw StateError('端口被占用');
  }

  @override
  String devicesJson() =>
      '{"schema_version":2,"type":"devices","devices":[]}';

  @override
  Future<void> refresh() async {
    refreshCalls++;
  }

  @override
  Future<String?> addManualIp(String host, int port) async => null;

  @override
  Future<String> send(
    List<String> paths,
    String fingerprint,
    String? pin,
  ) async =>
      'session-1';

  @override
  Future<void> respondPrepare(
    String sessionId,
    List<String> acceptedFileIds,
  ) async {}

  @override
  Future<void> respondFile(
    String sessionId,
    String fileId,
    String savePath,
  ) async {}

  @override
  Future<void> reject(String sessionId) async {}

  @override
  Future<void> cancel(String sessionId) async {}

  @override
  Future<void> stop() async {}
}

class FakePlatform implements TransferPlatform {
  final List<String> calls = <String>[];

  @override
  Future<List<StagedFile>> pickFiles({bool multiple = true}) async => const [];

  @override
  Future<SavedFile> saveToDownloads({
    required String tempPath,
    required String fileName,
    String? mime,
  }) async =>
      SavedFile(uri: 'content://x', displayName: fileName);

  @override
  Future<void> acquireMulticastLock() async => calls.add('lock');

  @override
  Future<void> releaseMulticastLock() async => calls.add('unlock');

  @override
  Future<void> startForeground({required String title, required String text}) async =>
      calls.add('foreground');

  @override
  Future<void> stopForeground() async => calls.add('foreground-stop');

  @override
  Future<bool> requestNotificationPermission() async => true;

  @override
  Future<void> cleanupCache(List<String> paths) async {}
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  Future<TransferController> makeTransfer(FakeBackend backend) async {
    final controller = TransferController(
      backend: backend,
      platform: FakePlatform(),
      settingsStore: TransferSettingsStore(),
      favoritesStore: TransferFavoritesStore(),
      cacheRoot: () async => '/tmp/cache',
      identityRoot: () async => '/tmp/identity',
    );
    return controller;
  }

  setUp(() {
    SharedPreferences.setMockInitialValues(<String, Object>{});
  });

  test('init 会自动拉起互传服务，并把运行状态暴露给 UI', () async {
    final backend = FakeBackend();
    final transfer = await makeTransfer(backend);
    final store = FakeChatStore();
    final chat = ChatController(store: store, transfer: transfer);

    await chat.init();

    expect(chat.isRunning, isTrue);
    expect(chat.runtime, TransferRuntime.running);
    expect(chat.lastError, isNull);

    chat.dispose();
    await transfer.stop();
  });

  test('启动失败时把错误原因透出，不再静默吞掉', () async {
    final backend = FakeBackend()..failStart = true;
    final transfer = await makeTransfer(backend);
    final chat = ChatController(store: FakeChatStore(), transfer: transfer);

    await chat.init();

    expect(chat.isRunning, isFalse);
    expect(chat.lastError, contains('端口被占用'));

    chat.dispose();
  });

  test('开关互传：停止后 running 关闭，再开启能恢复', () async {
    final backend = FakeBackend();
    final transfer = await makeTransfer(backend);
    final chat = ChatController(store: FakeChatStore(), transfer: transfer);

    await chat.init();
    expect(chat.isRunning, isTrue);

    await chat.stopService();
    expect(chat.isRunning, isFalse);

    await chat.startService();
    expect(chat.isRunning, isTrue);

    chat.dispose();
    await transfer.stop();
  });

  test('刷新设备会真正触发一次发现刷新', () async {
    final backend = FakeBackend();
    final transfer = await makeTransfer(backend);
    final chat = ChatController(store: FakeChatStore(), transfer: transfer);

    await chat.init();
    await chat.refreshDevices();

    expect(backend.refreshCalls, greaterThan(0));

    chat.dispose();
    await transfer.stop();
  });

  test('收到文本消息：落库 + 会话未读数 +1', () async {
    final backend = FakeBackend();
    final transfer = await makeTransfer(backend);
    final store = FakeChatStore();
    final chat = ChatController(store: store, transfer: transfer);

    await chat.init();

    backend.emit(TransferMessage(2, ChatMessage.fromJson(<String, dynamic>{
      'id': 'm1',
      'conversation_id': 'PEER',
      'sender': 'PEER',
      'sender_alias': '手机',
      'kind': 'text',
      'body': '你好',
      'created_at': 1700000000000,
      'status': 'sent',
    })));
    await Future<void>.delayed(const Duration(milliseconds: 30));

    expect(store.messages.length, 1);
    expect(store.messages.first['body'], '你好');
    expect(store.messages.first['direction'], 'receive');
    expect(store.conversations.length, 1);
    expect(store.conversations.first['unread'], 1);

    chat.dispose();
    await transfer.stop();
  });

  test('trust 消息只登记信任，不进聊天记录', () async {
    final backend = FakeBackend();
    final transfer = await makeTransfer(backend);
    final store = FakeChatStore();
    final chat = ChatController(store: store, transfer: transfer);

    await chat.init();

    backend.emit(TransferMessage(2, ChatMessage.fromJson(<String, dynamic>{
      'id': 'm2',
      'conversation_id': 'PEER',
      'sender': 'PEER',
      'sender_alias': '手机',
      'kind': 'trust',
      'body': 'trust',
      'created_at': 1700000000000,
      'status': 'sent',
    })));
    await Future<void>.delayed(const Duration(milliseconds: 30));

    expect(store.messages, isEmpty);
    expect(store.trusted.contains('PEER'), isTrue);

    chat.dispose();
    await transfer.stop();
  });
}
