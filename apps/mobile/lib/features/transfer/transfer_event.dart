import 'dart:convert';

/// 与 `crates/transfer-core` 的 `SCHEMA_VERSION` 对齐。
const int transferSchemaVersion = 1;

enum TransferDirection { send, receive }

enum TransferProtocol { http, https }

enum TransferErrorScope { send, receive, discovery, server }

enum TransferSessionEndReason { completed, cancelled, rejected, error }

class TransferChannel {
  const TransferChannel({
    required this.host,
    required this.port,
    required this.protocol,
  });

  final String host;
  final int port;
  final TransferProtocol protocol;
}

class TransferDevice {
  const TransferDevice({
    required this.alias,
    required this.version,
    required this.fingerprint,
    required this.download,
    required this.channels,
    this.deviceModel,
    this.deviceType,
  });

  final String alias;
  final String version;
  final String fingerprint;
  final bool download;
  final List<TransferChannel> channels;
  final String? deviceModel;
  final String? deviceType;

  /// 发送时优先用 https 通道，其次第一个可用通道。
  TransferChannel? get preferredChannel {
    for (final channel in channels) {
      if (channel.protocol == TransferProtocol.https) return channel;
    }
    return channels.isEmpty ? null : channels.first;
  }
}

class TransferDeviceSummary {
  const TransferDeviceSummary({required this.alias, required this.fingerprint});

  final String alias;
  final String fingerprint;
}

class TransferFile {
  const TransferFile({
    required this.id,
    required this.fileName,
    required this.size,
    this.fileType,
    this.sha256,
  });

  final String id;
  final String fileName;
  final int size;
  final String? fileType;
  final String? sha256;
}

/// 平台无关事件。字段语义与 Rust `TransferEvent` 一一对应。
sealed class TransferEvent {
  const TransferEvent(this.schemaVersion);

  final int schemaVersion;
}

class TransferDevices extends TransferEvent {
  const TransferDevices(super.schemaVersion, this.devices);

  final List<TransferDevice> devices;
}

class TransferIncoming extends TransferEvent {
  const TransferIncoming(
    super.schemaVersion, {
    required this.sessionId,
    required this.ip,
    required this.device,
    required this.files,
  });

  final String sessionId;
  final String ip;
  final TransferDeviceSummary device;
  final List<TransferFile> files;

  int get totalBytes => files.fold(0, (sum, file) => sum + file.size);
}

class TransferProgress extends TransferEvent {
  const TransferProgress(
    super.schemaVersion, {
    required this.sessionId,
    required this.fileId,
    required this.direction,
    required this.transferred,
    required this.total,
    required this.percent,
  });

  final String sessionId;
  final String fileId;
  final TransferDirection direction;
  final int transferred;
  final int total;
  final double percent;
}

class TransferCompleted extends TransferEvent {
  const TransferCompleted(
    super.schemaVersion, {
    required this.sessionId,
    required this.fileId,
    required this.direction,
    this.path,
  });

  final String sessionId;
  final String fileId;
  final TransferDirection direction;
  final String? path;
}

class TransferError extends TransferEvent {
  const TransferError(
    super.schemaVersion, {
    required this.scope,
    required this.code,
    required this.message,
  });

  final TransferErrorScope scope;

  /// 与 core `ErrorCode` 的字符串形式一致（如 `PIN_INVALID`）。
  final String code;
  final String message;
}

class TransferSessionEnded extends TransferEvent {
  const TransferSessionEnded(
    super.schemaVersion, {
    required this.sessionId,
    required this.reason,
  });

  final String sessionId;
  final TransferSessionEndReason reason;
}

