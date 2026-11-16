import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../state/task_store.dart';
import '../transfer/transfer_service.dart';
import 'chat_controller.dart';
import 'chat_message.dart';

/// 底部「互传」Tab 的新主页：会话列表 + 附近设备（Q26）。
class ConversationListPage extends ConsumerStatefulWidget {
  const ConversationListPage({super.key});

  @override
  ConsumerState<ConversationListPage> createState() =>
      _ConversationListPageState();
}

class _ConversationListPageState extends ConsumerState<ConversationListPage> {
  late final ChatController controller;

  @override
  void initState() {
    super.initState();
    controller = ChatController(
      // 与待办共用同一个数据库实例，避免启动迁移时多个 SQLite 连接争锁。
      store: ref.read(databaseProvider),
      transfer: ref.read(transferControllerProvider),
    );
    controller.addListener(_onChanged);
    controller.init();
  }

  void _onChanged() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    controller.removeListener(_onChanged);
    controller.dispose();
    super.dispose();
  }

  Future<void> _open(String id, String alias) async {
    await Navigator.of(context).push(MaterialPageRoute<void>(
      builder: (_) => ChatPage(controller: controller, id: id, alias: alias),
    ));
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final prompt = controller.trustPrompt;
    // 点击通知进来时直接打开对应会话（Q45）。
    final pending = controller.pendingOpenConversation;
    if (pending != null) {
      controller.pendingOpenConversation = null;
      final alias = controller.conversations
              .where((c) => c.id == pending)
              .isEmpty
          ? pending.substring(0, pending.length > 8 ? 8 : pending.length)
          : controller.conversations
              .firstWhere((c) => c.id == pending)
              .alias;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _open(pending, alias);
      });
    }
    return Stack(
      children: [
        ListView(
          padding: const EdgeInsets.only(bottom: 16),
          children: [
            _transferStatus(),
            if (controller.isRunning && controller.devices.isEmpty)
              const Padding(
                padding: EdgeInsets.fromLTRB(16, 0, 16, 6),
                child: Text(
                  '还没发现附近设备：让对方在自己的互传里点一次「刷新」，'
                  '并确认两台设备在同一个 Wi-Fi。',
                  style: TextStyle(fontSize: 11, color: Colors.orange),
                ),
              ),
            if (controller.lastError != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 6),
                child: Text(
                  controller.lastError!,
                  style: const TextStyle(fontSize: 11, color: Colors.red),
                ),
              ),
            if (controller.notice != null)
              Padding(
                padding: const EdgeInsets.all(8),
                child: Text(controller.notice!,
                    style: const TextStyle(fontSize: 12, color: Colors.blue)),
              ),
            const Padding(
              padding: EdgeInsets.fromLTRB(16, 12, 16, 4),
              child: Text('会话', style: TextStyle(fontWeight: FontWeight.w600)),
            ),
            if (controller.conversations.isEmpty)
              const Padding(
                padding: EdgeInsets.all(16),
                child: Text('还没有会话，从下方附近设备开始',
                    style: TextStyle(fontSize: 12, color: Colors.grey)),
              ),
            for (final c in controller.conversations)
              ListTile(
                leading: CircleAvatar(
                  child: Text(c.alias.isEmpty ? '?' : c.alias.substring(0, 1)),
                ),
                title: Text(c.alias),
                subtitle: Text(
                    c.id.length > 12 ? c.id.substring(0, 12) : c.id,
                    style: const TextStyle(fontSize: 10)),
                trailing: c.unread > 0
                    ? CircleAvatar(
                        radius: 10,
                        backgroundColor: Colors.blue,
                        child: Text('${c.unread}',
                            style: const TextStyle(
                                fontSize: 10, color: Colors.white)))
                    : null,
                onTap: () => _open(c.id, c.alias),
              ),
            const Divider(),
            const Padding(
              padding: EdgeInsets.fromLTRB(16, 8, 16, 4),
              child: Text('附近设备',
                  style: TextStyle(fontWeight: FontWeight.w600)),
            ),
            if (controller.devices.isEmpty)
              const Padding(
                padding: EdgeInsets.all(16),
                child: Text('暂未发现设备（互传开关打开后会自动发现）',
                    style: TextStyle(fontSize: 12, color: Colors.grey)),
              ),
            for (final d in controller.devices)
              ListTile(
                leading: const Icon(Icons.devices),
                title: Text(d.alias),
                subtitle: Text(
                    d.fingerprint.length > 12
                        ? d.fingerprint.substring(0, 12)
                        : d.fingerprint,
                    style: const TextStyle(fontSize: 10)),
                onTap: () =>
                    _open(d.fingerprint.toUpperCase(), d.alias),
              ),
          ],
        ),
        if (prompt != null) _trustDialog(prompt),
      ],
    );
  }

  /// 互传状态条：运行状态 + 启停 + 刷新（手机端之前没有这个入口）。
  Widget _transferStatus() {
    final running = controller.isRunning;
    return Container(
      margin: const EdgeInsets.fromLTRB(12, 10, 12, 6),
      padding: const EdgeInsets.fromLTRB(10, 6, 4, 6),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        children: [
          Icon(
            Icons.circle,
            size: 9,
            color: running ? Colors.green : Colors.grey,
          ),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              running
                  ? '互传已开启 · ${controller.devices.length} 台设备'
                  : '互传未启动',
              style: const TextStyle(fontSize: 12),
            ),
          ),
          TextButton(
            onPressed: () => running
                ? controller.stopService()
                : controller.startService(),
            child: Text(running ? '停止' : '开启'),
          ),
          IconButton(
            tooltip: '刷新附近设备',
            icon: const Icon(Icons.refresh, size: 16),
            onPressed: () => controller.refreshDevices(),
          ),
        ],
      ),
    );
  }

  Widget _trustDialog(TrustPrompt prompt) => Container(
        color: Colors.black54,
        alignment: Alignment.center,
        child: Card(
          margin: const EdgeInsets.all(24),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text('首次接收确认',
                    style: TextStyle(fontWeight: FontWeight.bold)),
                const SizedBox(height: 8),
                Text('${prompt.alias} 第一次给你发文件。确认后这一对设备后续互传不再需要确认。',
                    style: const TextStyle(fontSize: 12)),
                const SizedBox(height: 8),
                for (final file in prompt.event.files)
                  Row(children: [
                    const Icon(Icons.attach_file, size: 14),
                    const SizedBox(width: 4),
                    Expanded(
                      child: Text(file.fileName,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontSize: 12)),
                    ),
                  ]),
                const SizedBox(height: 12),
                Row(
                  mainAxisAlignment: MainAxisAlignment.end,
                  children: [
                    TextButton(
                      onPressed: () => controller.confirmTrust(false),
                      child: const Text('拒绝'),
                    ),
                    const SizedBox(width: 8),
                    FilledButton(
                      onPressed: () => controller.confirmTrust(true),
                      child: const Text('接收并信任'),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      );
}

/// 二级会话页：消息气泡、输入框、图片预览、长按菜单、转发（Q26/Q25）。
class ChatPage extends StatefulWidget {
  const ChatPage({
    super.key,
    required this.controller,
    required this.id,
    required this.alias,
  });

  final ChatController controller;
  final String id;
  final String alias;

  @override
  State<ChatPage> createState() => _ChatPageState();
}

class _ChatPageState extends State<ChatPage> {
  final TextEditingController _text = TextEditingController();

  ChatController get controller => widget.controller;

  @override
  void initState() {
    super.initState();
    controller.addListener(_onChanged);
    controller.openConversation(widget.id);
  }

  void _onChanged() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    controller.removeListener(_onChanged);
    _text.dispose();
    super.dispose();
  }

  void _preview(ChatEntry entry) {
    final preview = entry.message.preview;
    if (preview == null) return;
    showDialog<void>(
      context: context,
      builder: (_) => Dialog(
        backgroundColor: Colors.black,
        child: GestureDetector(
          onTap: () => Navigator.of(context).pop(),
          child: Image.memory(base64Decode(preview)),
        ),
      ),
    );
  }

  void _menu(ChatEntry entry) {
    showModalBottomSheet<void>(
      context: context,
      builder: (_) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (entry.message.kind == ChatMessageKind.image)
              ListTile(
                leading: const Icon(Icons.copy),
                title: const Text('复制图片'),
                onTap: () {
                  Navigator.pop(context);
                  controller.menuAction('copyImage', entry);
                },
              ),
            if (entry.message.filePath != null) ...[
              ListTile(
                leading: const Icon(Icons.folder_open),
                title: const Text('在文件管理器中显示'),
                onTap: () {
                  Navigator.pop(context);
                  controller.menuAction('showInFiles', entry);
                },
              ),
              ListTile(
                leading: const Icon(Icons.save_alt),
                title: const Text('保存到相册'),
                onTap: () {
                  Navigator.pop(context);
                  controller.menuAction('saveToGallery', entry);
                },
              ),
              ListTile(
                leading: const Icon(Icons.open_in_new),
                title: const Text('用其他应用打开'),
                onTap: () {
                  Navigator.pop(context);
                  controller.menuAction('openFile', entry);
                },
              ),
            ],
            ListTile(
              leading: const Icon(Icons.forward),
              title: const Text('转发'),
              onTap: () {
                Navigator.pop(context);
                _forward(entry);
              },
            ),
            ListTile(
              leading: const Icon(Icons.delete, color: Colors.red),
              title: const Text('删除（含本地文件）',
                  style: TextStyle(color: Colors.red)),
              onTap: () {
                Navigator.pop(context);
                _delete(entry);
              },
            ),
          ],
        ),
      ),
    );
  }

  void _forward(ChatEntry entry) {
    showModalBottomSheet<void>(
      context: context,
      builder: (_) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Padding(
              padding: EdgeInsets.all(12),
              child: Text('转发给', style: TextStyle(fontWeight: FontWeight.bold)),
            ),
            for (final c in controller.conversations
                .where((c) => c.id != widget.id))
              ListTile(
                title: Text(c.alias),
                onTap: () {
                  Navigator.pop(context);
                  controller.forward(entry, c.id, c.alias);
                },
              ),
            for (final d in controller.devices
                .where((d) =>
                    d.fingerprint.toUpperCase() != widget.id &&
                    !controller.conversations
                        .any((c) => c.id == d.fingerprint.toUpperCase())))
              ListTile(
                title: Text(d.alias),
                onTap: () {
                  Navigator.pop(context);
                  controller.forward(
                      entry, d.fingerprint.toUpperCase(), d.alias);
                },
              ),
          ],
        ),
      ),
    );
  }

  void _delete(ChatEntry entry) {
    showDialog<void>(
      context: context,
      builder: (_) => AlertDialog(
        title: const Text('删除消息'),
        content: const Text('同时删除本地接收文件？此操作不可恢复（发送方原始文件不会被删）。'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('取消'),
          ),
          TextButton(
            onPressed: () {
              Navigator.pop(context);
              controller.deleteEntry(entry);
            },
            child: const Text('删除', style: TextStyle(color: Colors.red)),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.alias),
        actions: [
          IconButton(
            tooltip: '取消信任',
            icon: const Icon(Icons.star_border),
            onPressed: () => controller.revokeTrust(widget.id),
          ),
          IconButton(
            tooltip: '清空历史',
            icon: const Icon(Icons.delete_sweep),
            onPressed: () => controller.clearHistory(widget.id),
          ),
        ],
      ),
      body: Column(
        children: [
          Expanded(
            child: ListView.builder(
              padding: const EdgeInsets.all(12),
              itemCount: controller.messages.length,
              itemBuilder: (_, index) {
                final entry = controller.messages[index];
                final mine = entry.direction == 'send';
                return Align(
                  alignment:
                      mine ? Alignment.centerRight : Alignment.centerLeft,
                  child: GestureDetector(
                    onLongPress: () => _menu(entry),
                    onTap: () => _preview(entry),
                    child: Container(
                      margin: const EdgeInsets.symmetric(vertical: 4),
                      padding: const EdgeInsets.all(10),
                      constraints: BoxConstraints(
                        maxWidth: MediaQuery.of(context).size.width * 0.72,
                      ),
                      decoration: BoxDecoration(
                        color: mine
                            ? Colors.blue
                            : Theme.of(context).colorScheme.surfaceContainerHighest,
                        borderRadius: BorderRadius.circular(10),
                      ),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (entry.message.preview != null)
                            Image.memory(
                              base64Decode(entry.message.preview!),
                              height: 160,
                              fit: BoxFit.cover,
                            )
                          else if (entry.message.kind ==
                              ChatMessageKind.text)
                            Text(entry.message.body,
                                style: TextStyle(
                                    color: mine ? Colors.white : null))
                          else
                            Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                const Icon(Icons.attach_file, size: 14),
                                Flexible(
                                  child: Text(entry.message.body,
                                      overflow: TextOverflow.ellipsis),
                                ),
                              ],
                            ),
                          const SizedBox(height: 4),
                          Text(
                            [
                              if (entry.message.status ==
                                  ChatMessageStatus.pending)
                                '待发送',
                              if (entry.message.forwardedFrom != null)
                                '转发自 ${entry.message.forwardedFrom}',
                            ].join(' · '),
                            style: const TextStyle(
                                fontSize: 9, color: Colors.white70),
                          ),
                        ],
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
          SafeArea(
            child: Row(
              children: [
                IconButton(
                  icon: const Icon(Icons.attach_file),
                  onPressed: controller.sendFiles,
                ),
                Expanded(
                  child: TextField(
                    controller: _text,
                    decoration: const InputDecoration(
                      hintText: '输入消息',
                      border: InputBorder.none,
                    ),
                    onSubmitted: (value) {
                      controller.sendText(value);
                      _text.clear();
                    },
                  ),
                ),
                IconButton(
                  icon: const Icon(Icons.send),
                  onPressed: () {
                    controller.sendText(_text.text);
                    _text.clear();
                  },
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
