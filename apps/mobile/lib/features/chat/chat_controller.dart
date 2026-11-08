import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../../data/database.dart';
import '../transfer/transfer_event.dart';
import '../transfer/transfer_service.dart';
import 'chat_event_bus.dart';
import 'chat_message.dart';

class ChatConversation {
  const ChatConversation({
    required this.id,
    required this.alias,
    required this.lastMessageAt,
    required this.unread,
    required this.pinned,
  });

  final String id;
  final String alias;
  final int lastMessageAt;
  final int unread;
  final bool pinned;

  factory ChatConversation.fromRow(Map<String, dynamic> row) => ChatConversation(
        id: row['id'] as String? ?? '',
        alias: row['alias'] as String? ?? '设备',
        lastMessageAt: (row['last_message_at'] as num?)?.toInt() ?? 0,
        unread: (row['unread'] as num?)?.toInt() ?? 0,
        pinned: (row['pinned'] as num?)?.toInt() == 1,
      );
}

class ChatEntry {
  const ChatEntry({
    required this.message,
    required this.direction,
    this.progress = 0,
  });

  final ChatMessage message;
  /// `send` / `receive`：删除时只有 receive 方向能删本地文件（Q32）。
  final String direction;
  final double progress;

  factory ChatEntry.fromRow(Map<String, dynamic> row) {
    String? optional(Object? value) =>
        (value is String && value.isNotEmpty) ? value : null;
    final message = ChatMessage.fromJson(<String, dynamic>{
      'id': row['id'],
      'conversation_id': row['conversation_id'],
      'sender': row['sender'],
      'sender_alias': row['sender_alias'],
      'kind': row['kind'],
      'body': row['body'],
      'created_at': row['created_at'],
      'status': row['status'],
      if (optional(row['file_path']) != null) 'file_path': row['file_path'],
      if ((row['file_size'] as num?)?.toInt() != 0)
        'file_size': row['file_size'],
      if (optional(row['preview']) != null) 'preview': row['preview'],
      if (optional(row['forwarded_from']) != null)
        'forwarded_from': row['forwarded_from'],
    });
    return ChatEntry(
      message: message,
      direction: row['direction'] as String? ?? 'receive',
      progress: (row['progress'] as num?)?.toDouble() ?? 0,
    );
  }
}

T? firstOrNullWhere<T>(Iterable<T> items, bool Function(T) test) {
  for (final item in items) {
    if (test(item)) return item;
  }
  return null;
}

class TrustPrompt {
  const TrustPrompt(this.event);
  final TransferIncoming event;

  String get fingerprint => event.device.fingerprint.toUpperCase();
  String get alias => event.device.alias;
}

/// 会话式互传的状态与业务：消息收发、首次确认、转发、删除、通知、待发补投。
class ChatController extends ChangeNotifier {
  ChatController({required this.db, required this.transfer});

  final AppDatabase db;
  final TransferController transfer;

  static const MethodChannel _platform = MethodChannel('rtodo/chat');
  static const MethodChannel _transferChannel = MethodChannel('rtodo/transfer');

  List<ChatConversation> conversations = <ChatConversation>[];
  List<ChatEntry> messages = <ChatEntry>[];
  List<TransferDevice> devices = <TransferDevice>[];
  String? activeId;
  TrustPrompt? trustPrompt;
  String? notice;
  /// 点击通知带进来的会话，列表页会自动推到详情页（Q45）。
  String? pendingOpenConversation;
  String selfFingerprint = '';
  String selfAlias = 'r-todo';

  StreamSubscription<TransferEvent>? _subscription;
  Timer? _retry;
  bool _started = false;

