import 'dart:math';

/// 与 `crates/transfer-core/src/chat.rs` 的 `ChatMessage` 一一对应（契约 v2）。
enum ChatMessageKind { text, image, file, trust }

enum ChatMessageStatus { pending, sent, failed }

ChatMessageKind chatKind(String raw) => switch (raw) {
      'image' => ChatMessageKind.image,
      'file' => ChatMessageKind.file,
      'trust' => ChatMessageKind.trust,
      _ => ChatMessageKind.text,
    };

String chatKindName(ChatMessageKind kind) => switch (kind) {
      ChatMessageKind.text => 'text',
      ChatMessageKind.image => 'image',
      ChatMessageKind.file => 'file',
      ChatMessageKind.trust => 'trust',
    };

ChatMessageStatus chatStatus(String raw) => switch (raw) {
      'pending' => ChatMessageStatus.pending,
      'failed' => ChatMessageStatus.failed,
      _ => ChatMessageStatus.sent,
    };

String chatStatusName(ChatMessageStatus status) => switch (status) {
      ChatMessageStatus.pending => 'pending',
      ChatMessageStatus.failed => 'failed',
      ChatMessageStatus.sent => 'sent',
    };

class ChatMessage {
  const ChatMessage({
    required this.id,
    required this.conversationId,
    required this.sender,
    required this.senderAlias,
    required this.kind,
    required this.body,
    required this.createdAt,
    required this.status,
    this.filePath,
    this.fileSize,
    this.preview,
    this.forwardedFrom,
  });

  final String id;
  /// 本地库视角：对端 fingerprint（每个设备就是一个好友，无账号）。
  final String conversationId;
  final String sender;
  final String senderAlias;
  final ChatMessageKind kind;
  final String body;
  final int createdAt;
  final ChatMessageStatus status;
  final String? filePath;
  final int? fileSize;
  /// 图片缩略图 base64（不含 data: 前缀）。
  final String? preview;
  final String? forwardedFrom;

  factory ChatMessage.fromJson(Map<String, dynamic> json) => ChatMessage(
        id: json['id'] as String? ?? '',
        conversationId: json['conversation_id'] as String? ?? '',
        sender: json['sender'] as String? ?? '',
        senderAlias: json['sender_alias'] as String? ?? '设备',
        kind: chatKind(json['kind'] as String? ?? 'text'),
        body: json['body'] as String? ?? '',
        createdAt: (json['created_at'] as num?)?.toInt() ??
            DateTime.now().millisecondsSinceEpoch,
        status: chatStatus(json['status'] as String? ?? 'sent'),
        filePath: json['file_path'] as String?,
        fileSize: (json['file_size'] as num?)?.toInt(),
        preview: json['preview'] as String?,
        forwardedFrom: json['forwarded_from'] as String?,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'conversation_id': conversationId,
        'sender': sender,
        'sender_alias': senderAlias,
        'kind': chatKindName(kind),
        'body': body,
        'created_at': createdAt,
        'status': chatStatusName(status),
        if (filePath != null) 'file_path': filePath,
        if (fileSize != null) 'file_size': fileSize,
        if (preview != null) 'preview': preview,
        if (forwardedFrom != null) 'forwarded_from': forwardedFrom,
      };

  ChatMessage copyWith({
    String? conversationId,
    String? sender,
    String? senderAlias,
    ChatMessageKind? kind,
    String? body,
    int? createdAt,
    ChatMessageStatus? status,
    String? filePath,
    int? fileSize,
    String? preview,
    String? forwardedFrom,
  }) =>
      ChatMessage(
        id: id,
        conversationId: conversationId ?? this.conversationId,
        sender: sender ?? this.sender,
        senderAlias: senderAlias ?? this.senderAlias,
        kind: kind ?? this.kind,
        body: body ?? this.body,
        createdAt: createdAt ?? this.createdAt,
        status: status ?? this.status,
        filePath: filePath ?? this.filePath,
        fileSize: fileSize ?? this.fileSize,
        preview: preview ?? this.preview,
        forwardedFrom: forwardedFrom ?? this.forwardedFrom,
      );

  /// 发送给对端时把 conversation_id 换成本机 fingerprint（对端视角）。
  ChatMessage forPeer(String selfFingerprint) =>
      copyWith(conversationId: selfFingerprint);

  static String newId() {
    final random = Random();
    return '${DateTime.now().microsecondsSinceEpoch}-${random.nextInt(1 << 32)}';
  }

  static ChatMessage create({
    required String conversationId,
    required String sender,
    required String senderAlias,
    required ChatMessageKind kind,
    required String body,
    ChatMessageStatus status = ChatMessageStatus.sent,
    String? filePath,
    int? fileSize,
    String? preview,
    String? forwardedFrom,
  }) =>
      ChatMessage(
        id: newId(),
        conversationId: conversationId,
        sender: sender,
        senderAlias: senderAlias,
        kind: kind,
        body: body,
        createdAt: DateTime.now().millisecondsSinceEpoch,
        status: status,
        filePath: filePath,
        fileSize: fileSize,
        preview: preview,
        forwardedFrom: forwardedFrom,
      );
}
