import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:r_todo_mobile/features/transfer/transfer_favorites.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() => SharedPreferences.setMockInitialValues({}));

  test('首次加载为空集合', () async {
    expect(await TransferFavoritesStore().load(), isEmpty);
  });

  test('toggle 加入后再 toggle 移除', () async {
    final store = TransferFavoritesStore();

    final added = await store.toggle('FP-1');
    expect(added, {'FP-1'});

    final removed = await store.toggle('FP-1');
    expect(removed, isEmpty);
  });

  test('收藏结果持久化，新实例能读到', () async {
    await TransferFavoritesStore().toggle('FP-1');
    await TransferFavoritesStore().toggle('FP-2');

    expect(await TransferFavoritesStore().load(), {'FP-1', 'FP-2'});
  });
}
