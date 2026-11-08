import 'package:shared_preferences/shared_preferences.dart';

/// 收藏的设备（按 fingerprint）。当前只用于「自动接受」判断与列表标记。
class TransferFavoritesStore {
  static const String _key = 'transfer.favorites';

  Future<Set<String>> load() async {
    final prefs = await SharedPreferences.getInstance();
    return (prefs.getStringList(_key) ?? const <String>[]).toSet();
  }

  Future<Set<String>> toggle(String fingerprint) async {
    final prefs = await SharedPreferences.getInstance();
    final current = (prefs.getStringList(_key) ?? const <String>[]).toSet();

    if (!current.remove(fingerprint)) {
      current.add(fingerprint);
    }

    await prefs.setStringList(_key, current.toList(growable: false));
    return current;
  }
}
