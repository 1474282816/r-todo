/**
 * 应用内更新 前端桥
 *
 * 调用 tauri-plugin-updater 检查、下载、安装更新。
 * 启动时静默检查，有更新时通知 UI 显示红点。
 */
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

export type UpdateStatus =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "available"; version: string; body: string | null }
  | { state: "not-available" }
  | { state: "downloading"; downloaded: number; total: number }
  | { state: "ready" }
  | { state: "error"; message: string };

type Listener = (status: UpdateStatus) => void;
const listeners = new Set<Listener>();
let status: UpdateStatus = { state: "idle" };
let pendingUpdate: Update | null = null;

function updateStatus(patch: UpdateStatus) {
  status = patch;
  listeners.forEach((l) => l(status));
}

export function subscribeUpdateStatus(fn: Listener): () => void {
  listeners.add(fn);
  fn(status);
  return () => {
    listeners.delete(fn);
  };
}

export function getUpdateStatus(): UpdateStatus {
  return status;
}

/** 获取当前版本号 */
export async function getAppVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "unknown";
  }
}

/** 静默检查更新（启动时调用） */
export async function silentCheckUpdate(): Promise<void> {
  try {
    const update = await check();
    if (update) {
      pendingUpdate = update;
      updateStatus({
        state: "available",
        version: update.version,
        body: update.body ?? null,
      });
    }
  } catch {
    // 静默检查失败不报错
  }
}

/** 手动检查更新 */
export async function checkForUpdate(): Promise<void> {
  updateStatus({ state: "checking" });
  try {
    const update = await check();
    if (update) {
      pendingUpdate = update;
      updateStatus({
        state: "available",
        version: update.version,
        body: update.body ?? null,
      });
    } else {
      updateStatus({ state: "not-available" });
    }
  } catch (e) {
    updateStatus({ state: "error", message: String(e) });
  }
}

/** 下载并安装更新 */
export async function downloadAndInstall(): Promise<void> {
  if (!pendingUpdate) {
    updateStatus({ state: "error", message: "没有待安装的更新" });
    return;
  }

  let downloaded = 0;
  let total = 0;

  updateStatus({ state: "downloading", downloaded: 0, total: 0 });

  try {
    await pendingUpdate.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          total = event.data.contentLength ?? 0;
          updateStatus({ state: "downloading", downloaded: 0, total });
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          updateStatus({ state: "downloading", downloaded, total });
          break;
        case "Finished":
          break;
      }
    });

    updateStatus({ state: "ready" });
    pendingUpdate = null;
  } catch (e) {
    updateStatus({ state: "error", message: String(e) });
  }
}

/** 重启应用 */
export async function relaunchApp(): Promise<void> {
  await relaunch();
}