/// 解析 core 的 `EventEnvelope` JSON。
///
/// - 未知 `type` 返回 null（协议前向兼容，调用方忽略即可）。
/// - `schema_version` 不匹配或 JSON 非法抛 [FormatException]。
TransferEvent? parseTransferEvent(String raw) {
  final decoded = jsonDecode(raw);
  if (decoded is! Map) {
    throw const FormatException('互传事件不是 JSON 对象');
  }
  final map = decoded.cast<String, dynamic>();

  final version = map['schema_version'];
  if (version != transferSchemaVersion) {
    throw FormatException('不支持的互传协议版本: $version');
  }

  switch (map['type']) {
    case 'devices':
      return TransferDevices(
        transferSchemaVersion,
        _list(map['devices'], _parseDevice, 'devices'),
      );
    case 'incoming':
      return TransferIncoming(
        transferSchemaVersion,
        sessionId: _string(map['session_id'], 'session_id'),
        ip: _string(map['ip'], 'ip'),
        device: _parseSummary(_map(map['device'], 'device')),
        files: _list(map['files'], _parseFile, 'files'),
      );
    case 'progress':
      return TransferProgress(
        transferSchemaVersion,
        sessionId: _string(map['session_id'], 'session_id'),
        fileId: _string(map['file_id'], 'file_id'),
        direction: _direction(map['direction']),
        transferred: _int(map['transferred'], 'transferred'),
        total: _int(map['total'], 'total'),
        percent: _double(map['percent'], 'percent'),
      );
    case 'completed':
      return TransferCompleted(
        transferSchemaVersion,
        sessionId: _string(map['session_id'], 'session_id'),
        fileId: _string(map['file_id'], 'file_id'),
        direction: _direction(map['direction']),
        path: _optionalString(map['path']),
      );
    case 'error':
      return TransferError(
        transferSchemaVersion,
        scope: _scope(map['scope']),
        code: _string(map['code'], 'code'),
        message: _string(map['message'], 'message'),
      );
    case 'session_ended':
      return TransferSessionEnded(
        transferSchemaVersion,
        sessionId: _string(map['session_id'], 'session_id'),
        reason: _reason(map['reason']),
      );
    default:
      return null;
  }
}

TransferDevice _parseDevice(Object? value) {
  final map = _map(value, 'device');
  return TransferDevice(
    alias: _string(map['alias'], 'alias'),
    version: _string(map['version'], 'version'),
    fingerprint: _string(map['fingerprint'], 'fingerprint'),
    download: map['download'] == true,
    deviceModel: _optionalString(map['device_model']),
    deviceType: _optionalString(map['device_type']),
    channels: _list(map['channels'], (raw) {
      final channel = _map(raw, 'channel');
      return TransferChannel(
        host: _string(channel['host'], 'host'),
        port: _int(channel['port'], 'port'),
        protocol: channel['protocol'] == 'http'
            ? TransferProtocol.http
            : TransferProtocol.https,
      );
    }, 'channels'),
  );
}

TransferDeviceSummary _parseSummary(Map<String, dynamic> map) =>
    TransferDeviceSummary(
      alias: _string(map['alias'], 'alias'),
      fingerprint: _string(map['fingerprint'], 'fingerprint'),
    );

TransferFile _parseFile(Object? value) {
  final map = _map(value, 'file');
  return TransferFile(
    id: _string(map['id'], 'id'),
    fileName: _string(map['file_name'], 'file_name'),
    size: _int(map['size'], 'size'),
    fileType: _optionalString(map['file_type']),
    sha256: _optionalString(map['sha256']),
  );
}

Map<String, dynamic> _map(Object? value, String field) {
  if (value is Map) return value.cast<String, dynamic>();
  throw FormatException('互传事件字段 $field 应为对象');
}

String _string(Object? value, String field) {
  if (value is String) return value;
  throw FormatException('互传事件字段 $field 应为字符串');
}

String? _optionalString(Object? value) => value is String ? value : null;

int _int(Object? value, String field) {
  if (value is int) return value;
  throw FormatException('互传事件字段 $field 应为整数');
}

double _double(Object? value, String field) {
  if (value is num) return value.toDouble();
  throw FormatException('互传事件字段 $field 应为数字');
}

List<T> _list<T>(Object? value, T Function(Object?) parse, String field) {
  if (value is List) return value.map(parse).toList(growable: false);
  throw FormatException('互传事件字段 $field 应为数组');
}

TransferDirection _direction(Object? value) => switch (value) {
      'receive' => TransferDirection.receive,
      'send' => TransferDirection.send,
      _ => throw FormatException('未知传输方向: $value'),
    };

TransferErrorScope _scope(Object? value) => switch (value) {
      'send' => TransferErrorScope.send,
      'receive' => TransferErrorScope.receive,
      'discovery' => TransferErrorScope.discovery,
      'server' => TransferErrorScope.server,
      _ => throw FormatException('未知错误范围: $value'),
    };

TransferSessionEndReason _reason(Object? value) => switch (value) {
      'completed' => TransferSessionEndReason.completed,
      'cancelled' => TransferSessionEndReason.cancelled,
      'rejected' => TransferSessionEndReason.rejected,
      'error' => TransferSessionEndReason.error,
      _ => throw FormatException('未知会话结束原因: $value'),
    };
