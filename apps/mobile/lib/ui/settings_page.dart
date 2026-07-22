import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';
import 'package:gap/gap.dart';

import '../state/task_store.dart';
import '../sync/sync_service.dart';
import 'datetime_format.dart';

class SettingsPage extends ConsumerStatefulWidget {
  const SettingsPage({super.key});

  @override
  ConsumerState<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends ConsumerState<SettingsPage> {
  final _urlController = TextEditingController(text: 'http://192.168.10.10:8090');
  final _userController = TextEditingController();
  final _pwdController = TextEditingController();
  bool _busy = false;
  String? _error;

  final _sync = SyncService.instance;

  @override
  void initState() {
    super.initState();
    _sync.statusStream.listen((_) {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _urlController.dispose();
    _userController.dispose();
    _pwdController.dispose();
    super.dispose();
  }

  Future<void> _login() async {
    if (_urlController.text.isEmpty ||
        _userController.text.isEmpty ||
        _pwdController.text.isEmpty) {
      setState(() => _error = '请填写 URL、用户名、密码');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await _sync.login(
        _urlController.text.trim(),
        _userController.text.trim(),
        _pwdController.text,
      );
      await ref.read(taskListProvider.notifier).refreshFromDb();
      _pwdController.clear();
    } catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.theme.colors;
    return FScaffold(
      header: FHeader.nested(
        title: const Text('设置'),
        prefixes: [
          FHeaderAction.back(onPress: () => Navigator.of(context).pop()),
        ],
      ),
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('云同步',
                style: TextStyle(
                    fontSize: 14,
                    fontWeight: FontWeight.w600,
                    color: colors.foreground)),
            const Gap(12),
          if (_sync.connected) ...[
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: const Color(0x11000000),
                borderRadius: BorderRadius.circular(8),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      const Icon(Icons.cloud_done,
                          size: 14, color: Color(0xFF22C55E)),
                      const Gap(6),
                      Text(_sync.username ?? '已连接',
                          style: const TextStyle(fontSize: 13)),
                    ],
                  ),
                  const Gap(4),
                  Text(_sync.serverUrl ?? '',
                      style: const TextStyle(
                          fontSize: 11, color: Color(0xFF9CA3AF))),
                  if (_sync.lastSyncAt != null) ...[
                    const Gap(2),
                    Text('上次同步：${formatDateTime(_sync.lastSyncAt)}',
                        style: const TextStyle(
                            fontSize: 10, color: Color(0xFFBBBBBB))),
                  ],
                ],
              ),
            ),
            const Gap(8),
            Row(
              children: [
                FButton(
                  variant: FButtonVariant.outline,
                  mainAxisSize: MainAxisSize.min,
                  onPress: _busy
                      ? null
                      : () async {
                          setState(() => _busy = true);
                          try {
                            await _sync.fullSync();
                            await ref
                                .read(taskListProvider.notifier)
                                .refreshFromDb();
                          } finally {
                            if (mounted) setState(() => _busy = false);
                          }
                        },
                  child: const Text('立即同步'),
                ),
                const Gap(8),
                FButton(
                  variant: FButtonVariant.outline,
                  mainAxisSize: MainAxisSize.min,
                  onPress: () async {
                    await _sync.logout();
                    if (mounted) setState(() {});
                  },
                  child: const Text('退出登录'),
                ),
              ],
            ),
          ] else ...[
            FTextField(
              control: FTextFieldControl.managed(controller: _urlController),
              hint: '服务端 URL',
            ),
            const Gap(8),
            FTextField(
              control: FTextFieldControl.managed(controller: _userController),
              hint: '用户名',
            ),
            const Gap(8),
            FTextField.password(
              control: FTextFieldControl.managed(controller: _pwdController),
              hint: '密码',
            ),
            const Gap(12),
            FButton(
              mainAxisSize: MainAxisSize.min,
              onPress: _busy ? null : _login,
              child: Text(_busy ? '登录中…' : '登录并同步'),
            ),
          ],
          if (_error != null) ...[
            const Gap(8),
            Text(_error!,
                style: const TextStyle(fontSize: 11, color: Color(0xFFEF4444))),
          ],
          ],
        ),
      ),
    );
  }
}
