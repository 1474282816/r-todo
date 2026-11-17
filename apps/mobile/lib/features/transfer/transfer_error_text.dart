/// 互传错误码 → 用户可读中文文案。
///
/// 错误码来自 `crates/transfer-core` 的 `ErrorCode`，不要新增/改名。
String transferErrorText(String code, {String? message}) {
  switch (code) {
    case 'PORT_IN_USE':
      return '端口 54417 已被占用。可能官方 LocalSend 正在运行，或换一个端口再试。';
    case 'FIREWALL':
      return '系统防火墙拦截了连接。请在防火墙设置中允许 rtools 访问局域网。';
    case 'MULTICAST_UNAVAILABLE':
      return '当前网络收不到局域网广播。请检查 Wi-Fi 是否开启了 AP 隔离，或改用手动 IP 连接。';
    case 'TLS':
      return '安全连接建立失败。请确认两端应用版本一致后重试。';
    case 'PIN_REQUIRED':
      return '对方要求输入 PIN 校验码。';
    case 'PIN_INVALID':
      return 'PIN 校验码不正确，请重新输入。';
    case 'REJECTED':
      return '对方拒绝了本次传输。';
    case 'CHECKSUM_MISMATCH':
      return '文件校验失败，传输过程中数据损坏，请重试。';
    case 'CANCELLED':
      return '传输已取消。';
    case 'IO':
      return '读写文件失败。请检查存储空间与文件权限后重试。';
    default:
      if (message != null && message.trim().isNotEmpty) {
        return '传输出错：${message.trim()}';
      }
      return '传输出错，请重试。';
  }
}
