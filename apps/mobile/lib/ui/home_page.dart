import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';
import 'package:gap/gap.dart';

import '../data/task.dart';
import '../state/task_store.dart';
import 'constants.dart';
import 'settings_page.dart';
import 'task_editor_sheet.dart';

class HomePage extends ConsumerStatefulWidget {
  const HomePage({super.key});

  @override
  ConsumerState<HomePage> createState() => _HomePageState();
}

class _HomePageState extends ConsumerState<HomePage> {
  final _inputController = TextEditingController();
  Priority _quickPriority = Priority.low;

  @override
  void dispose() {
    _inputController.dispose();
    super.dispose();
  }

  Future<void> _quickAdd() async {
    final text = _inputController.text.trim();
    if (text.isEmpty) return;
    await ref
        .read(taskListProvider.notifier)
        .add(title: text, priority: _quickPriority);
    _inputController.clear();
    setState(() => _quickPriority = Priority.low);
  }

  @override
  Widget build(BuildContext context) {
    final tasks = ref.watch(taskListProvider);
    final visible = sortAndFilter(tasks);
    final active = visible.where((t) => !t.isCompleted).toList();
    final done = visible.where((t) => t.isCompleted).toList();

    return FScaffold(
      header: FHeader(
        title: Row(
          children: [
            const Text('待办事项'),
            const Gap(8),
            Text(
              '(${active.length}/${visible.length})',
              style: const TextStyle(
                fontSize: 13,
                color: Color(0xFF9CA3AF),
              ),
            ),
          ],
        ),
        suffixes: [
          FHeaderAction(
            icon: const Icon(Icons.settings_outlined),
            onPress: () => Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (_) => const SettingsPage(),
              ),
            ),
          ),
        ],
      ),
      child: Column(
        children: [
          // 快速添加
          _QuickAddBar(
            controller: _inputController,
            priority: _quickPriority,
            onPriorityChange: (p) => setState(() => _quickPriority = p),
            onSubmit: _quickAdd,
          ),
          const Gap(8),
          Expanded(
            child: visible.isEmpty
                ? const _EmptyState()
                : _TaskList(active: active, done: done),
          ),
        ],
      ),
    );
  }
}

class _QuickAddBar extends StatelessWidget {
  const _QuickAddBar({
    required this.controller,
    required this.priority,
    required this.onPriorityChange,
    required this.onSubmit,
  });

  final TextEditingController controller;
  final Priority priority;
  final ValueChanged<Priority> onPriorityChange;
  final VoidCallback onSubmit;

