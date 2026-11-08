import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

import '../../src/rust/api/transfer.dart' as rust;
import '../chat/chat_event_bus.dart';
import 'transfer_error_text.dart';
import 'transfer_event.dart';
import 'transfer_favorites.dart';
import 'transfer_settings.dart';

/// 传给 Rust 的启动参数（目录由 Flutter 侧算好）。
class TransferLaunchConfig {
  const TransferLaunchConfig({
    required this.alias,
    required this.port,
    required this.receiveDir,
    required this.identityDir,
    required this.autoAcceptFavorites,
    this.pin,
  });

  final String alias;
  final int port;
  final String receiveDir;
  final String identityDir;
  final bool autoAcceptFavorites;
  final String? pin;
}

enum TransferRuntime { stopped, starting, running }

/// 已选中待发送的文件（Kotlin 已复制到 cache）。
class StagedFile {
  const StagedFile({
    required this.path,
    required this.name,
    required this.size,
    this.mime,
  });

  final String path;
  final String name;
  final int size;
  final String? mime;
}

class SavedFile {
  const SavedFile({required this.uri, required this.displayName});

  final String uri;
  final String displayName;
}

/// Kotlin 平台能力（SAF / MediaStore / 前台服务 / 组播锁）。
abstract class TransferPlatform {
  Future<List<StagedFile>> pickFiles({bool multiple = true});

  Future<SavedFile> saveToDownloads({
    required String tempPath,
    required String fileName,
    String? mime,
  });

  Future<void> acquireMulticastLock();

  Future<void> releaseMulticastLock();

  Future<void> startForeground({required String title, required String text});

  Future<void> stopForeground();

  Future<bool> requestNotificationPermission();

  Future<void> cleanupCache(List<String> paths);
}

class MethodChannelTransferPlatform implements TransferPlatform {
  MethodChannelTransferPlatform({MethodChannel? channel})
      : _channel = channel ?? const MethodChannel('rtodo/transfer');

  final MethodChannel _channel;

  @override
  Future<List<StagedFile>> pickFiles({bool multiple = true}) async {
    List<Map<Object?, Object?>>? raw;
    try {
      raw = await _channel.invokeListMethod<Map<Object?, Object?>>(
        'pickFiles',
        {'multiple': multiple},
      );
    } on MissingPluginException {
      return const [];
    }
    return (raw ?? const <Map<Object?, Object?>>[])
        .map((item) => StagedFile(
              path: (item['path'] ?? '').toString(),
              name: (item['name'] ?? '').toString(),
              size: (item['size'] as num?)?.toInt() ?? 0,
              mime: item['mime']?.toString(),
            ))
        .where((file) => file.path.isNotEmpty)
        .toList(growable: false);
  }

  @override
  Future<SavedFile> saveToDownloads({
    required String tempPath,
    required String fileName,
    String? mime,
  }) async {
    final raw = await _channel.invokeMapMethod<String, Object?>(
      'saveToDownloads',
      {'tempPath': tempPath, 'fileName': fileName, 'mime': mime},
    );
    return SavedFile(
      uri: (raw?['uri'] ?? '').toString(),
      displayName: (raw?['displayName'] ?? fileName).toString(),
    );
  }

  @override
  Future<void> acquireMulticastLock() => _invoke('acquireMulticastLock');

  @override
  Future<void> releaseMulticastLock() => _invoke('releaseMulticastLock');

  @override
  Future<void> startForeground({required String title, required String text}) =>
      _invoke('startForeground', {'title': title, 'text': text});

  @override
  Future<void> stopForeground() => _invoke('stopForeground');

  @override
  Future<bool> requestNotificationPermission() async {
    try {
      final granted =
          await _channel.invokeMethod<bool>('requestNotificationPermission');
      return granted ?? true;
    } on MissingPluginException {
      return true;
    }
  }

