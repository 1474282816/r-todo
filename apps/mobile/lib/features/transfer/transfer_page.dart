import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';
import 'package:gap/gap.dart';

import 'transfer_event.dart';
import 'transfer_service.dart';

/// 「互传」Tab 内容：开关、设备列表、发送进度、接收确认。
///
/// 外层脚手架（header / 底部 Tab）由 `RootShell` 统一提供。
class TransferView extends ConsumerStatefulWidget {
  const TransferView({super.key});

  @override
  ConsumerState<TransferView> createState() => _TransferViewState();
}

class _TransferViewState extends ConsumerState<TransferView> {
  bool _dialogOpen = false;

  @override
  Widget build(BuildContext context) {
    final controller = ref.watch(transferControllerProvider);
    final colors = context.theme.colors;

    ref.listen<TransferController>(transferControllerProvider, (previous, next) {
      _maybeShowIncoming(next);
    });
    // 首帧可能已有请求（页面重建）
    WidgetsBinding.instance.addPostFrameCallback((_) => _maybeShowIncoming(controller));

    return ListView(
      padding: const EdgeInsets.fromLTRB(16, 4, 16, 32),
        children: [
          _StatusCard(controller: controller),
          if (controller.lastError != null) ...[
            const Gap(12),
            _ErrorBanner(
              message: controller.lastError!,
              onDismiss: controller.clearError,
            ),
          ],
          if (controller.lastSavedFile != null) ...[
            const Gap(12),
            _SavedBanner(
              fileName: controller.lastSavedFile!,
              onDismiss: controller.clearError,
            ),
          ],
          const Gap(20),
          Row(
            children: [
              Text(
                '附近的设备',
                style: TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w600,
                  color: colors.foreground,
                ),
              ),
              const Gap(6),
              if (controller.devices.isNotEmpty)
                Text(
                  '${controller.devices.length}',
                  style: const TextStyle(fontSize: 12, color: Color(0xFF9CA3AF)),
                ),
              const Spacer(),
              if (controller.isRunning) ...[
                _IconAction(
                  icon: Icons.refresh,
                  tooltip: '刷新',
                  onTap: controller.refresh,
                ),
                _IconAction(
                  icon: Icons.add_link,
                  tooltip: '手动 IP',
                  onTap: () => _showManualIpDialog(context, controller),
                ),
              ],
            ],
          ),
          const Gap(8),
          if (!controller.isRunning)
            const _HintBox(text: '打开互传后，会自动发现同一 Wi-Fi 下的设备。')
          else if (controller.devices.isEmpty)
            const _HintBox(
              text: '还没有发现设备。\n确认对方也打开了互传、且在同一 Wi-Fi（访客网络/AP 隔离会拦截广播），或用「手动 IP」连接。',
            )
          else
            for (final device in controller.devices)
              _DeviceRow(
                device: device,
                favorite: controller.isFavorite(device.fingerprint),
                onFavorite: () => controller.toggleFavorite(device.fingerprint),
                onSend: () => _pickPinAndSend(controller, device),
              ),
          const Gap(20),
          if (controller.tasks.isNotEmpty) ...[
            Text(
              '传输中',
              style: TextStyle(
                fontSize: 13,
                fontWeight: FontWeight.w600,
                color: colors.foreground,
              ),
            ),
            const Gap(8),
            for (final task in controller.tasks)
              _TaskRow(
                task: task,
                onCancel: task.done
                    ? null
                    : () => controller.cancelSession(task.sessionId),
              ),
            const Gap(16),
          ],
          Text(
            '接收的文件保存在「下载/rtools」，传输期间会显示通知。',
            style: TextStyle(
              fontSize: 11,
              height: 1.5,
              color: colors.mutedForeground,
            ),
          ),
      ],
    );
  }

  Future<void> _maybeShowIncoming(TransferController controller) async {
    if (_dialogOpen || !mounted) return;
    if (controller.incoming.isEmpty) return;
    if (controller.incoming.any((request) => request.autoAccept)) return;

    final request = controller.incoming.first;
    _dialogOpen = true;
    final accept = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) => _IncomingDialog(request: request),
    );
    _dialogOpen = false;

    if (!mounted) return;
    if (accept == true) {
      await controller.acceptIncoming(request);
    } else {
      await controller.rejectIncoming(request);
    }
    // 可能还有排队的请求
    if (mounted) _maybeShowIncoming(controller);
  }

  Future<void> _showManualIpDialog(
    BuildContext context,
    TransferController controller,
  ) async {
    final input = await showDialog<_ManualIpInput>(
      context: context,
      builder: (_) => const _ManualIpDialog(),
    );
    if (input == null || input.host.isEmpty) return;

    await controller.addManualIp(input.host, input.port);
  }

  Future<void> _pickPinAndSend(
    TransferController controller,
    TransferDevice device,
  ) async {
    final files = await controller.pickFiles();
    if (files.isEmpty || !mounted) return;

    await controller.sendStaged(files, device.fingerprint);

    // 对方设了 PIN：Rust 回 PIN_REQUIRED / PIN_INVALID，弹一次输入后复用同一批文件重试。
    if (!controller.pinRequired || !mounted) return;
    final pin = await showDialog<String>(
      context: context,
      builder: (_) => const _PinDialog(),
    );
    if (pin == null || pin.isEmpty) {
      await controller.discardStaged(files);
      return;
    }
    if (!mounted) return;

    await controller.sendStaged(files, device.fingerprint, pin: pin);
    if (!mounted) return;
    if (controller.pinRequired) {
      // 重试仍失败（PIN 错/对方取消），别把文件留在 cache 里。
      await controller.discardStaged(files);
    }
  }
}