  @override
  Widget build(BuildContext context) {
    return Padding(
      // FTextField 自带 horizontal:12 内边距 + ~1px 边框；FTile 内容左缘在 15px。
      // 微调后 left:0 让输入框文字左缘对齐待办项。
      padding: const EdgeInsets.only(left: 0, right: 13),
      child: Row(
        children: [
          Expanded(
            child: FTextField(
              control: FTextFieldControl.managed(controller: controller),
              hint: '添加任务…',
              onSubmit: (_) => onSubmit(),
            ),
          ),
          const Gap(8),
          // 3 个优先级圆点
          for (final p in Priority.values)
            GestureDetector(
              onTap: () => onPriorityChange(p),
              child: Container(
                margin: const EdgeInsets.only(left: 4),
                width: 18,
                height: 18,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: priority == p
                      ? priorityColors[p]
                      : Colors.transparent,
                  border: Border.all(color: priorityColors[p]!, width: 1.5),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _TaskList extends ConsumerWidget {
  const _TaskList({required this.active, required this.done});

  final List<TodoTask> active;
  final List<TodoTask> done;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final notifier = ref.read(taskListProvider.notifier);
    // 底部留白：系统安全区（手势条/圆角）+ 额外余量，防最后一条被屏幕圆角挡住
    final bottomInset = MediaQuery.of(context).padding.bottom + 24;

    return ReorderableListView(
      padding: EdgeInsets.only(bottom: bottomInset),
      buildDefaultDragHandles: false,
      proxyDecorator: (child, index, animation) {
        // 去掉默认的白底+直角+重阴影，改为透明背景 + 圆角 + 柔和阴影
        return AnimatedBuilder(
          animation: animation,
          builder: (context, _) {
            final t = Curves.easeInOut.transform(animation.value);
            return Transform.scale(
              scale: 1 + 0.02 * t,
              child: Material(
                color: Colors.transparent,
                elevation: 0,
                child: Container(
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(10),
                    boxShadow: [
                      BoxShadow(
                        color: Colors.black.withValues(alpha: 0.12 * t),
                        blurRadius: 12 * t,
                        offset: Offset(0, 4 * t),
                      ),
                    ],
                  ),
                  child: child,
                ),
              ),
            );
          },
        );
      },
      onReorder: (oldIndex, newIndex) {
        if (newIndex > active.length) newIndex = active.length;
        if (oldIndex >= active.length) return; // 只允许未完成区内拖
        if (newIndex > oldIndex) newIndex -= 1;
        if (newIndex >= active.length) newIndex = active.length - 1;
        final ids = active.map((t) => t.id).toList();
        final moved = ids.removeAt(oldIndex);
        ids.insert(newIndex, moved);
        notifier.reorderActive(ids);
      },
      children: [
        for (final (i, t) in active.indexed)
          _TaskRow(key: ValueKey(t.id), task: t, dragIndex: i),
        // 已完成区（不可拖，用 ReorderableListView 要求每个 child 有 key）
        if (done.isNotEmpty)
          Padding(
            key: const ValueKey('__done_divider__'),
            padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
            child: Container(height: 1, color: const Color(0x11000000)),
          ),
        for (final t in done)
          _TaskRow(key: ValueKey(t.id), task: t),
      ],
    );
  }
}

class _TaskRow extends ConsumerWidget {
  const _TaskRow({super.key, required this.task, this.dragIndex});

  final TodoTask task;
  /// 未完成任务传入其在 active 列表的索引（用于拖拽）；已完成任务传 null（不可拖）
  final int? dragIndex;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final notifier = ref.read(taskListProvider.notifier);
    final completed = task.isCompleted;

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Dismissible(
        key: ValueKey('dismiss_${task.id}'),
        direction: DismissDirection.endToStart,
        background: Container(
          alignment: Alignment.centerRight,
          padding: const EdgeInsets.only(right: 20),
          decoration: BoxDecoration(
            color: const Color(0xFFEF4444),
            borderRadius: BorderRadius.circular(10),
          ),
          child: const Icon(Icons.delete_outline, color: Colors.white),
        ),
        onDismissed: (_) => notifier.remove(task.id),
        confirmDismiss: (_) async {
          return await showDialog<bool>(
            context: context,
            builder: (ctx) => Dialog(
              backgroundColor: Colors.transparent,
              child: FCard(
                child: Padding(
                  padding: const EdgeInsets.all(20),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      const Text(
                        '确认删除',
                        style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600),
                      ),
                      const Gap(12),
                      Text('确定要删除「${task.title}」吗？'),
                      const Gap(20),
                      Row(
                        mainAxisAlignment: MainAxisAlignment.end,
                        children: [
                          FButton(
                            variant: FButtonVariant.outline,
                            mainAxisSize: MainAxisSize.min,
                            onPress: () => Navigator.of(ctx).pop(false),
                            child: const Text('取消'),
                          ),
                          const Gap(8),
                        GestureDetector(
                          onTap: () => Navigator.of(ctx).pop(true),
                          child: Container(
                            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                            decoration: BoxDecoration(
                              color: const Color(0xFFEF4444),
                              borderRadius: BorderRadius.circular(6),
                            ),
                            child: const Text(
                              '删除',
                              style: TextStyle(color: Colors.white),
                            ),
                          ),
                        ),
                        ],
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ) ?? false;
        },
        child: FTile(
        prefix: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              width: 3,
              height: 24,
              decoration: BoxDecoration(
                color: priorityColors[task.priority],
                borderRadius: BorderRadius.circular(2),
              ),
            ),
            const Gap(8),
            GestureDetector(
              onTap: () => notifier.toggleComplete(task.id),
              child: Container(
                width: 22,
                height: 22,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color:
                      completed ? const Color(0xFF22C55E) : Colors.transparent,
                  border: Border.all(
                    color: completed
                        ? const Color(0xFF22C55E)
                        : const Color(0xFFD4D4D8),
                    width: 2,
                  ),
                ),
                child: completed
                    ? const Icon(Icons.check, size: 14, color: Colors.white)
                    : null,
              ),
            ),
          ],
        ),
        title: Text(
          task.title,
          style: TextStyle(
            decoration: completed ? TextDecoration.lineThrough : null,
            color: completed ? const Color(0xFF9CA3AF) : null,
          ),
        ),
        suffix: dragIndex != null
            ? ReorderableDragStartListener(
                index: dragIndex!,
                child: const Padding(
                  padding: EdgeInsets.symmetric(horizontal: 4, vertical: 8),
                  child: Icon(Icons.drag_indicator,
                      size: 20, color: Color(0xFFCCCCCC)),
                ),
              )
            : null,
        onPress: () => showFSheet(
          context: context,
          side: FLayout.btt,
          draggable: false,
          mainAxisMaxRatio: null,
          builder: (_) => TaskEditorSheet(task: task),
        ),
      ),
      ),
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();

  @override
  Widget build(BuildContext context) {
    return const Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text('今天没有任务',
              style: TextStyle(fontSize: 15, color: Color(0xFF9CA3AF))),
          Gap(6),
          Text('在上方添加一条', style: TextStyle(fontSize: 12, color: Color(0xFFBBBBBB))),
        ],
      ),
    );
  }
}
