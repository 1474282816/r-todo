import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

const DEFAULT_OFFSET_KEY = "rtools/default-remind-offset";

/** 全局默认提前提醒分钟数（新建任务时使用）。 */
export function getDefaultOffset(): number {
  const value = localStorage.getItem(DEFAULT_OFFSET_KEY);
  return value ? Number(value) || 15 : 15;
}

export function setDefaultOffset(minutes: number): void {
  localStorage.setItem(DEFAULT_OFFSET_KEY, String(minutes));
}

export async function ensureNotificationPermission(): Promise<boolean> {
  let granted = await isPermissionGranted();
  if (!granted) {
    granted = (await requestPermission()) === "granted";
  }
  return granted;
}

/**
 * Engine 是提醒的唯一调度者。新壳不再向前端缓存任务时间或运行第二套定时器。
 */
export async function prepareNotifications(): Promise<void> {
  await ensureNotificationPermission();
}
