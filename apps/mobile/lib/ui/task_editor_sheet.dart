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
  late final TextEditingController _remindController;
  late final TextEditingController _repeatController;
  late Priority _priority;
  int _offset = 15;

  @override
  void initState() {
    super.initState();
    _titleController = TextEditingController(text: widget.task.title);
    _remindController = TextEditingController(
      text: _localText(widget.task.remindAt),
    );
    _repeatController = TextEditingController(
      text: widget.task.repeatRule ?? '',
    );
    _priority = widget.task.priority;
    _offset = widget.task.remindOffsetMin;
  }

  @override
  void dispose() {
    _titleController.dispose();
    _remindController.dispose();
    _repeatController.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final updated = widget.task.copyWith(
      title: _titleController.text.trim().isEmpty
          ? widget.task.title
          : _titleController.text.trim(),
      priority: _priority,
      remindAt: () => _iso(_remindController.text),
      remindOffsetMin: _offset,
      repeatRule: () {
        final value = _repeatController.text.trim();
        return value.isEmpty ? null : value;
      },
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
          const Text('提醒时间（例如 2026-11-17 09:30）',
              style: TextStyle(fontSize: 12, color: Color(0xFF9CA3AF))),
          const Gap(8),
          FTextField(
            control: FTextFieldControl.managed(controller: _remindController),
            hint: '留空表示不提醒',
          ),
          const Gap(8),
          Wrap(
            spacing: 6,
            children: [
              for (final minutes in const [0, 5, 15, 30, 60])
                ChoiceChip(
                  label: Text(minutes == 0 ? '准时' : '$minutes 分钟前'),
                  selected: _offset == minutes,
                  onSelected: (_) => setState(() => _offset = minutes),
                ),
            ],
          ),
          const Gap(16),
          const Text('重复规则（RRULE，留空表示不重复）',
              style: TextStyle(fontSize: 12, color: Color(0xFF9CA3AF))),
          const Gap(8),
          FTextField(
            control: FTextFieldControl.managed(controller: _repeatController),
            hint: '例如 FREQ=DAILY;INTERVAL=1',
          ),
          const Gap(8),
          Wrap(
            spacing: 6,
            children: [
              for (final preset in const [
                'FREQ=DAILY;INTERVAL=1',
                'FREQ=WEEKLY;INTERVAL=1',
                'FREQ=MONTHLY;INTERVAL=1',
              ])
                ActionChip(
                  label: Text(switch (preset.split(';').first) {
                    'FREQ=DAILY' => '每天',
                    'FREQ=WEEKLY' => '每周',
                    _ => '每月',
                  }),
                  onPressed: () =>
                      setState(() => _repeatController.text = preset),
                ),
            ],
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

  static String _localText(String? iso) {
    final value = DateTime.tryParse(iso ?? '')?.toLocal();
    if (value == null) return '';
    String two(int number) => number.toString().padLeft(2, '0');
    return '${value.year}-${two(value.month)}-${two(value.day)} '
        '${two(value.hour)}:${two(value.minute)}';
  }

  static String? _iso(String text) {
    final value = DateTime.tryParse(text.trim());
    return value?.toUtc().toIso8601String();
  }
}
