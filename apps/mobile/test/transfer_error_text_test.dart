import 'package:flutter_test/flutter_test.dart';
import 'package:r_todo_mobile/features/transfer/transfer_error_text.dart';

void main() {
  group('transferErrorText', () {
    const codes = [
      'PORT_IN_USE',
      'FIREWALL',
      'MULTICAST_UNAVAILABLE',
      'TLS',
      'PIN_REQUIRED',
      'PIN_INVALID',
      'REJECTED',
      'CHECKSUM_MISMATCH',
      'CANCELLED',
      'IO',
      'UNKNOWN',
    ];

    test('契约里的每个错误码都有中文文案', () {
      for (final code in codes) {
        final text = transferErrorText(code);
        expect(text, isNotEmpty, reason: '$code 缺少文案');
        expect(text, isNot(equals(code)), reason: '$code 直接回显了错误码');
      }
    });

    test('会话取消不算失败文案', () {
      expect(transferErrorText('CANCELLED'), contains('取消'));
    });

    test('端口占用提示可操作信息', () {
      expect(transferErrorText('PORT_IN_USE'), contains('53317'));
    });

    test('未知错误码带出原始 message', () {
      expect(
        transferErrorText('SOMETHING_NEW', message: '底层炸了'),
        contains('底层炸了'),
      );
    });

    test('未知错误码无 message 时给通用文案', () {
      expect(transferErrorText('SOMETHING_NEW'), isNotEmpty);
    });
  });
}