  Future<void> init() async {
    if (_started) return;
    _started = true;
    await db.createChatTables();
    selfAlias = transfer.settings.alias;
    try {
      final dir = await transfer.identityDirectory();
      final file = File('$dir/transfer-fingerprint.txt');
      if (await file.exists()) {
        selfFingerprint = (await file.readAsString()).trim().toUpperCase();
      }
    } catch (_) {
      // 身份目录不可读时退化为临时身份，聊天仍可用但重启会换好友。
    }
    await refreshConversations();
    devices = transfer.devices;
    _subscription = ChatEventBus.instance.stream.listen(_onEvent);
    _retry = Timer.periodic(const Duration(seconds: 5), (_) => flushOutbox());
    await _consumeNotificationIntent();
    if (!transfer.isRunning) {
      try {
        await transfer.start();
      } catch (_) {
        // 服务未就绪时仍可浏览历史
      }
    }
    notifyListeners();
  }

  @override
  void dispose() {
    _subscription?.cancel();
    _retry?.cancel();
    super.dispose();
  }

  void flash(String text) {
    notice = text;
    notifyListeners();
    Future<void>.delayed(const Duration(seconds: 3), () {
      if (notice == text) {
        notice = null;
        notifyListeners();
      }
    });
  }

  /// 通知显示详情（正文/文件名），并让点击直达该会话（Q45）。
  Future<void> _notify(String fingerprint, String title, String body) async {
    await transfer.showChatNotification(title, body);
    try {
      await _platform.invokeMethod<void>(
        'showNotification',
        <String, Object?>{
          'title': title,
          'text': body,
          'conversation': fingerprint,
        },
      );
    } on MissingPluginException {
      // 非 Android 环境（单测）忽略
    } on PlatformException catch (_) {
      // 通知失败不影响收消息
    }
  }

  /// 点击常驻通知后，Kotlin 把 fingerprint 存到 pendingConversation。
  Future<void> _consumeNotificationIntent() async {
    try {
      final fingerprint =
          await _platform.invokeMethod<String>('pendingConversation');
      if (fingerprint != null && fingerprint.isNotEmpty) {
        pendingOpenConversation = fingerprint.toUpperCase();
        await openConversation(pendingOpenConversation!);
        notifyListeners();
      }
    } on MissingPluginException {
      // 非 Android 环境
    } on PlatformException catch (_) {
      // 忽略
    }
  }

  Future<void> refreshConversations() async {
    final rows = await db.listChatConversations();
    conversations = rows.map(ChatConversation.fromRow).toList(growable: false);
    notifyListeners();
  }

  Future<void> refreshMessages() async {
    final id = activeId;
    if (id == null) {
      messages = <ChatEntry>[];
      notifyListeners();
      return;
    }
    final rows = await db.listChatMessages(id);
    messages = rows.map(ChatEntry.fromRow).toList(growable: false);
    notifyListeners();
  }

  Future<void> openConversation(String id) async {
    activeId = id;
    await db.markChatRead(id);
    await refreshMessages();
    await refreshConversations();
  }

  // ─────────────────── 发送 ───────────────────

  TransferChannel? _channelOf(String fingerprint) {
    for (final device in devices) {
      if (device.fingerprint.toUpperCase() == fingerprint.toUpperCase()) {
        return device.preferredChannel;
      }
    }
    return null;
  }

