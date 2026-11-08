import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:forui/forui.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:r_todo_mobile/features/transfer/transfer_event.dart';
import 'package:r_todo_mobile/features/transfer/transfer_favorites.dart';
import 'package:r_todo_mobile/features/transfer/transfer_page.dart';
import 'package:r_todo_mobile/features/transfer/transfer_service.dart';
import 'package:r_todo_mobile/features/transfer/transfer_settings.dart';

class _Backend implements TransferBackend {
  final _events = StreamController<TransferEvent>.broadcast();
  List<String> sent = const [];

  @override
  Stream<TransferEvent> get events => _events.stream;

  @override
  Future<void> start(TransferLaunchConfig config) async {}

  @override
  String devicesJson() => '';

  @override
  Future<void> refresh() async {}

  @override
  Future<String?> addManualIp(String host, int port) async => null;

  @override
  Future<String> send(List<String> paths, String fingerprint, String? pin) async {
    sent = paths;
    return 's1';
  }

  @override
  Future<void> respondPrepare(String sessionId, List<String> acceptedFileIds) async {}

  @override
  Future<void> respondFile(String sessionId, String fileId, String savePath) async {}

  @override
  Future<void> reject(String sessionId) async {}

  @override
  Future<void> cancel(String sessionId) async {}

  @override
  Future<void> stop() async {}

  void emit(TransferEvent event) => _events.add(event);
}

class _Platform implements TransferPlatform {
  List<StagedFile> picked = const [];

  @override
  Future<List<StagedFile>> pickFiles({bool multiple = true}) async => picked;

  @override
  Future<SavedFile> saveToDownloads({
    required String tempPath,
    required String fileName,
    String? mime,
  }) async =>
      SavedFile(uri: 'u', displayName: fileName);

  @override
  Future<void> acquireMulticastLock() async {}

  @override
  Future<void> releaseMulticastLock() async {}

  @override
  Future<void> startForeground({required String title, required String text}) async {}

  @override
  Future<void> stopForeground() async {}

  @override
  Future<bool> requestNotificationPermission() async => true;

  @override
  Future<void> cleanupCache(List<String> paths) async {}
}

Widget _app(TransferController controller) {
  return ProviderScope(
    overrides: [
      transferControllerProvider.overrideWith((ref) => controller),
    ],
    child: MaterialApp(
      theme: ThemeData(),
      builder: (context, child) => FTheme(
        data: FTheme.neutral.light.touch,
        child: child!,
      ),
      home: const Scaffold(body: TransferView()),
    ),
  );
}

TransferController _controller(_Backend backend, _Platform platform) {
  return TransferController(
    backend: backend,
    platform: platform,
    settingsStore: TransferSettingsStore(),
    favoritesStore: TransferFavoritesStore(),
    cacheRoot: () async => 'C:/cache',
  );
}

void main() {
  setUp(() => SharedPreferences.setMockInitialValues({}));

  testWidgets('未开启互传时给出说明，不显示设备列表', (tester) async {
    final controller = _controller(_Backend(), _Platform());

    await tester.pumpWidget(_app(controller));
    await tester.pump();

    expect(find.text('附近的设备'), findsOneWidget);
    expect(find.textContaining('打开互传后'), findsOneWidget);
  });

  testWidgets('发送完成后显示文件名与「已完成」，不再显示「发送中」',
      (tester) async {
    final backend = _Backend();
    final platform = _Platform()
      ..picked = const [
        StagedFile(path: '/cache/outbox/uuid-报告.pdf', name: '报告.pdf', size: 10),
      ];
    final controller = _controller(backend, platform);

    await tester.pumpWidget(_app(controller));
    await controller.start();
    await controller.sendFiles('FP');

    backend.emit(const TransferProgress(
      1,
      sessionId: 's1',
      fileId: 'f1',
      direction: TransferDirection.send,
      transferred: 5,
      total: 10,
      percent: 50,
    ));
    await tester.pump();
    expect(find.text('发送中'), findsOneWidget);

    backend.emit(const TransferCompleted(
      1,
      sessionId: 's1',
      fileId: 'f1',
      direction: TransferDirection.send,
      path: '/cache/outbox/uuid-报告.pdf',
    ));
    await tester.pump(const Duration(milliseconds: 10));
    await tester.pump(const Duration(milliseconds: 10));

    expect(find.text('报告.pdf'), findsOneWidget);
    expect(find.text('已完成'), findsOneWidget);
    expect(find.text('发送中'), findsNothing);
  });

  testWidgets('发现设备后列表显示名字，点击行即发送', (tester) async {
    final backend = _Backend();
    final platform = _Platform()
      ..picked = const [StagedFile(path: '/cache/outbox/a', name: 'a.pdf', size: 8)];
    final controller = _controller(backend, platform);

    await tester.pumpWidget(_app(controller));
    await controller.start();

    backend.emit(const TransferDevices(1, [
      TransferDevice(
        alias: '客厅电脑',
        version: '2.2',
        fingerprint: 'ABCDEF123456',
        download: false,
        channels: [
          TransferChannel(host: '192.168.1.5', port: 53317, protocol: TransferProtocol.https),
        ],
      ),
    ]));
    await tester.pump();
    await tester.pump();

    expect(find.text('客厅电脑'), findsOneWidget);

    await tester.tap(find.text('客厅电脑'));
    await tester.pump();

    expect(backend.sent, ['/cache/outbox/a']);
    expect(controller.lastError, isNull);
  });
}
