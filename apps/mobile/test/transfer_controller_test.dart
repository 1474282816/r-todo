import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:r_todo_mobile/features/transfer/transfer_event.dart';
import 'package:r_todo_mobile/features/transfer/transfer_favorites.dart';
import 'package:r_todo_mobile/features/transfer/transfer_service.dart';
import 'package:r_todo_mobile/features/transfer/transfer_settings.dart';

class _FakeBackend implements TransferBackend {
  final _events = StreamController<TransferEvent>.broadcast();

  TransferLaunchConfig? config;
  List<String> sentPaths = const [];
  String? sentFingerprint;
  final List<String> prepared = [];
  final List<String> respondedFiles = [];
  final List<String> rejected = [];
  final List<String> cancelled = [];
  bool stopped = false;
  Object? sendError;

  @override
  Stream<TransferEvent> get events => _events.stream;

  @override
  Future<void> start(TransferLaunchConfig config) async => this.config = config;

  @override
  String devicesJson() => '{"schema_version":2,"type":"devices","devices":[]}';

  @override
  Future<void> refresh() async {}

  @override
  Future<String?> addManualIp(String host, int port) async => '{"alias":"PC"}';

  @override
  Future<String> send(List<String> paths, String fingerprint, String? pin) async {
    if (sendError != null) throw sendError!;
    sentPaths = paths;
    sentFingerprint = fingerprint;
    return 'session-1';
  }

  @override
  Future<void> respondPrepare(String sessionId, List<String> acceptedFileIds) async {
    prepared.addAll(acceptedFileIds);
  }

  @override
  Future<void> respondFile(String sessionId, String fileId, String savePath) async {
    respondedFiles.add(savePath);
  }

  @override
  Future<void> reject(String sessionId) async => rejected.add(sessionId);

  @override
  Future<void> cancel(String sessionId) async => cancelled.add(sessionId);

  @override
  Future<void> stop() async => stopped = true;

  void emit(TransferEvent event) => _events.add(event);
}

class _FakePlatform implements TransferPlatform {
  List<StagedFile> picked = const [];
  SavedFile? saved;
  final List<String> calls = [];
  final List<String> cleaned = [];

  @override
  Future<List<StagedFile>> pickFiles({bool multiple = true}) async {
    calls.add('pickFiles');
    return picked;
  }

  @override
  Future<SavedFile> saveToDownloads({
    required String tempPath,
    required String fileName,
    String? mime,
  }) async {
    calls.add('save:$fileName');
    return saved ?? SavedFile(uri: 'content://x', displayName: fileName);
  }

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
  Future<bool> requestNotificationPermission() async {
    calls.add('permission');
    return true;
  }

  @override
  Future<void> cleanupCache(List<String> paths) async => cleaned.addAll(paths);
}

TransferDevice _device({String fingerprint = 'FP'}) => TransferDevice(
      alias: 'PC',
      version: '2.2',
      fingerprint: fingerprint,
      download: false,
      deviceType: 'desktop',
      channels: const [
        TransferChannel(host: '192.168.1.5', port: 54417, protocol: TransferProtocol.https),
      ],
    );

TransferIncoming _incoming({String fingerprint = 'FP'}) => TransferIncoming(
      1,
      sessionId: 's1',
      ip: '192.168.1.5',
      device: TransferDeviceSummary(alias: 'PC', fingerprint: fingerprint),
      files: const [
        TransferFile(id: 'f1', fileName: '报告.pdf', size: 2048, fileType: 'application/pdf'),
      ],
    );

