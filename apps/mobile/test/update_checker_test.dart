import 'package:flutter_test/flutter_test.dart';
import 'package:r_todo_mobile/update/update_checker.dart';

void main() {
  group('isNewerVersion', () {
    test('新版本大于当前版本', () {
      expect(isNewerVersion('0.0.6', '0.0.5'), isTrue);
      expect(isNewerVersion('0.1.0', '0.0.9'), isTrue);
      expect(isNewerVersion('0.0.10', '0.0.9'), isTrue);
      expect(isNewerVersion('1.0.0', '0.9.9'), isTrue);
    });

    test('相同或更旧版本不算更新', () {
      expect(isNewerVersion('0.0.5', '0.0.5'), isFalse);
      expect(isNewerVersion('0.0.4', '0.0.5'), isFalse);
      expect(isNewerVersion('0.0.5', '0.0.10'), isFalse);
    });
  });

  group('UpdateInfo.fromReleaseJson', () {
    test('从 assets 中挑出 apk 下载地址', () {
      final info = UpdateInfo.fromReleaseJson({
        'tag_name': 'v0.0.5',
        'assets': [
          {
            'name': 'v0.0.5.zip',
            'browser_download_url': 'https://x/v0.0.5.zip'
          },
          {
            'name': 'r-todo_0.0.5.apk',
            'browser_download_url': 'https://x/r-todo_0.0.5.apk'
          },
        ],
      });
      expect(info.version, '0.0.5');
      expect(info.url, 'https://x/r-todo_0.0.5.apk');
    });

    test('没有 apk 资产时 url 为空（如最新 release 是 Windows 版）', () {
      final info = UpdateInfo.fromReleaseJson({
        'tag_name': 'v0.0.4',
        'assets': [
          {'name': 'setup.exe', 'browser_download_url': 'https://x/setup.exe'},
        ],
      });
      expect(info.version, '0.0.4');
      expect(info.url, isEmpty);
    });
  });
}
