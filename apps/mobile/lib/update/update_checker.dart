import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';

/// 从 GitHub release 获取的最新版本信息
class UpdateInfo {
  const UpdateInfo({
    required this.version,
    required this.url,
    required this.notes,
  });

  final String version;
  final String url;
  final String notes;

  /// 解析 GitHub `releases/latest` API 响应。
  /// 版本取 tag_name（去 `v` 前缀），下载地址取 assets 里第一个 .apk。
  factory UpdateInfo.fromReleaseJson(Map<String, dynamic> json) {
    final tag = json['tag_name'] as String? ?? '';
    final version = tag.startsWith('v') ? tag.substring(1) : tag;
    var url = '';
    final assets = json['assets'] as List? ?? const [];
    for (final a in assets) {
      if (a is Map<String, dynamic>) {
        final name = a['name'] as String? ?? '';
        if (name.endsWith('.apk')) {
          url = a['browser_download_url'] as String? ?? '';
          break;
        }
      }
    }
    return UpdateInfo(version: version, url: url, notes: '');
  }
}

/// 语义版本比较（如 "0.0.6" vs "0.0.5"）：remote 更新返回 true。
bool isNewerVersion(String remote, String local) {
  final r = remote.split('.').map(int.tryParse).toList();
  final l = local.split('.').map(int.tryParse).toList();
  final n = r.length > l.length ? r.length : l.length;
  for (var i = 0; i < n; i++) {
    final rv = i < r.length ? r[i] ?? 0 : 0;
    final lv = i < l.length ? l[i] ?? 0 : 0;
    if (rv != lv) return rv > lv;
  }
  return false;
}

/// 检测更新 + 下载 APK。
///
/// 版本源：GitHub 仓库最新 release（公开仓库匿名可访问，发版即自动生效）。
/// 安装由原生侧完成（MainActivity MethodChannel `r_todo/install`）。
class UpdateChecker {
  UpdateChecker._();

  static const apiUrl =
      'https://api.github.com/repos/1474282816/r-todo/releases/latest';

  /// 拉取远端最新版本信息；失败或没有 APK 资产返回 null（静默，不打扰用户）。
  static Future<UpdateInfo?> fetch() async {
    try {
      final resp = await http
          .get(
            Uri.parse(apiUrl),
            headers: const {
              'Accept': 'application/vnd.github+json',
              'User-Agent': 'rtools-mobile-updater',
            },
          )
          .timeout(const Duration(seconds: 10));
      if (resp.statusCode != 200) return null;
      final json = jsonDecode(utf8.decode(resp.bodyBytes));
      if (json is! Map<String, dynamic>) return null;
      final info = UpdateInfo.fromReleaseJson(json);
      if (info.url.isEmpty) return null; // 最新 release 没有 APK（如 Windows 版）
      return info;
    } catch (_) {
      return null;
    }
  }

  /// 下载 APK 到应用内部缓存目录（无需存储权限），返回文件路径。
  /// [onProgress] 回调下载进度 0.0 ~ 1.0。
  static Future<String> download(
    String url, {
    void Function(double progress)? onProgress,
  }) async {
    final dir = await getTemporaryDirectory();
    final file = File('${dir.path}/rtools-update.apk');
    final request = http.Request('GET', Uri.parse(url));
    final client = http.Client();
    final streamed = await client.send(request);
    if (streamed.statusCode != 200) {
      throw HttpException('下载失败: HTTP ${streamed.statusCode}');
    }
    final total = streamed.contentLength ?? 0;
    var received = 0;
    final sink = file.openWrite();
    await for (final chunk in streamed.stream) {
      received += chunk.length;
      sink.add(chunk);
      if (total > 0 && onProgress != null) {
        onProgress(received / total);
      }
    }
    await sink.close();
    client.close();
    return file.path;
  }
}
