import 'dart:async';

import '../transfer/transfer_event.dart';

/// FRB 事件流只能订阅一次，聊天层通过广播总线复用 TransferController 的事件。
class ChatEventBus {
  ChatEventBus._();

  static final ChatEventBus instance = ChatEventBus._();

  final StreamController<TransferEvent> _controller =
      StreamController<TransferEvent>.broadcast();

  Stream<TransferEvent> get stream => _controller.stream;

  void add(TransferEvent event) {
    if (!_controller.isClosed) _controller.add(event);
  }
}