  @override
  Future<void> cleanupCache(List<String> paths) =>
      _invoke('cleanupCache', {'paths': paths});

  /// 平台层缺失（单测 / 非 Android）时静默忽略，不阻塞 UI 流程。
  Future<void> _invoke(String method, [Map<String, Object?>? args]) async {
    try {
      await _channel.invokeMethod<void>(method, args);
    } on MissingPluginException {
      // ignore
    } on PlatformException catch (error) {
      debugPrint('互传平台调用 $method 失败: ${error.message}');
    }
  }
}

/// Rust 侧（FRB）抽象，便于单测替换。
abstract class TransferBackend {
  Future<void> start(TransferLaunchConfig config);

  Stream<TransferEvent> get events;

  String devicesJson();

  Future<void> refresh();

  Future<String?> addManualIp(String host, int port);

  Future<String> send(List<String> paths, String fingerprint, String? pin);

  Future<void> respondPrepare(String sessionId, List<String> acceptedFileIds);

  Future<void> respondFile(String sessionId, String fileId, String savePath);

  Future<void> reject(String sessionId);

  Future<void> cancel(String sessionId);

  Future<void> stop();
}

class FrbTransferBackend implements TransferBackend {
  rust.MobileTransfer? _transfer;
  final _events = StreamController<TransferEvent>.broadcast();
  StreamSubscription<String>? _subscription;

  @override
  Stream<TransferEvent> get events => _events.stream;

  @override
  Future<void> start(TransferLaunchConfig config) async {
    final transfer = await rust.start(
      config: rust.MobileTransferConfig(
        alias: config.alias,
        port: config.port,
        receiveDir: config.receiveDir,
        identityDir: config.identityDir,
        pin: config.pin,
        autoAcceptFavorites: config.autoAcceptFavorites,
      ),
    );
    _transfer = transfer;

    _subscription = transfer.subscribe().listen(
      (raw) {
        final event = parseTransferEvent(raw);
        if (event != null) _events.add(event);
      },
      onError: (Object error) => debugPrint('互传事件流异常: $error'),
    );
  }

  @override
  String devicesJson() => _transfer?.devicesJson() ?? '';

  @override
  Future<void> refresh() async {
    await _transfer?.refresh();
  }

  @override
  Future<String?> addManualIp(String host, int port) async =>
      _transfer?.addManualIp(host: host, port: port);

  @override
  Future<String> send(
    List<String> paths,
    String fingerprint,
    String? pin,
  ) =>
      _require().send(
        stagedPaths: paths,
        fingerprint: fingerprint,
        pin: pin,
      );

  @override
  Future<void> respondPrepare(String sessionId, List<String> acceptedFileIds) =>
      _require().respondPrepare(
        sessionId: sessionId,
        acceptedFileIds: acceptedFileIds,
      );

  @override
  Future<void> respondFile(String sessionId, String fileId, String savePath) =>
      _require().respondFile(
        sessionId: sessionId,
        fileId: fileId,
        savePath: savePath,
      );

  @override
  Future<void> reject(String sessionId) =>
      _require().reject(sessionId: sessionId);

  @override
  Future<void> cancel(String sessionId) =>
      _require().cancel(sessionId: sessionId);

  @override
  Future<void> stop() async {
    await _subscription?.cancel();
    _subscription = null;
    final transfer = _transfer;
    _transfer = null;
    await transfer?.stop();
  }

  rust.MobileTransfer _require() {
    final transfer = _transfer;
    if (transfer == null) {
      throw StateError('互传服务未启动');
    }
    return transfer;
  }
}

/// 一条传输的进度视图。
class TransferTask {
  const TransferTask({
    required this.sessionId,
    required this.fileId,
    required this.direction,
    this.percent = 0,
    this.transferred = 0,
    this.total = 0,
    this.fileName,
    this.done = false,
  });

