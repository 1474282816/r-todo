import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';
import 'package:gap/gap.dart';

import 'transfer_service.dart';
import 'transfer_settings.dart';

/// 互传设置：设备名、端口、PIN、自动接收。
class TransferSettingsPage extends ConsumerStatefulWidget {
  const TransferSettingsPage({super.key});

  @override
  ConsumerState<TransferSettingsPage> createState() =>
      _TransferSettingsPageState();
}

class _TransferSettingsPageState extends ConsumerState<TransferSettingsPage> {
  late final TextEditingController _aliasController;
  late final TextEditingController _portController;
  late final TextEditingController _pinController;
  bool _autoAccept = false;
  bool _saved = false;

  @override
  void initState() {
    super.initState();
    final settings = ref.read(transferControllerProvider).settings;
    _aliasController = TextEditingController(text: settings.alias);
    _portController =
        TextEditingController(text: settings.port.toString());
    _pinController = TextEditingController(text: settings.pin ?? '');
    _autoAccept = settings.autoAcceptFavorites;
  }

  @override
  void dispose() {
    _aliasController.dispose();
    _portController.dispose();
    _pinController.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final controller = ref.read(transferControllerProvider);
    final port = int.tryParse(_portController.text.trim()) ??
        TransferSettings.defaultPort;

    await controller.saveSettings(TransferSettings(
      alias: _aliasController.text.trim().isEmpty
          ? TransferSettings.defaultAlias
          : _aliasController.text.trim(),
      port: port <= 0 ? TransferSettings.defaultPort : port,
      pin: _pinController.text.trim().isEmpty ? null : _pinController.text.trim(),
      autoAcceptFavorites: _autoAccept,
    ));

    if (!mounted) return;
    setState(() => _saved = true);
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      const SnackBar(content: Text('已保存，重新开启互传后生效')),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.theme.colors;

    return FScaffold(
      header: FHeader.nested(
        title: const Text('互传设置'),
        prefixes: [
          FHeaderAction.back(onPress: () => Navigator.of(context).pop()),
        ],
      ),
      child: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
        children: [
          Text(
            '本机',
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w600,
              color: colors.foreground,
            ),
          ),
          const Gap(8),
          FTextField(
            control: FTextFieldControl.managed(controller: _aliasController),
            hint: '设备名称（对方看到的名字）',
          ),
          const Gap(8),
          FTextField(
            control: FTextFieldControl.managed(controller: _portController),
            hint: '端口（默认 53317）',
          ),
          const Gap(8),
          FTextField(
            control: FTextFieldControl.managed(controller: _pinController),
            hint: 'PIN 校验码（留空表示不需要）',
          ),
          const Gap(16),
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      '自动接收收藏设备',
                      style: TextStyle(fontSize: 13, color: colors.foreground),
                    ),
                    const Gap(2),
                    const Text(
                      '只对已收藏（★）的设备生效，默认关闭',
                      style: TextStyle(fontSize: 11, color: Color(0xFF9CA3AF)),
                    ),
                  ],
                ),
              ),
              FSwitch(
                value: _autoAccept,
                onChange: (value) => setState(() => _autoAccept = value),
              ),
            ],
          ),
          const Gap(20),
          Row(
            children: [
              FButton(
                mainAxisSize: MainAxisSize.min,
                onPress: _save,
                child: Text(_saved ? '已保存' : '保存'),
              ),
              const Gap(12),
              if (_saved)
                const Text(
                  '重新开启互传后生效',
                  style: TextStyle(fontSize: 11, color: Color(0xFF9CA3AF)),
                ),
            ],
          ),
          const Gap(24),
          Text(
            '接收目录',
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w600,
              color: colors.foreground,
            ),
          ),
          const Gap(6),
          const Text(
            '接收的文件保存到系统「下载/r-todo」。传输中的临时文件放在应用缓存里，结束后自动清理。',
            style: TextStyle(fontSize: 11, height: 1.6, color: Color(0xFF9CA3AF)),
          ),
        ],
      ),
    );
  }
}
