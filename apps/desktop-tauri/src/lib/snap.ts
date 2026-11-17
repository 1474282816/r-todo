/**
 * 窗口贴边吸附 + 收起为边条
 */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, PhysicalPosition } from "@tauri-apps/api/window";

const COLLAPSE_KEY = "rtools/collapse-on-snap";

export type SnapEdge =
  | "left"
  | "right"
  | "top"
  | "bottom"
  | "top-left"
  | "top-right"
  | "bottom-left"
  | "bottom-right";

export interface SnapTarget {
  edge: SnapEdge;
  x: number;
  y: number;
}

/** 用户偏好：吸附后是否收起为边条（默认关） */
export function isCollapseOnSnapEnabled(): boolean {
  return localStorage.getItem(COLLAPSE_KEY) === "true";
}

export function setCollapseOnSnap(enabled: boolean): void {
  localStorage.setItem(COLLAPSE_KEY, enabled ? "true" : "false");
}

/** 询问 Rust 现在窗口该不该 snap；null 表示不该 */
export async function getSnapTarget(): Promise<SnapTarget | null> {
  return await invoke<SnapTarget | null>("get_snap_target");
}

/** 使用 Tauri 官方的平滑设置位置：这里直接 set，动画由 CSS 无法做窗口移动，
 *  我们用小步长手动缓动 */
export async function animateToPosition(
  targetX: number,
  targetY: number,
  durationMs = 180,
): Promise<void> {
  const win = getCurrentWindow();
  const start = await win.outerPosition();
  const startX = start.x;
  const startY = start.y;
  const dx = targetX - startX;
  const dy = targetY - startY;
  if (dx === 0 && dy === 0) return;

  const t0 = performance.now();
  return new Promise((resolve) => {
    const tick = async () => {
      const t = Math.min(1, (performance.now() - t0) / durationMs);
      // easeOutCubic
      const e = 1 - Math.pow(1 - t, 3);
      const x = Math.round(startX + dx * e);
      const y = Math.round(startY + dy * e);
      await win.setPosition(new PhysicalPosition(x, y));
      if (t >= 1) {
        resolve();
      } else {
        requestAnimationFrame(tick);
      }
    };
    requestAnimationFrame(tick);
  });
}

/** 询问 Rust "收起后"目标坐标，由前端负责动画过去 */
export async function getCollapseTarget(
  edge: SnapEdge,
): Promise<{ x: number; y: number }> {
  const [x, y] = await invoke<[number, number]>("get_collapse_target", {
    edge,
  });
  return { x, y };
}