  final String sessionId;
  final String fileId;
  final TransferDirection direction;
  final double percent;
  final int transferred;
  final int total;
  final String? fileName;
  final bool done;

  TransferTask copyWith({
    double? percent,
    int? transferred,
    int? total,
    String? fileName,
    bool? done,
  }) =>
      TransferTask(
        sessionId: sessionId,
        fileId: fileId,
        direction: direction,
        percent: percent ?? this.percent,
        transferred: transferred ?? this.transferred,
        total: total ?? this.total,
        fileName: fileName ?? this.fileName,
        done: done ?? this.done,
      );

  String get key => '$sessionId/$fileId';
}

/// 待用户确认的接收请求。
class IncomingRequest {
  const IncomingRequest({required this.event, required this.autoAccept});

  final TransferIncoming event;
  final bool autoAccept;

  String get sessionId => event.sessionId;
}

class TransferController extends ChangeNotifier {
  TransferController({
    required TransferBackend backend,
    required TransferPlatform platform,
    required TransferSettingsStore settingsStore,
    required TransferFavoritesStore favoritesStore,
    Future<String> Function()? cacheRoot,
    Future<String> Function()? identityRoot,
  })  : _backend = backend,
        _platform = platform,
        _settingsStore = settingsStore,
        _favorites = favoritesStore,
        _cacheRoot = cacheRoot ?? _defaultCacheRoot,
        _identityRoot = identityRoot ?? cacheRoot ?? _defaultCacheRoot;

  final TransferBackend _backend;
  final TransferPlatform _platform;
  final TransferSettingsStore _settingsStore;
  final TransferFavoritesStore _favorites;
  final Future<String> Function() _cacheRoot;
  final Future<String> Function() _identityRoot;

  StreamSubscription<TransferEvent>? _sub;

  /// 接收中的临时文件：`sessionId/fileId` → 文件名，用于落盘后改名。
  final Map<String, TransferFile> _tempFiles = {};

  /// 发送中的暂存文件：cache 路径 → 文件信息。
  /// core 的 `Completed` 只回传路径、没有文件名，靠它在完成时把「发送中」换成文件名。
  final Map<String, StagedFile> _stagedFiles = {};

  TransferRuntime runtime = TransferRuntime.stopped;
  TransferSettings settings = const TransferSettings();
  List<TransferDevice> devices = const [];
  List<IncomingRequest> incoming = const [];
  List<TransferTask> tasks = const [];
  String? lastError;

  /// 最近一次错误的契约错误码（`PIN_REQUIRED` 等），供 UI 决定是否弹 PIN。
  String? lastErrorCode;
  String? lastSavedFile;
  Set<String> favoriteFingerprints = <String>{};

  bool get isRunning => runtime == TransferRuntime.running;

  Future<void> loadSettings() async {
    settings = await _settingsStore.load();
    favoriteFingerprints = await _favorites.load();
    notifyListeners();
  }

  /// 启动互传：组播锁 + Rust 服务 + 事件流。
  Future<void> start() async {
    if (runtime != TransferRuntime.stopped) return;

    runtime = TransferRuntime.starting;
    lastError = null;
    notifyListeners();

    try {
      settings = await _settingsStore.load();
      favoriteFingerprints = await _favorites.load();

      await _platform.requestNotificationPermission();
      await _platform.acquireMulticastLock();
      await _backend.start(TransferLaunchConfig(
        alias: settings.alias,
        port: settings.port,
        receiveDir: await _inboxDir(),
        identityDir: await _identityDir(),
        pin: settings.pin,
        autoAcceptFavorites: settings.autoAcceptFavorites,
      ));

      _sub = _backend.events.listen(_onEvent);
      _applyDevicesSnapshot();
      runtime = TransferRuntime.running;
      // 会话式互传：开关打开期间常驻前台服务，后台也能收消息（Q30/Q38）。
      await _platform.startForeground(
        title: 'r-todo 互传',
        text: '正在监听消息（划掉 App 后不再接收）',
      );
    } catch (error) {
      runtime = TransferRuntime.stopped;
      lastError = _friendlyError(error);
      await _releaseResources();
    }
    notifyListeners();
  }

