import 'chat_message.dart';

/// 会话式互传的存储依赖。
///
/// 抽成接口有两个原因：
/// 1. 生产用 [AppDatabase]（drift / SQLite），单测用内存实现，不必在测试里加载原生 SQLite；
/// 2. 会话层不直接依赖 drift，避免把 SQL 细节漏进业务逻辑。
abstract class ChatStore {
  /// 建表（幂等）。待办的表不在这里，动它们会破坏既有数据。
  Future<void> createChatTables();

  Future<List<Map<String, dynamic>>> listChatConversations();

  Future<void> upsertChatConversation(
    String id,
    String alias,
    int at, {
    int unreadDelta = 0,
  });

  Future<void> markChatRead(String id);

  Future<List<Map<String, dynamic>>> listChatMessages(String conversationId);

  Future<void> insertChatMessage(ChatMessage message, String direction);

  Future<void> patchChatMessage(
    String id, {
    String? status,
    String? filePath,
    double? progress,
    String? forwardedFrom,
  });

  Future<void> deleteChatMessage(String id);

  Future<void> clearChatConversation(String id);

  Future<bool> isChatTrusted(String fingerprint);

  Future<void> trustChatPeer(String fingerprint);

  Future<void> revokeChatTrust(String fingerprint);

  Future<void> enqueueChatOutbox(String target, String payload, String id);

  Future<List<Map<String, dynamic>>> listChatOutbox();

  Future<void> removeChatOutbox(String id);

  Future<void> bumpChatOutbox(String id, String error);
}
