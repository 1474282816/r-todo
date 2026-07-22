/**
 * Windows 桌面嵌入 + 开机自启 前端桥
 */
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

const STORAGE_KEY = "r-todo/embedded";

export async function embedToDesktop(): Promise<void> {
  await invoke("embed_to_desktop");
  localStorage.setItem(STORAGE_KEY, "true");
}

export async function unembedFromDesktop(): Promise<void> {
  await invoke("unembed_from_desktop");
  localStorage.setItem(STORAGE_KEY, "false");
}

/**
 * 是否偏好嵌入桌面模式。
 *
 * 默认 **true**（首次运行未设置时）。
 * 用户显式切到"浮空"后，localStorage 里存 "false"，之后启动会保留浮空状态。
 */
export function isEmbeddedPreference(): boolean {
  const v = localStorage.getItem(STORAGE_KEY);
  if (v === null) return true; // 首次运行默认嵌入
  return v === "true";
}

export async function isAutostartEnabled(): Promise<boolean> {
  try {
    return await isEnabled();
  } catch {
    return false;
  }
}

export async function setAutostart(enable_: boolean): Promise<void> {
  if (enable_) await enable();
  else await disable();
}
