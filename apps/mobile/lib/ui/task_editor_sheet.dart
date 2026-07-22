import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:forui/forui.dart';
import 'package:gap/gap.dart';

import '../data/task.dart';
import '../state/task_store.dart';
import 'constants.dart';

/// 任务编辑底部 sheet（M7 核心版：标题 + 优先级；提醒/重复留第二批）
class TaskEditorSheet extends ConsumerStatefulWidget {
  const TaskEditorSheet({super.key, required this.task});

  final TodoTask task;

  @override
  ConsumerState<TaskEditorSheet> createState() => _TaskEditorSheetState();
}

class _TaskEditorSheetState extends ConsumerState<TaskEditorSheet> {
  late final TextEditingController _titleController;
  late Priority _priority;

  @override
  void initState() {
    super.initState();
    _titleController = TextEditingController(text: widget.task.title);
    _priority = widget.task.priority;
  }

  @override
  void dispose() {
    _titleController.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final updated = widget.task.copyWith(
      title: _titleController.text.trim().isEmpty
          ? widget.task.title
          : _titleController.text.trim(),
      priority: _priority,
    );
    await ref.read(taskListProvider.notifier).update(widget.task.id, updated);
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final colors = context.theme.colors;
    return Container(
      width: double.infinity,
      decoration: BoxDecoration(
        color: colors.background,
        borderRadius: const BorderRadius.vertical(top: Radius.circular(16)),
      ),
      padding: EdgeInsets.only(
        left: 16,
        right: 16,
        top: 16,
        bottom: MediaQuery.of(context).viewInsets.bottom + 16,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('编辑任务',
              style: TextStyle(
                  fontSize: 14,
                  fontWeight: FontWeight.w600,
                  color: colors.foreground)),
          const Gap(12),
          FTextField(
            control: FTextFieldControl.managed(controller: _titleController),
            hint: '任务标题',
          ),
          const Gap(16),
          const Text('优先级',
              style: TextStyle(fontSize: 12, color: Color(0xFF9CA3AF))),
          const Gap(8),
          Row(
            children: [
              for (final p in Priority.values)
                Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: GestureDetector(
                    onTap: () => setState(() => _priority = p),
                    child: Container(
                      padding: const EdgeInsets.symmetric(
                          horizontal: 12, vertical: 6),
                      decoration: BoxDecoration(
                        color: _priority == p
                            ? const Color(0x22000000)
                            : Colors.transparent,
                        borderRadius: BorderRadius.circular(20),
                      ),
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Container(
                            width: 8,
                            height: 8,
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              color: priorityColors[p],
                            ),
                          ),
                          const Gap(6),
                          Text(priorityLabels[p]!,
                              style: const TextStyle(fontSize: 13)),
                        ],
                      ),
                    ),
                  ),
                ),
            ],
          ),
          const Gap(20),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              FButton(
                variant: FButtonVariant.outline,
                mainAxisSize: MainAxisSize.min,
                onPress: () => Navigator.of(context).pop(),
                child: const Text('取消'),
              ),
              const Gap(8),
              FButton(
                mainAxisSize: MainAxisSize.min,
                onPress: _save,
                child: const Text('保存'),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