class _StatusCard extends StatelessWidget {
  const _StatusCard({required this.controller});

  final TransferController controller;

  @override
  Widget build(BuildContext context) {
    final colors = context.theme.colors;
    final status = switch (controller.runtime) {
      TransferRuntime.stopped => '未开启',
      TransferRuntime.starting => '正在启动…',
      TransferRuntime.running => '已开启',
    };

    return Container(
      padding: const EdgeInsets.fromLTRB(14, 12, 8, 12),
      decoration: BoxDecoration(
        color: const Color(0x0D000000),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(
                      controller.isRunning
                          ? Icons.wifi_tethering
                          : Icons.wifi_tethering_off,
                      size: 15,
                      color: controller.isRunning
                          ? const Color(0xFF22C55E)
                          : const Color(0xFF9CA3AF),
                    ),
                    const Gap(6),
                    Text(
                      status,
                      style: TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: colors.foreground,
                      ),
                    ),
                  ],
                ),
                const Gap(4),
                Text(
                  '${controller.settings.alias} · 端口 ${controller.settings.port}',
                  style: const TextStyle(fontSize: 11, color: Color(0xFF9CA3AF)),
                ),
              ],
            ),
          ),
          FSwitch(
            value: controller.isRunning,
            enabled: controller.runtime != TransferRuntime.starting,
            onChange: (value) => value
                ? controller.start()
                : controller.stop(),
          ),
        ],
      ),
    );
  }
}

class _DeviceRow extends StatelessWidget {
  const _DeviceRow({
    required this.device,
    required this.favorite,
    required this.onFavorite,
    required this.onSend,
  });

  final TransferDevice device;
  final bool favorite;
  final VoidCallback onFavorite;
  final VoidCallback onSend;

