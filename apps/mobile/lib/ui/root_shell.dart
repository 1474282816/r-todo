import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';
import 'package:gap/gap.dart';

import '../features/transfer/transfer_page.dart';
import '../features/transfer/transfer_settings_page.dart';
import '../state/task_store.dart';
import 'home_page.dart';
import 'settings_page.dart';

/// 底部两个 Tab：待办 / 互传。header 与底部栏统一在这里，子页面只负责内容。
class RootShell extends ConsumerStatefulWidget {
  const RootShell({super.key});

  @override
  ConsumerState<RootShell> createState() => _RootShellState();
}

class _RootShellState extends ConsumerState<RootShell> {
  int _index = 0;

  @override
  Widget build(BuildContext context) {
    // Tab 之间是平级的，不拦截系统返回：任一 Tab 按返回都直接退到桌面/后台。
    return FScaffold(
      header: _index == 0 ? _buildTodoHeader() : _buildTransferHeader(),
      footer: FBottomNavigationBar(
        index: _index,
        onChange: (index) => setState(() => _index = index),
        children: const [
          FBottomNavigationBarItem(
            icon: Icon(Icons.checklist_outlined),
            label: Text('待办'),
          ),
          FBottomNavigationBarItem(
            icon: Icon(Icons.swap_horiz),
            label: Text('互传'),
          ),
        ],
      ),
      child: IndexedStack(
        index: _index,
        children: const [
          HomeView(),
          TransferView(),
        ],
      ),
    );
  }

  Widget _buildTodoHeader() {
    final visible = sortAndFilter(ref.watch(taskListProvider));
    final activeCount = visible.where((task) => !task.isCompleted).length;

    return FHeader(
      title: Row(
        children: [
          const Text('待办事项'),
          const Gap(8),
          Text(
            '($activeCount/${visible.length})',
            style: const TextStyle(fontSize: 13, color: Color(0xFF9CA3AF)),
          ),
        ],
      ),
      suffixes: [
        FHeaderAction(
          icon: const Icon(Icons.settings_outlined),
          onPress: () => Navigator.of(context).push(
            MaterialPageRoute<void>(builder: (_) => const SettingsPage()),
          ),
        ),
      ],
    );
  }

  Widget _buildTransferHeader() {
    return FHeader(
      title: const Text('互传'),
      suffixes: [
        FHeaderAction(
          icon: const Icon(Icons.tune),
          onPress: () => Navigator.of(context).push(
            MaterialPageRoute<void>(builder: (_) => const TransferSettingsPage()),
          ),
        ),
      ],
    );
  }
}