  Future<void> _post(ChatMessage message, String target) async {
    final channel = _channelOf(target);
    if (channel == null) throw StateError('目标设备不在线，消息已排队');
    final dir = await transfer.identityDirectory();
    final context = SecurityContext(withTrustedRoots: false)
      ..useCertificateChainBytes(
          utf8.encode(await File('$dir/transfer-cert.pem').readAsString()))
      ..usePrivateKeyBytes(
          utf8.encode(await File('$dir/transfer-key.pem').readAsString()));
    final client = HttpClient(context: context)
      ..connectionTimeout = const Duration(seconds: 5)
      ..badCertificateCallback =
          (X509Certificate cert, String host, int port) => true;
    try {
      final uri = Uri(
        scheme: channel.protocol == TransferProtocol.http ? 'http' : 'https',
        host: channel.host,
        port: channel.port,
        path: '/api/rtodo/v1/message',
      );
      final request = await client.postUrl(uri);
      request.headers.contentType = ContentType.json;
      request.write(jsonEncode(message.forPeer(selfFingerprint).toJson()));
      final response = await request.close();
      await response.drain<void>();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw StateError('HTTP ${response.statusCode}');
      }
    } finally {
      client.close();
    }
  }

  Future<void> sendText(String text) async {
    final id = activeId;
    if (id == null || text.trim().isEmpty) return;
    final message = ChatMessage.create(
      conversationId: id,
      sender: selfFingerprint,
      senderAlias: selfAlias,
      kind: ChatMessageKind.text,
      body: text.trim(),
      status: ChatMessageStatus.pending,
    );
    await _store(message, 'send');
    try {
      await _post(message, id);
      await db.patchChatMessage(message.id, status: 'sent');
    } catch (error) {
      await db.patchChatMessage(message.id, status: 'failed');
      await db.enqueueChatOutbox(id, jsonEncode(message.toJson()), message.id);
      flash('对方不在线，消息已排队，恢复后自动补投');
    }
    await refreshMessages();
  }

  Future<String?> _thumbnail(String path, String? mime) async {
    if (mime == null || !mime.startsWith('image/')) return null;
    try {
      final bytes = await File(path).readAsBytes();
      final codec = await ui.instantiateImageCodec(bytes, targetWidth: 320);
      final frame = await codec.getNextFrame();
      final data = await frame.image.toByteData(format: ui.ImageByteFormat.png);
      codec.dispose();
      if (data == null) return null;
      return base64Encode(data.buffer.asUint8List());
    } catch (_) {
      return null;
    }
  }

  /// 会话内发文件/图片：SAF 选文件 → 暂存路径 → 原 v2 文件链路。
  Future<void> sendFiles() async {
    final id = activeId;
    if (id == null) return;
    List<Map<Object?, Object?>>? raw;
    try {
      raw = await _transferChannel.invokeListMethod<Map<Object?, Object?>>(
        'pickFiles',
        <String, Object?>{'multiple': true},
      );
    } on MissingPluginException {
      return;
    }
    final picked = (raw ?? <Map<Object?, Object?>>[])
        .map((item) => StagedFile(
              path: (item['path'] ?? '').toString(),
              name: (item['name'] ?? '').toString(),
              size: (item['size'] as num?)?.toInt() ?? 0,
              mime: item['mime']?.toString(),
            ))
        .where((file) => file.path.isNotEmpty)
        .toList(growable: false);
    if (picked.isEmpty) return;

    for (final file in picked) {
      final image = file.mime?.startsWith('image/') ?? false;
      final preview = await _thumbnail(file.path, file.mime);
      final message = ChatMessage.create(
        conversationId: id,
        sender: selfFingerprint,
        senderAlias: selfAlias,
        kind: image ? ChatMessageKind.image : ChatMessageKind.file,
        body: file.name,
        filePath: file.path,
        fileSize: file.size,
        preview: preview,
        status: ChatMessageStatus.pending,
      );
      await _store(message, 'send');
      // 图片元数据也走消息端点，对端不必依赖 FileDto.preview 也能看到图。
      if (image && preview != null) {
        try {
          await _post(message, id);
        } catch (_) {
          // 元数据丢了也没关系，原图仍走文件链路
        }
      }
    }
    await transfer.sendPaths(
      id,
      picked.map((file) => file.path).toList(growable: false),
    );
    await refreshMessages();
  }

  Future<void> _store(ChatMessage message, String direction) async {
    await db.insertChatMessage(message, direction);
    await db.upsertChatConversation(
      message.conversationId,
      message.senderAlias,
      message.createdAt,
    );
    await refreshConversations();
    await refreshMessages();
  }

  // ─────────────────── 事件 ───────────────────

  Future<void> _onEvent(TransferEvent event) async {
    switch (event) {
      case TransferMessage(:final message):
        await _onMessage(message);
      case TransferDevices(:final devices):
        this.devices = devices;
        notifyListeners();
        await flushOutbox();
      case TransferIncoming():
        await _onIncoming(event);
      case TransferCompleted(:final direction, :final path):
        if (direction == TransferDirection.receive && path != null) {
          await _attachReceivedPath(path);
        }
      default:
        break;
    }
  }

  Future<void> _onMessage(ChatMessage message) async {
    if (message.kind == ChatMessageKind.trust) {
      await db.trustChatPeer(message.sender);
      flash('${message.senderAlias} 已信任本设备，后续文件免确认');
      return;
    }
    if (message.sender.isNotEmpty && message.sender == selfFingerprint) return;
    final peer = message.sender;
    final incoming = message.copyWith(conversationId: peer);
    await db.insertChatMessage(incoming, 'receive');
    await db.upsertChatConversation(
      peer,
      message.senderAlias,
      message.createdAt,
      unreadDelta: activeId == peer ? 0 : 1,
    );
    // 通知显示详情（Q45），前台服务常驻保证后台也能收到（Q30）。
    await _notify(
      peer,
      '${message.senderAlias} 发来消息',
      message.kind == ChatMessageKind.text
          ? message.body
          : '📎 ${message.body}',
    );
    await refreshConversations();
    await refreshMessages();
  }

  Future<void> _onIncoming(TransferIncoming event) async {
    final prompt = TrustPrompt(event);
    final trusted = await db.isChatTrusted(prompt.fingerprint);
    if (trusted) {
      await _accept(prompt, markTrusted: false);
    } else {
      trustPrompt = prompt;
      notifyListeners();
      await _notify(
        prompt.fingerprint,
        '${prompt.alias} 发来文件',
        '首次接收需要确认（之后免确认）',
      );
    }
  }

  Future<void> confirmTrust(bool accept) async {
    final prompt = trustPrompt;
    if (prompt == null) return;
    trustPrompt = null;
    notifyListeners();
    if (accept) {
      await _accept(prompt, markTrusted: true);
    } else {
      final request = firstOrNullWhere(
        transfer.incoming,
        (item) => item.sessionId == prompt.event.sessionId,
      );
      if (request != null) await transfer.rejectIncoming(request);
    }
  }

  Future<void> _accept(TrustPrompt prompt, {required bool markTrusted}) async {
    final request = firstOrNullWhere(
      transfer.incoming,
      (item) => item.sessionId == prompt.event.sessionId,
    );
    if (request != null) await transfer.acceptIncoming(request);
    if (markTrusted) {
      await db.trustChatPeer(prompt.fingerprint);
      await transfer.toggleFavorite(prompt.fingerprint);
      final ack = ChatMessage.create(
        conversationId: prompt.fingerprint,
        sender: selfFingerprint,
        senderAlias: selfAlias,
        kind: ChatMessageKind.trust,
        body: 'trust',
      );
      try {
        await _post(ack, prompt.fingerprint);
      } catch (_) {
        // 回执丢失时各自方向再确认一次即可（Q36）
      }
    }
    for (final file in prompt.event.files) {
      final image = (file.fileType?.startsWith('image/') ?? false) ||
          RegExp(r'\.(png|jpe?g|gif|webp|bmp)$', caseSensitive: false)
              .hasMatch(file.fileName);
      final message = ChatMessage.create(
        conversationId: prompt.fingerprint,
        sender: prompt.fingerprint,
        senderAlias: prompt.alias,
        kind: image ? ChatMessageKind.image : ChatMessageKind.file,
        body: file.fileName,
        fileSize: file.size,
        preview: file.preview,
        status: ChatMessageStatus.pending,
      );
      await db.insertChatMessage(message, 'receive');
    }
    await db.upsertChatConversation(
        prompt.fingerprint, prompt.alias, DateTime.now().millisecondsSinceEpoch);
    await refreshConversations();
    await refreshMessages();
  }

  Future<void> _attachReceivedPath(String path) async {
    final rows = await db.listChatMessages(activeId ?? '');
    for (final row in rows.reversed) {
      if (row['direction'] == 'receive' &&
          (row['file_path'] == null || row['file_path'] == '')) {
        await db.patchChatMessage(row['id'] as String,
            filePath: path, status: 'sent', progress: 100);
        break;
      }
    }
    await refreshMessages();
  }

  // ─────────────────── 补投 / 转发 / 删除 / 菜单 ───────────────────

  Future<void> flushOutbox() async {
    final rows = await db.listChatOutbox();
    for (final row in rows) {
      final attempts = (row['attempts'] as num?)?.toInt() ?? 0;
      final createdAt = (row['created_at'] as num?)?.toInt() ?? 0;
      final delay = (1 << (attempts.clamp(0, 6))) * 1000;
      if (attempts > 0 && DateTime.now().millisecondsSinceEpoch - createdAt < delay) {
        continue;
      }
      try {
        final message = ChatMessage.fromJson(
            jsonDecode(row['payload'] as String) as Map<String, dynamic>);
        await _post(message, row['target'] as String);
        await db.removeChatOutbox(row['id'] as String);
        await db.patchChatMessage(message.id, status: 'sent');
        await refreshMessages();
      } catch (error) {
        await db.bumpChatOutbox(row['id'] as String, error.toString());
      }
    }
  }

  Future<void> forward(ChatEntry entry, String target, String alias) async {
    final message = entry.message;
    if (message.kind != ChatMessageKind.text) {
      final path = message.filePath;
      if (path == null || !File(path).existsSync()) {
        flash('原文件已删除或移动，无法转发');
        return;
      }
      // 对方拿到独立副本；本机不产生新副本（Q39）。
      await transfer.sendPaths(target, <String>[path]);
    }
    final forwarded = ChatMessage.create(
      conversationId: target,
      sender: selfFingerprint,
      senderAlias: selfAlias,
      kind: message.kind == ChatMessageKind.trust
          ? ChatMessageKind.text
          : message.kind,
      body: message.body,
      filePath: message.filePath,
      fileSize: message.fileSize,
      preview: message.preview,
      forwardedFrom: message.senderAlias,
    );
    await db.insertChatMessage(forwarded, 'send');
    await db.upsertChatConversation(
        target, alias, forwarded.createdAt);
    if (message.kind == ChatMessageKind.text) {
      try {
        await _post(forwarded, target);
      } catch (_) {
        // 文本转发失败会留在下次补投（文件消息本身已走文件链路）
      }
    }
    await db.patchChatMessage(message.id, forwardedFrom: alias);
    await refreshConversations();
    await refreshMessages();
    flash('已转发给 $alias');
  }

  Future<void> deleteEntry(ChatEntry entry) async {
    if (entry.direction == 'receive' && entry.message.filePath != null) {
      try {
        await _platform.invokeMethod<void>('deleteFile', <String, Object?>{
          'uri': entry.message.filePath,
        });
      } on MissingPluginException {
        // 非 Android 环境（单测）只删记录
      } on PlatformException catch (_) {
        // 文件已不存在时同样只删记录
      }
    }
    await db.deleteChatMessage(entry.message.id);
    await refreshMessages();
  }

  Future<void> menuAction(String action, ChatEntry entry) async {
    try {
      await _platform.invokeMethod<void>(action, <String, Object?>{
        'uri': entry.message.filePath,
        'name': entry.message.body,
      });
    } on MissingPluginException {
      flash('该操作仅在 Android 上可用');
    } on PlatformException catch (error) {
      flash(error.message ?? '操作失败');
    }
  }

  Future<void> clearHistory(String id) async {
    await db.clearChatConversation(id);
    if (activeId == id) activeId = null;
    await refreshConversations();
    await refreshMessages();
  }

  Future<void> revokeTrust(String id) async {
    await db.revokeChatTrust(id);
    // 手机端首次确认同时写 favorites，撤销时两边都要清（Q36/Q29）。
    if (transfer.isFavorite(id)) await transfer.toggleFavorite(id);
    flash('已撤销信任，下次发文件会重新确认');
  }
}