  @override
  Widget build(BuildContext context) {
    final colors = context.theme.colors;
    final icon = switch (device.deviceType) {
      'desktop' => Icons.computer_outlined,
      'mobile' => Icons.smartphone_outlined,
      _ => Icons.devices_other_outlined,
    };

    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: onSend,
          borderRadius: BorderRadius.circular(8),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 10),
            child: Row(
              children: [
                Icon(icon, size: 18, color: colors.mutedForeground),
                const Gap(10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        device.alias,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(fontSize: 14, color: colors.foreground),
                      ),
                      const Gap(2),
                      Text(
                        [
                          if (device.deviceModel != null) device.deviceModel!,
                          device.fingerprint.length > 8
                              ? device.fingerprint.substring(0, 8)
                              : device.fingerprint,
                        ].join(' · '),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 11,
                          color: Color(0xFF9CA3AF),
                        ),
                      ),
                    ],
                  ),
                ),
                _IconAction(
                  icon: favorite ? Icons.star : Icons.star_border,
                  tooltip: favorite ? '取消收藏' : '收藏（可自动接收）',
                  color: favorite ? const Color(0xFFF59E0B) : null,
                  onTap: onFavorite,
                ),
                _IconAction(
                  icon: Icons.upload_file_outlined,
                  tooltip: '发送文件',
                  onTap: onSend,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _TaskRow extends StatelessWidget {
  const _TaskRow({required this.task, this.onCancel});

  final TransferTask task;
  final VoidCallback? onCancel;

  @override
  Widget build(BuildContext context) {
    final colors = context.theme.colors;
    final label = task.direction == TransferDirection.send ? '发送' : '接收';
    final name = task.fileName ?? (task.done ? '$label完成' : '$label中');

    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                task.done ? Icons.check_circle_outline : Icons.swap_vert,
                size: 14,
                color: task.done
                    ? const Color(0xFF22C55E)
                    : colors.mutedForeground,
              ),
              const Gap(6),
              Expanded(
                child: Text(
                  name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(fontSize: 12, color: colors.foreground),
                ),
              ),
              Text(
                task.done ? '已完成' : '${task.percent.toStringAsFixed(0)}%',
                style: TextStyle(
                  fontSize: 11,
                  color: task.done
                      ? const Color(0xFF22C55E)
                      : const Color(0xFF9CA3AF),
                ),
              ),
              if (onCancel != null)
                _IconAction(
                  icon: Icons.close,
                  tooltip: '取消',
                  onTap: onCancel!,
                ),
            ],
          ),
          const Gap(4),
          ClipRRect(
            borderRadius: BorderRadius.circular(2),
            child: LinearProgressIndicator(
              value: task.percent / 100,
              minHeight: 3,
              backgroundColor: const Color(0x14000000),
              valueColor: AlwaysStoppedAnimation<Color>(
                task.done ? const Color(0xFF22C55E) : colors.primary,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _IconAction extends StatelessWidget {
  const _IconAction({
    required this.icon,
    required this.tooltip,
    required this.onTap,
    this.color,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback onTap;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    return IconButton(
      icon: Icon(icon, size: 18),
      color: color ?? context.theme.colors.mutedForeground,
      tooltip: tooltip,
      visualDensity: VisualDensity.compact,
      onPressed: onTap,
    );
  }
}

class _HintBox extends StatelessWidget {
  const _HintBox({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: const Color(0x0A000000),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Text(
        text,
        style: const TextStyle(
          fontSize: 11,
          height: 1.6,
          color: Color(0xFF9CA3AF),
        ),
      ),
    );
  }
}

class _ErrorBanner extends StatelessWidget {
  const _ErrorBanner({required this.message, required this.onDismiss});

  final String message;
  final VoidCallback onDismiss;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.fromLTRB(12, 8, 4, 8),
      decoration: BoxDecoration(
        color: const Color(0x14EF4444),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        children: [
          const Icon(Icons.error_outline, size: 14, color: Color(0xFFEF4444)),
          const Gap(8),
          Expanded(
            child: Text(
              message,
              style: const TextStyle(
                fontSize: 11,
                height: 1.5,
                color: Color(0xFFEF4444),
              ),
            ),
          ),
          _IconAction(
            icon: Icons.close,
            tooltip: '关闭',
            onTap: onDismiss,
          ),
        ],
      ),
    );
  }
}

class _SavedBanner extends StatelessWidget {
  const _SavedBanner({required this.fileName, required this.onDismiss});

  final String fileName;
  final VoidCallback onDismiss;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.fromLTRB(12, 8, 4, 8),
      decoration: BoxDecoration(
        color: const Color(0x1422C55E),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        children: [
          const Icon(Icons.download_done, size: 14, color: Color(0xFF22C55E)),
          const Gap(8),
          Expanded(
            child: Text(
              '已保存到「下载/rtools/$fileName」',
              style: const TextStyle(
                fontSize: 11,
                height: 1.5,
                color: Color(0xFF22C55E),
              ),
            ),
          ),
          _IconAction(icon: Icons.close, tooltip: '关闭', onTap: onDismiss),
        ],
      ),
    );
  }
}

