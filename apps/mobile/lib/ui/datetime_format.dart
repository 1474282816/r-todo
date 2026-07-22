/// 时间格式化工具（与桌面端 formatDateTime 对齐）
library;

String _pad(int n) => n.toString().padLeft(2, '0');

/// 独立时间戳统一格式：YYYY-MM-DD HH:mm:ss（本地时区）
String formatDateTime(String? iso) {
  if (iso == null || iso.isEmpty) return '';
  final d = DateTime.tryParse(iso)?.toLocal();
  if (d == null) return '';
  return '${d.year}-${_pad(d.month)}-${_pad(d.day)} '
      '${_pad(d.hour)}:${_pad(d.minute)}:${_pad(d.second)}';
}

/// 日期上下文里的时间（月视图等已知日期的场景）：HH:mm:ss
String formatTime(String? iso) {
  if (iso == null || iso.isEmpty) return '';
  final d = DateTime.tryParse(iso)?.toLocal();
  if (d == null) return '';
  return '${_pad(d.hour)}:${_pad(d.minute)}:${_pad(d.second)}';
}
