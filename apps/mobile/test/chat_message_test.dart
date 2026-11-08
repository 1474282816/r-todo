import 'package:flutter_test/flutter_test.dart';
import 'package:r_todo_mobile/features/chat/chat_controller.dart';
import 'package:r_todo_mobile/features/chat/chat_message.dart';

void main() {
  group('ChatMessage 契约 v2', () {
    test('JSON 往返保留可选字段', () {
      final message = ChatMessage.create(
        conversationId: 'PEER',
        sender: 'SELF',
        senderAlias: '电脑',
        kind: ChatMessageKind.image,
        body: 'a.png',
        filePath: '/tmp/a.png',
        fileSize: 123,
        preview: 'BASE64',
      );
      final back = ChatMessage.fromJson(message.toJson());
      expect(back.id, message.id);
      expect(back.kind, ChatMessageKind.image);
      expect(back.preview, 'BASE64');
      expect(back.fileSize, 123);
      expect(back.status, ChatMessageStatus.sent);
    });

    test('forPeer 把会话 id 换成本机 fingerprint', () {
      final message = ChatMessage.create(
        conversationId: 'PEER',
        sender: 'SELF',
        senderAlias: '电脑',
        kind: ChatMessageKind.text,
        body: 'hi',
      );
      expect(message.forPeer('SELF').conversationId, 'SELF');
      expect(message.conversationId, 'PEER');
    });

    test('trust 消息不进聊天记录', () {
      final message = ChatMessage.fromJson(<String, dynamic>{
        'id': 'm1',
        'conversation_id': 'PEER',
        'sender': 'PEER',
        'sender_alias': '手机',
        'kind': 'trust',
        'body': 'trust',
        'created_at': 1,
        'status': 'sent',
      });
      expect(message.kind, ChatMessageKind.trust);
      expect(chatKindName(message.kind), 'trust');
    });
  });

  group('ChatEntry DB 行解析', () {
    test('空串还原成 null，方向保留', () {
      final entry = ChatEntry.fromRow(<String, dynamic>{
        'id': 'm1',
        'conversation_id': 'PEER',
        'sender': 'PEER',
        'sender_alias': '手机',
        'kind': 'text',
        'body': '你好',
        'file_path': '',
        'file_size': 0,
        'preview': '',
        'forwarded_from': '',
        'created_at': 1700000000000,
        'status': 'sent',
        'direction': 'receive',
        'progress': 0,
      });
      expect(entry.direction, 'receive');
      expect(entry.message.filePath, isNull);
      expect(entry.message.preview, isNull);
      expect(entry.message.body, '你好');
    });

    test('转发标记与缩略图能读回', () {
      final entry = ChatEntry.fromRow(<String, dynamic>{
        'id': 'm2',
        'conversation_id': 'PEER',
        'sender': 'SELF',
        'sender_alias': '电脑',
        'kind': 'image',
        'body': 'a.png',
        'file_path': '/tmp/a.png',
        'file_size': 10,
        'preview': 'BASE64',
        'forwarded_from': '手机',
        'created_at': 1,
        'status': 'sent',
        'direction': 'send',
        'progress': 100,
      });
      expect(entry.message.forwardedFrom, '手机');
      expect(entry.message.preview, 'BASE64');
      expect(entry.progress, 100);
    });
  });

  test('firstOrNullWhere 找不到时返回 null', () {
    expect(firstOrNullWhere<int>(<int>[1, 2], (v) => v == 3), isNull);
    expect(firstOrNullWhere<int>(<int>[1, 2], (v) => v == 2), 2);
  });
}