  Future<void> stop() async {
    if (runtime == TransferRuntime.stopped) return;

    // 先让 UI 立刻响应：串流取消 / Rust stop 都可能慢，不能卡住开关。
    runtime = TransferRuntime.stopped;
    devices = const [];
    tasks = const [];
    incoming = const [];
    _tempFiles.clear();
    _stagedFiles.clear();
    notifyListeners();

    final sub = _sub;
    _sub = null;
    try {
      await sub?.cancel().timeout(const Duration(seconds: 3));
    } catch (error) {
      debugPrint('取消事件订阅失败: $error');
    }

    try {
      await _backend.stop().timeout(const Duration(seconds: 5));
    } catch (error) {
      debugPrint('停止互传服务失败: $error');
    }

    await _releaseResources();
  }

  Future<void> refresh() async {
    if (!isRunning) return;
    try {
      await _backend.refresh();
      _applyDevicesSnapshot();
      lastError = null;
    } catch (error) {
      lastError = _friendlyError(error);
    }
    notifyListeners();
  }

  Future<void> addManualIp(String host, int port) async {
    if (!isRunning) return;
    try {
      final json = await _backend.addManualIp(host, port);
      if (json != null) {
        lastError = null;
        _applyDevicesSnapshot();
      } else {
        lastError = '没有在该地址找到设备，请确认对方已开启互传';
      }
    } catch (error) {
      lastError = _friendlyError(error);
    }
    notifyListeners();
  }

  /// 选择文件并发送给指定设备。
  Future<void> sendFiles(String fingerprint, {String? pin}) async {
    if (!isRunning) return;

    final files = await pickFiles();
    if (files.isEmpty) return;

    await sendStaged(files, fingerprint, pin: pin);
  }

  /// 只调起文件选择，不发送（PIN 重试时复用同一批文件）。
  Future<List<StagedFile>> pickFiles() async {
    if (!isRunning) return const [];
    return _platform.pickFiles();
  }

  /// 发送已暂存到 cache 的文件。
  Future<void> sendStaged(
    List<StagedFile> files,
    String fingerprint, {
    String? pin,
  }) async {
    if (files.isEmpty) return;

    final paths = files.map((file) => file.path).toList(growable: false);
    for (final file in files) {
      _stagedFiles[file.path] = file;
    }
    await _platform.startForeground(
      title: '正在发送',
      text: files.length == 1 ? files.first.name : '${files.length} 个文件',
    );

    try {
      await _backend.send(paths, fingerprint, pin);
      lastError = null;
      lastErrorCode = null;
    } catch (error) {
      final code = _errorCodeOf(error);
      lastErrorCode = code;
      lastError = _friendlyError(error);
      // PIN 校验失败时保留 cache 文件，UI 会拿同一批文件重试。
      if (code != 'PIN_REQUIRED' && code != 'PIN_INVALID') {
        await _platform.cleanupCache(paths);
        for (final path in paths) {
          _stagedFiles.remove(path);
        }
      }
      // 常驻模式：只把通知恢复成常驻文案，不停止服务。
      await _maybeStopForeground();
    }
    notifyListeners();
  }

  /// PIN 输入被取消 / 放弃重试时清掉暂存文件。
  Future<void> discardStaged(List<StagedFile> files) async {
    await _platform.cleanupCache(
      files.map((file) => file.path).toList(growable: false),
    );
    for (final file in files) {
      _stagedFiles.remove(file.path);
    }
    lastError = null;
    lastErrorCode = null;
    notifyListeners();
  }

  /// 对方要求 PIN 时，UI 弹一次输入框再重试。
  bool get pinRequired =>
      lastErrorCode == 'PIN_REQUIRED' || lastErrorCode == 'PIN_INVALID';