void main() {
  late _FakeBackend backend;
  late _FakePlatform platform;
  late TransferController controller;

  setUp(() {
    SharedPreferences.setMockInitialValues({});
    backend = _FakeBackend();
    platform = _FakePlatform();
    controller = TransferController(
      backend: backend,
      platform: platform,
      settingsStore: TransferSettingsStore(),
      favoritesStore: TransferFavoritesStore(),
      cacheRoot: () async => 'C:/cache',
    );
  });

  tearDown(() => controller.dispose());

  Future<void> settle() => Future<void>.delayed(Duration.zero);

  test('启动：拉组播锁、把 cache 内的 inbox 目录交给 Rust', () async {
    await controller.start();
    await settle();

    expect(controller.runtime, TransferRuntime.running);
    expect(platform.calls, containsAll(['permission', 'lock']));
    expect(backend.config?.receiveDir, 'C:/cache/transfer/inbox');
    expect(backend.config?.alias, TransferSettings.defaultAlias);
  });

  test('devices 事件刷新设备列表', () async {
    await controller.start();
    backend.emit(TransferDevices(1, [_device()]));
    await settle();

    expect(controller.devices, hasLength(1));
    expect(controller.devices.single.alias, 'PC');
  });

  test('发送：选中文件后把 cache 路径交给 Rust，并开前台服务', () async {
    platform.picked = const [
      StagedFile(path: '/cache/outbox/a', name: 'a.pdf', size: 10),
      StagedFile(path: '/cache/outbox/b', name: 'b.png', size: 20),
    ];
    await controller.start();

    await controller.sendFiles('FP');

    expect(backend.sentPaths, ['/cache/outbox/a', '/cache/outbox/b']);
    expect(backend.sentFingerprint, 'FP');
    expect(platform.calls, contains('foreground'));
    expect(controller.lastError, isNull);
  });

  test('发送失败：错误码转中文并清理 cache、前台服务保持常驻', () async {
    platform.picked = const [StagedFile(path: '/cache/outbox/a', name: 'a', size: 1)];
    await controller.start();
    backend.sendError = StateError('boom');

    await controller.sendFiles('FP');

    expect(controller.lastError, contains('操作失败'));
    expect(platform.cleaned, ['/cache/outbox/a']);
    // 常驻前台服务：出错也不停止，只把通知恢复成常驻文案。
    expect(platform.calls, isNot(contains('foreground-stop')));
  });

  test('PIN_REQUIRED：保留暂存文件，供用户输入 PIN 后重试', () async {
    platform.picked = const [
      StagedFile(path: '/cache/outbox/a', name: 'a', size: 1),
    ];
    await controller.start();
    backend.sendError = StateError(
      '{"code":"PIN_REQUIRED","scope":"send","message":"need pin"}',
    );

    await controller.sendFiles('FP');

    expect(controller.pinRequired, isTrue);
    expect(platform.cleaned, isEmpty, reason: '重试还需要这批文件');

    await controller.discardStaged(platform.picked);
    expect(platform.cleaned, ['/cache/outbox/a']);
    expect(controller.pinRequired, isFalse);
  });

  test('发送完成后任务显示原始文件名，而不是「发送中」', () async {
    platform.picked = const [
      StagedFile(
        path: '/cache/outbox/9f2c-报告.pdf',
        name: '报告.pdf',
        size: 2048,
      ),
    ];
    await controller.start();
    await controller.sendFiles('FP');

    backend.emit(const TransferProgress(
      1,
      sessionId: 's1',
      fileId: 'f1',
      direction: TransferDirection.send,
      transferred: 1024,
      total: 2048,
      percent: 50,
    ));
    await settle();
    expect(controller.tasks.single.done, isFalse);

    backend.emit(const TransferCompleted(
      1,
      sessionId: 's1',
      fileId: 'f1',
      direction: TransferDirection.send,
      path: '/cache/outbox/9f2c-报告.pdf',
    ));
    await settle();

    expect(controller.tasks.single.fileName, '报告.pdf');
    expect(controller.tasks.single.done, isTrue);
    expect(controller.tasks.single.percent, 100);
  });

  test('Completed 丢失时，SessionEnded 也能把发送任务收尾成文件名', () async {
    platform.picked = const [
      StagedFile(path: '/cache/outbox/uuid-照片.png', name: '照片.png', size: 4096),
    ];
    await controller.start();
    await controller.sendFiles('FP');

    backend.emit(const TransferProgress(
      1,
      sessionId: 's9',
      fileId: 'f9',
      direction: TransferDirection.send,
      transferred: 4096,
      total: 4096,
      percent: 100,
    ));
    await settle();
    expect(controller.tasks.single.done, isFalse);

    backend.emit(const TransferSessionEnded(
      1,
      sessionId: 's9',
      reason: TransferSessionEndReason.completed,
    ));
    await settle();

    expect(controller.tasks.single.done, isTrue);
    expect(controller.tasks.single.fileName, '照片.png');
  });

  test('收藏设备 + 自动接受：收到请求直接 prepare/respond', () async {
    SharedPreferences.setMockInitialValues({
      'transfer.favorites': ['FP'],
      'transfer.auto_accept_favorites': true,
    });

    await controller.start();
    backend.emit(_incoming());
    await settle();

    expect(controller.incoming, isEmpty);
    expect(backend.prepared, ['f1']);
    expect(backend.respondedFiles, ['C:/cache/transfer/inbox/f1']);
  });

  test('未知设备：进入待确认列表，不自动接受', () async {
    await controller.start();
    backend.emit(_incoming(fingerprint: 'OTHER'));
    await settle();

    expect(controller.incoming, hasLength(1));
    expect(backend.prepared, isEmpty);
  });

  test('接收完成：交给 Kotlin 存 Downloads，并清理 cache', () async {
    await controller.start();
    backend.emit(_incoming());
    await settle();
    await controller.acceptIncoming(controller.incoming.single);

    backend.emit(const TransferCompleted(
      1,
      sessionId: 's1',
      fileId: 'f1',
      direction: TransferDirection.receive,
      path: 'C:/cache/transfer/inbox/f1',
    ));
    await settle();

    expect(platform.calls, contains('save:报告.pdf'));
    expect(platform.cleaned, contains('C:/cache/transfer/inbox/f1'));
    expect(controller.lastSavedFile, '报告.pdf');
  });

  test('错误事件：按错误码给出可读文案', () async {
    await controller.start();
    backend.emit(const TransferError(
      1,
      scope: TransferErrorScope.send,
      code: 'PIN_INVALID',
      message: 'bad pin',
    ));
    await settle();

    expect(controller.lastError, contains('PIN'));
  });

  test('停止：释放组播锁并清空设备列表', () async {
    await controller.start();
    backend.emit(TransferDevices(1, [_device()]));
    await settle();

    await controller.stop();

    expect(controller.runtime, TransferRuntime.stopped);
    expect(controller.devices, isEmpty);
    expect(backend.stopped, isTrue);
    expect(platform.calls, contains('unlock'));
  });
}
