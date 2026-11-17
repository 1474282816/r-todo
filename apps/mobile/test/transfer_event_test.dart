import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:r_todo_mobile/features/chat/chat_message.dart';
import 'package:r_todo_mobile/features/transfer/transfer_event.dart';

void main() {
  group('parseTransferEvent', () {
    test('解析 progress 事件', () {
      final event = parseTransferEvent(jsonEncode({
        'schema_version': 2,
        'type': 'progress',
        'session_id': 's1',
        'file_id': 'f1',
        'direction': 'send',
        'transferred': 1024,
        'total': 4096,
        'percent': 25.0,
      }));

      expect(event, isA<TransferProgress>());
      final progress = event as TransferProgress;
      expect(progress.schemaVersion, 2);
      expect(progress.sessionId, 's1');
      expect(progress.fileId, 'f1');
      expect(progress.direction, TransferDirection.send);
      expect(progress.transferred, 1024);
      expect(progress.total, 4096);
      expect(progress.percent, 25.0);
    });

    test('解析 incoming 事件（可选字段缺省）', () {
      final event = parseTransferEvent(jsonEncode({
        'schema_version': 2,
        'type': 'incoming',
        'session_id': 's2',
        'ip': '192.168.1.9',
        'device': {'alias': 'Phone', 'fingerprint': 'FP'},
        'files': [
          {
            'id': 'f1',
            'file_name': '报告.pdf',
            'size': 12345,
            'file_type': 'application/pdf',
          },
          {'id': 'f2', 'file_name': 'b.bin', 'size': 0},
        ],
      }));

      expect(event, isA<TransferIncoming>());
      final incoming = event as TransferIncoming;
      expect(incoming.ip, '192.168.1.9');
      expect(incoming.device.alias, 'Phone');
      expect(incoming.device.fingerprint, 'FP');
      expect(incoming.files, hasLength(2));
      expect(incoming.files.first.fileName, '报告.pdf');
      expect(incoming.files.first.fileType, 'application/pdf');
      expect(incoming.files.last.size, 0);
      expect(incoming.files.last.fileType, isNull);
      expect(incoming.files.last.sha256, isNull);
    });

    test('解析 devices 事件（channels / protocol）', () {
      final event = parseTransferEvent(jsonEncode({
        'schema_version': 2,
        'type': 'devices',
        'devices': [
          {
            'alias': 'PC',
            'version': '2.2',
            'device_model': 'Windows',
            'device_type': 'desktop',
            'fingerprint': 'ABC',
            'download': false,
            'channels': [
              {'host': '192.168.1.5', 'port': 54417, 'protocol': 'https'},
            ],
          },
        ],
      }));

      expect(event, isA<TransferDevices>());
      final devices = (event as TransferDevices).devices;
      expect(devices, hasLength(1));
      expect(devices.single.alias, 'PC');
      expect(devices.single.deviceModel, 'Windows');
      expect(devices.single.deviceType, 'desktop');
      expect(devices.single.channels.single.host, '192.168.1.5');
      expect(devices.single.channels.single.port, 54417);
      expect(devices.single.channels.single.protocol, TransferProtocol.https);
    });

    test('解析 completed 事件', () {
      final event = parseTransferEvent(jsonEncode({
        'schema_version': 2,
        'type': 'completed',
        'session_id': 's4',
        'file_id': 'f4',
        'direction': 'receive',
        'path': '/storage/emulated/0/Download/r-todo/a.pdf',
      }));

      expect(event, isA<TransferCompleted>());
      final completed = event as TransferCompleted;
      expect(completed.direction, TransferDirection.receive);
      expect(completed.path, '/storage/emulated/0/Download/r-todo/a.pdf');
    });

    test('解析 error 事件（错误码原样保留）', () {
      final event = parseTransferEvent(jsonEncode({
        'schema_version': 2,
        'type': 'error',
        'scope': 'receive',
        'code': 'PIN_INVALID',
        'message': 'PIN 错误',
      }));

      expect(event, isA<TransferError>());
      final error = event as TransferError;
      expect(error.code, 'PIN_INVALID');
      expect(error.scope, TransferErrorScope.receive);
      expect(error.message, 'PIN 错误');
    });

    test('解析 session_ended 事件', () {
      final event = parseTransferEvent(jsonEncode({
        'schema_version': 2,
        'type': 'session_ended',
        'session_id': 's3',
        'reason': 'completed',
      }));

      expect(event, isA<TransferSessionEnded>());
      final ended = event as TransferSessionEnded;
      expect(ended.reason, TransferSessionEndReason.completed);
    });

    test('未知事件类型返回 null（前向兼容）', () {
      final event = parseTransferEvent(jsonEncode({
        'schema_version': 2,
        'type': 'something_new',
      }));
      expect(event, isNull);
    });

    test('schema_version 不匹配抛 FormatException', () {
      expect(
        () => parseTransferEvent(jsonEncode({
          'schema_version': 3,
          'type': 'progress',
        })),
        throwsFormatException,
      );
    });

    test('message 事件解析出 ChatMessage', () {
      final event = parseTransferEvent(jsonEncode({
        'schema_version': 2,
        'type': 'message',
        'message': {
          'id': 'm1',
          'conversation_id': 'PEER',
          'sender': 'PEER',
          'sender_alias': '手机',
          'kind': 'text',
          'body': '你好',
          'created_at': 1700000000000,
          'status': 'sent',
        },
      }));
      expect(event, isA<TransferMessage>());
      final message = (event! as TransferMessage).message;
      expect(message.body, '你好');
      expect(message.kind, ChatMessageKind.text);
      expect(message.conversationId, 'PEER');
    });

    test('非法 JSON 抛 FormatException', () {
      expect(() => parseTransferEvent('not json'), throwsFormatException);
    });
  });
}