  /// 接受接收请求：prepare → 每个文件由 Rust 写入 cache → 再由 Kotlin 存到 Downloads。
  Future<void> acceptIncoming(IncomingRequest request) async {
    _removeIncoming(request.sessionId);
    final files = request.event.files;
    final inbox = await _inboxDir();

    for (final file in files) {
      _tempFiles['${request.sessionId}/${file.id}'] = file;
    }

    await _platform.startForeground(
      title: '正在接收',
      text: '来自 ${request.event.device.alias}',
    );

    try {
      await _backend.respondPrepare(
        request.sessionId,
        files.map((file) => file.id).toList(growable: false),
      );
      for (final file in files) {
        await _backend.respondFile(
          request.sessionId,
          file.id,
          '$inbox/${file.id}',
        );
      }
    } catch (error) {
      lastError = _friendlyError(error);
      await _maybeStopForeground();
    }
    notifyListeners();
  }

  Future<void> rejectIncoming(IncomingRequest request) async {
    _removeIncoming(request.sessionId);
    try {
      await _backend.reject(request.sessionId);
    } catch (error) {
      lastError = _friendlyError(error);
    }
    await _maybeStopForeground();
    notifyListeners();
  }

  Future<void> cancelSession(String sessionId) async {
    try {
      await _backend.cancel(sessionId);
    } catch (error) {
      lastError = _friendlyError(error);
    }
    notifyListeners();
  }

  Future<void> toggleFavorite(String fingerprint) async {
    favoriteFingerprints = await _favorites.toggle(fingerprint);
    notifyListeners();
  }

  bool isFavorite(String fingerprint) =>
      favoriteFingerprints.contains(fingerprint);

  Future<void> saveSettings(TransferSettings next) async {
    settings = next;
    await _settingsStore.save(next);
    notifyListeners();
  }

  void clearError() {
    lastError = null;
    lastErrorCode = null;
    lastSavedFile = null;
    notifyListeners();
  }

  Future<void> _onEvent(TransferEvent event) async {
    // FRB 事件流只有一个订阅者，聊天层通过广播总线复用。
    ChatEventBus.instance.add(event);
    switch (event) {
      case TransferDevices(:final devices):
        this.devices = devices;
      case TransferIncoming():
        final autoAccept = settings.autoAcceptFavorites &&
            favoriteFingerprints.contains(event.device.fingerprint);
        final request = IncomingRequest(event: event, autoAccept: autoAccept);
        incoming = [...incoming, request];
        if (autoAccept) {
          await acceptIncoming(request);
        } else {
          // 应用可能在后台：先挂个通知提醒用户回来确认。
          await _platform.startForeground(
            title: '收到文件',
            text: '来自 ${event.device.alias}，打开 r-todo 确认接收',
          );
        }
      case TransferProgress(
          :final sessionId,
          :final fileId,
          :final direction,
          :final percent,
          :final transferred,
          :final total,
        ):
        tasks = _upsertTask(
          sessionId,
          fileId,
          (task) => task.copyWith(
            percent: percent.clamp(0, 100).toDouble(),
            transferred: transferred,
            total: total,
          ),
          direction,
        );
      case TransferCompleted(
          :final sessionId,
          :final fileId,
          :final direction,
          :final path,
        ):
        // 发送侧 core 只在 Completed 里回传路径，文件名由我们自己暂存时映射。
        final completedName = direction == TransferDirection.send
            ? _takeStagedName(path)
            : _tempFiles['$sessionId/$fileId']?.fileName;
        tasks = _upsertTask(
          sessionId,
          fileId,
          (task) => task.copyWith(percent: 100, done: true),
          direction,
          fileName: completedName,
        );
        if (direction == TransferDirection.receive && path != null) {
          await _saveReceived(sessionId, fileId, path);
        }
        await _maybeStopForeground();
      case TransferError(:final code, :final message):
        lastErrorCode = code;
        lastError = transferErrorText(code, message: message);
        await _maybeStopForeground();
      case TransferSessionEnded(:final sessionId, :final reason):
        incoming = incoming
            .where((item) => item.sessionId != sessionId)
            .toList(growable: false);
        if (reason == TransferSessionEndReason.completed) {
          // 兜底：Completed 事件万一丢失/乱序，别让发送任务永远停在「发送中 100%」。
          tasks = tasks.map((task) {
            if (task.sessionId != sessionId ||
                task.done ||
                task.direction != TransferDirection.send) {
              return task;
            }
            return task.copyWith(
              percent: 100,
              done: true,
              fileName: _takeStagedNameBySize(task.total),
            );
          }).toList(growable: false);
        } else {
          tasks = tasks
              .where((task) => task.sessionId != sessionId || task.done)
              .toList(growable: false);
        }
        await _maybeStopForeground();
      case TransferMessage():
        // 会话消息由 ChatController 经 ChatEventBus 处理，这里不改传输态。
        break;
    }
    notifyListeners();
  }

