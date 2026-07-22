import { useEffect } from "react";

interface HotkeyHandlers {
  onNew?: () => void;
  onEscape?: () => void;
}

/**
 * 全局键盘快捷键
 * - Ctrl/Cmd + N: 聚焦新建输入框
 * - Escape: 关闭浮层（编辑器/设置）
 */
export function useHotkeys({ onNew, onEscape }: HotkeyHandlers) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const inEditable =
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable);

      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        onNew?.();
        return;
      }
      if (e.key === "Escape" && !inEditable) {
        onEscape?.();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onNew, onEscape]);
}