class _IncomingDialog extends StatelessWidget {
  const _IncomingDialog({required this.request});

  final IncomingRequest request;

  @override
  Widget build(BuildContext context) {
    final event = request.event;
    return AlertDialog(
      title: const Text('收到文件'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('来自 ${event.device.alias}（${event.ip}）',
              style: const TextStyle(fontSize: 13)),
          const Gap(8),
          for (final file in event.files.take(5))
            Text(
              '· ${file.fileName}  ${_formatSize(file.size)}',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 12, color: Color(0xFF6B7280)),
            ),
          if (event.files.length > 5)
            Text(
              '…共 ${event.files.length} 个文件',
              style: const TextStyle(fontSize: 12, color: Color(0xFF9CA3AF)),
            ),
          const Gap(10),
          Text(
            '共 ${_formatSize(event.totalBytes)}，接收后保存到「下载/rtools」。',
            style: const TextStyle(fontSize: 11, color: Color(0xFF9CA3AF)),
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: const Text('拒绝'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, true),
          child: const Text('接收'),
        ),
      ],
    );
  }
}

class _ManualIpInput {
  const _ManualIpInput({required this.host, required this.port});

  final String host;
  final int port;
}

/// 控制器必须由弹窗自己持有：外层在 `showDialog` 返回时立刻 dispose 会撞上
/// 关闭动画期间的 `_dependents.isEmpty` 断言（真机复现过红屏）。
class _ManualIpDialog extends StatefulWidget {
  const _ManualIpDialog();

  @override
  State<_ManualIpDialog> createState() => _ManualIpDialogState();
}

class _ManualIpDialogState extends State<_ManualIpDialog> {
  final _hostController = TextEditingController();
  final _portController = TextEditingController(text: '54417');

  @override
  void dispose() {
    _hostController.dispose();
    _portController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('手动连接'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          FTextField(
            control: FTextFieldControl.managed(controller: _hostController),
            hint: '对方 IP，如 192.168.1.5',
          ),
          const Gap(8),
          FTextField(
            control: FTextFieldControl.managed(controller: _portController),
            hint: '端口（默认 54417）',
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('取消'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(
            context,
            _ManualIpInput(
              host: _hostController.text.trim(),
              port: int.tryParse(_portController.text.trim()) ?? 54417,
            ),
          ),
          child: const Text('连接'),
        ),
      ],
    );
  }
}

class _PinDialog extends StatefulWidget {
  const _PinDialog();

  @override
  State<_PinDialog> createState() => _PinDialogState();
}

class _PinDialogState extends State<_PinDialog> {
  final _controller = TextEditingController();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('需要校验码'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Text(
            '对方的互传设置了 PIN，请输入后重试。',
            style: TextStyle(fontSize: 12),
          ),
          const Gap(10),
          FTextField(
            control: FTextFieldControl.managed(controller: _controller),
            hint: 'PIN',
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('取消'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, _controller.text.trim()),
          child: const Text('发送'),
        ),
      ],
    );
  }
}

String _formatSize(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
  if (bytes < 1024 * 1024 * 1024) {
    return '${(bytes / 1024 / 1024).toStringAsFixed(1)} MB';
  }
  return '${(bytes / 1024 / 1024 / 1024).toStringAsFixed(2)} GB';
}