  /// 接收完成后由 Kotlin 写入 `Downloads/r-todo`，再清掉 cache 副本。
  Future<void> _saveReceived(
    String sessionId,
    String fileId,
    String tempPath,
  ) async {
    final file = _tempFiles.remove('$sessionId/$fileId');
    try {
      final saved = await _platform.saveToDownloads(
        tempPath: tempPath,
        fileName: file?.fileName ?? tempPath.split('/').last,
        mime: file?.fileType,
      );
      lastSavedFile = saved.displayName;
    } catch (error) {
      lastError = '保存失败：$error';
    } finally {
      await _platform.cleanupCache([tempPath]);
    }
  }

  /// 取出（并消费）发送完成事件对应的原始文件名。
  String? _takeStagedName(String? path) {
    if (path == null) return null;

    final exact = _stagedFiles.remove(path);
    if (exact != null) return exact.name;

    // 路径风格/Canonicalize 不完全一致时，退而匹配 basename。
    final base = path.split(RegExp(r'[/\\]')).last;
    for (final entry in _stagedFiles.entries.toList()) {
      if (entry.key.split(RegExp(r'[/\\]')).last == base) {
        _stagedFiles.remove(entry.key);
        return entry.value.name;
      }
    }

    // 兜底：至少显示文件名而非「发送中」，并去掉 cache 的 uuid 前缀。
    final cleaned = base.replaceFirst(RegExp(r'^[0-9a-fA-F-]{36}-'), '');
    return cleaned.isEmpty ? base : cleaned;
  }

  /// 没有 Completed 事件时，按文件大小反查暂存文件名（SessionEnded 兜底用）。
  String? _takeStagedNameBySize(int total) {
    if (total <= 0) return null;
    for (final entry in _stagedFiles.entries.toList()) {
      if (entry.value.size == total) {
        _stagedFiles.remove(entry.key);
        return entry.value.name;
      }
    }
    return null;
  }

  List<TransferTask> _upsertTask(
    String sessionId,
    String fileId,
    TransferTask Function(TransferTask) update,
    TransferDirection direction, {
    String? fileName,
  }) {
    final key = '$sessionId/$fileId';
    final index = tasks.indexWhere((task) => task.key == key);
    if (index < 0) {
      final created = update(TransferTask(
        sessionId: sessionId,
        fileId: fileId,
        direction: direction,
        fileName: fileName ?? _tempFiles[key]?.fileName,
      ));
      return [...tasks, created];
    }
    final next = [...tasks];
    next[index] = update(next[index]);
    if (fileName != null && next[index].fileName == null) {
      next[index] = next[index].copyWith(fileName: fileName);
    }
    return next;
  }

