/**
 * 拖动结束检测 hook
 *
 * Tauri 没有 dragEnd 事件；我们用防抖：
 * onMoved 触发后 150ms 内没有新的 onMoved → 视为拖动结束。
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";

export function useDragEnd(handler: () => void, debounceMs = 150) {
  useEffect(() => {
    let timer: number | null = null;
    let unlisten: (() => void) | null = null;

    const win = getCurrentWindow();
    win
      .onMoved(() => {
        if (timer !== null) window.clearTimeout(timer);
        timer = window.setTimeout(() => {
          handler();
          timer = null;
        }, debounceMs);
      })
      .then((fn) => {
        unlisten = fn;
      });

    return () => {
      if (timer !== null) window.clearTimeout(timer);
      unlisten?.();
    };
  }, [handler, debounceMs]);
}
