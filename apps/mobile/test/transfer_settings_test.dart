import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:r_todo_mobile/features/transfer/transfer_settings.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() {
    SharedPreferences.setMockInitialValues({});
  });

  group('TransferSettingsStore', () {
    test('首次加载用默认值（端口 53317 / 不自动接受 / 无 PIN）', () async {
      final settings = await TransferSettingsStore().load();

      expect(settings.port, 53317);
      expect(settings.alias, isNotEmpty);
      expect(settings.autoAcceptFavorites, isFalse);
      expect(settings.pin, isNull);
      expect(settings.receiveDirName, 'r-todo');
    });

    test('保存后可原样读回', () async {
      final store = TransferSettingsStore();
      await store.save(const TransferSettings(
        alias: '我的手机',
        port: 53400,
        pin: '1234',
        autoAcceptFavorites: true,
      ));

      final settings = await store.load();
      expect(settings.alias, '我的手机');
      expect(settings.port, 53400);
      expect(settings.pin, '1234');
      expect(settings.autoAcceptFavorites, isTrue);
    });

    test('空白 alias 回退到默认值，空 PIN 视为未设置', () async {
      final store = TransferSettingsStore();
      await store.save(const TransferSettings(alias: '   ', port: 53317, pin: ''));

      final settings = await store.load();
      expect(settings.alias, TransferSettings.defaultAlias);
      expect(settings.pin, isNull);
    });

    test('非法端口回退到默认端口', () async {
      final store = TransferSettingsStore();
      await store.save(const TransferSettings(alias: 'A', port: 0));

      final settings = await store.load();
      expect(settings.port, TransferSettings.defaultPort);
    });
  });
}