  void _applyDevicesSnapshot() {
    final raw = _backend.devicesJson();
    if (raw.isEmpty) return;
    try {
      final event = parseTransferEvent(raw);
      if (event is TransferDevices) devices = event.devices;
    } on FormatException catch (error) {
      debugPrint('设备快照解析失败: $error');
    }
  }

  void _removeIncoming(String sessionId) {
    incoming = incoming
        .where((request) => request.sessionId != sessionId)
        .toList(growable: false);
  }

  /// 常驻模式下空闲时只把通知恢复成常驻文案，不停止前台服务；
  /// 只有 stop()/退出才会真正 stopForeground。
  Future<void> _maybeStopForeground() async {
    final busy = tasks.any((task) => !task.done) || incoming.isNotEmpty;
    if (!busy) {
      await _platform.startForeground(
        title: 'r-todo 互传',
        text: '正在监听消息（划掉 App 后不再接收）',
      );
    }
  }

  /// 会话消息到达时更新常驻通知，直接显示发送方与正文（Q45）。
  Future<void> showChatNotification(String title, String text) =>
      _platform.startForeground(title: title, text: text);

  /// 会话层直接发送已暂存的文件路径（发文件 / 转发用，不再弹文件选择器）。
  Future<void> sendPaths(String fingerprint, List<String> paths) async {
    if (paths.isEmpty) return;
    try {
      await _backend.send(paths, fingerprint, settings.pin);
      lastError = null;
    } catch (error) {
      lastError = _friendlyError(error);
      await _maybeStopForeground();
    }
    notifyListeners();
  }

  /// 证书目录：会话层用它构造 mTLS 客户端（Q7）。
  Future<String> identityDirectory() => _identityDir();

  Future<void> _releaseResources() async {
    try {
      await _platform
          .releaseMulticastLock()
          .timeout(const Duration(seconds: 3));
    } catch (error) {
      debugPrint('释放组播锁失败: $error');
    }
    try {
      await _platform.stopForeground().timeout(const Duration(seconds: 3));
    } catch (error) {
      debugPrint('停止前台服务失败: $error');
    }
  }

  Future<String> _baseDir() async => '${await _cacheRoot()}/transfer';

  Future<String> _inboxDir() async => '${await _baseDir()}/inbox';

  /// 证书/私钥放 App 私有文档目录，避免被系统清 cache 后 fingerprint 变化。
  Future<String> _identityDir() async =>
      '${await _identityRoot()}/transfer/identity';

  String _friendlyError(Object error) {
    final code = _errorCodeOf(error);
    if (code != null) return transferErrorText(code);
    if (error is StateError) return '操作失败：${error.message}';
    return '操作失败：$error';
  }

  String? _errorCodeOf(Object error) {
    final match =
        RegExp(r'"code":"([A-Z_]+)"').firstMatch(error.toString());
    return match?.group(1);
  }

  static Future<String> _defaultCacheRoot() async =>
      (await getTemporaryDirectory()).path;

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }
}

final transferBackendProvider = Provider<TransferBackend>((ref) {
  return FrbTransferBackend();
});

final transferPlatformProvider = Provider<TransferPlatform>((ref) {
  return MethodChannelTransferPlatform();
});

final transferSettingsStoreProvider =
    Provider<TransferSettingsStore>((ref) => TransferSettingsStore());

final transferFavoritesStoreProvider =
    Provider<TransferFavoritesStore>((ref) => TransferFavoritesStore());

final transferControllerProvider =
    ChangeNotifierProvider<TransferController>((ref) {
  final controller = TransferController(
    backend: ref.watch(transferBackendProvider),
    platform: ref.watch(transferPlatformProvider),
    settingsStore: ref.watch(transferSettingsStoreProvider),
    favoritesStore: ref.watch(transferFavoritesStoreProvider),
    identityRoot: () async => (await getApplicationDocumentsDirectory()).path,
  );
  controller.loadSettings();
  return controller;
});
