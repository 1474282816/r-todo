import 'package:shared_preferences/shared_preferences.dart';

/// 互传设置。持久化在 Flutter 侧（shared_preferences），启动时传给 Rust。
class TransferSettings {
  const TransferSettings({
    this.alias = defaultAlias,
    this.port = defaultPort,
    this.pin,
    // Q36：改叫「已确认设备自动收文件」，默认开；确认一次后后续免确认。
    this.autoAcceptFavorites = true,
  });

  static const String defaultAlias = 'rtools';
  static const int defaultPort = 53317;
  static const String defaultReceiveDirName = 'rtools';

  final String alias;
  final int port;
  final String? pin;
  final bool autoAcceptFavorites;

  /// 接收目录名（Android 由 Kotlin 落到 Downloads/<name>，Rust 只做展示）。
  String get receiveDirName => defaultReceiveDirName;

  TransferSettings copyWith({
    String? alias,
    int? port,
    String? pin,
    bool clearPin = false,
    bool? autoAcceptFavorites,
  }) {
    return TransferSettings(
      alias: alias ?? this.alias,
      port: port ?? this.port,
      pin: clearPin ? null : (pin ?? this.pin),
      autoAcceptFavorites: autoAcceptFavorites ?? this.autoAcceptFavorites,
    );
  }
}

class TransferSettingsStore {
  static const String _keyAlias = 'transfer.alias';
  static const String _keyPort = 'transfer.port';
  static const String _keyPin = 'transfer.pin';
  static const String _keyAutoAccept = 'transfer.auto_accept_favorites';

  Future<TransferSettings> load() async {
    final prefs = await SharedPreferences.getInstance();

    final alias = prefs.getString(_keyAlias)?.trim();
    final port = prefs.getInt(_keyPort);
    final pin = prefs.getString(_keyPin)?.trim();

    return TransferSettings(
      alias: (alias == null || alias.isEmpty)
          ? TransferSettings.defaultAlias
          : alias,
      port: (port == null || port <= 0 || port > 65535)
          ? TransferSettings.defaultPort
          : port,
      pin: (pin == null || pin.isEmpty) ? null : pin,
      autoAcceptFavorites: prefs.getBool(_keyAutoAccept) ?? true,
    );
  }

  Future<void> save(TransferSettings settings) async {
    final prefs = await SharedPreferences.getInstance();
    final alias = settings.alias.trim();

    await prefs.setString(
      _keyAlias,
      alias.isEmpty ? TransferSettings.defaultAlias : alias,
    );
    await prefs.setInt(
      _keyPort,
      settings.port <= 0 ? TransferSettings.defaultPort : settings.port,
    );
    await prefs.setBool(_keyAutoAccept, settings.autoAcceptFavorites);

    final pin = settings.pin?.trim();
    if (pin == null || pin.isEmpty) {
      await prefs.remove(_keyPin);
    } else {
      await prefs.setString(_keyPin, pin);
    }
  }
}
