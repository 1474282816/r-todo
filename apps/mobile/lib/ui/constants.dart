import 'package:flutter/widgets.dart';

import '../data/task.dart';

const priorityColors = <Priority, Color>{
  Priority.high: Color(0xFFEF4444),
  Priority.medium: Color(0xFFF59E0B),
  Priority.low: Color(0xFF9CA3AF),
};

const priorityLabels = <Priority, String>{
  Priority.high: '高',
  Priority.medium: '中',
  Priority.low: '低',
};
