import 'package:flutter_test/flutter_test.dart';
import 'package:rrule/rrule.dart';

void main() {
  test('每日重复规则可计算下一次提醒时间', () {
    final base = DateTime.utc(2026, 11, 17, 9, 30);
    final next = RecurrenceRule.fromString('RRULE:FREQ=DAILY;INTERVAL=1')
        .getInstances(start: base, after: base)
        .first;
    expect(next, DateTime.utc(2026, 11, 18, 9, 30));
  });
}
