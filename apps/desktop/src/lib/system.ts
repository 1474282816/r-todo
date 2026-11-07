/**
 * Windows 桌面嵌入 + 开机自启 前端桥
 */
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

const STORAGE_KEY = "r-todo/embedded";

export async function embedToDesktop(): Promise<void> {
  try {
    await invoke("embed_to_desktop");
    localStorage.setItem(STORAGE_KEY, "true");
  } catch (e) {
    // 嵌入失败（本机没有可用的图标层 WorkerW / 嵌入后不可见已回滚）：
    // 记成浮空，下次启动不再尝试，避免反复把窗口塞进看不见的层
    localStorage.setItem(STORAGE_KEY, "false");
    throw e;
  } finally {
  // 嵌入/浮空切换影响材质实现方式（挂件态同样用壁纸模糊，语义一致）
  await refreshWindowVisual().catch(() => {});
}
}

export async function unembedFromDesktop(): Promise<void> {
  await invoke("unembed_from_desktop");
  localStorage.setItem(STORAGE_KEY, "false");
  await refreshWindowVisual().catch(() => {});
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

/* ── Win11 窗口材质（DWM SystemBackdrop + 嵌态壁纸模糊模拟） ── */

export type Backdrop = "translucent" | "acrylic" | "mica" | "mica_alt" | "none";

const BACKDROP_KEY = "r-todo/backdrop";
/** 用户是否显式点过材质按钮（没有则是程序默认，可被新版默认覆盖） */
const BACKDROP_CHOSEN_KEY = "r-todo/backdrop-chosen";

/** 所有合法档位，按设置面板展示顺序 */
export const BACKDROP_OPTIONS: { value: Backdrop; label: string; desc: string }[] = [
  { value: "translucent", label: "半透明", desc: "纯 CSS 半透明，零开销，任何机器都生效" },
  { value: "acrylic", label: "亚克力", desc: "实时毛玻璃，最通透" },
  { value: "mica", label: "Mica", desc: "壁纸取色，官方窗口底色" },
  { value: "mica_alt", label: "Mica Alt", desc: "更强的壁纸取色" },
  { value: "none", label: "无", desc: "不透明纯色（回到默认窗口底色）" },
];

/** 偏好值归一化：老版本没有 translucent 档 */
function normalize(v: string | null): Backdrop | null {
  if (v === "translucent" || v === "none" || v === "mica" || v === "mica_alt" || v === "acrylic") {
    return v;
  }
  return null;
}

/**
 * 当前材质偏好。默认 **translucent**（半透明兜底；实时模糊作为可选增强）。
 *
 * 只有当用户在设置里显式点过按钮（BACKDROP_CHOSEN_KEY）才保留旧值，
 * 否则跟随本版本的默认值 —— 老版本程序写死的 acrylic 不会挡住新版默认。
 */
export function getBackdropPreference(): Backdrop {
  if (localStorage.getItem(BACKDROP_CHOSEN_KEY) !== "true") return "translucent";
  return normalize(localStorage.getItem(BACKDROP_KEY)) ?? "translucent";
}

/** 窗口不透明度偏好，0.2 ~ 1.0，默认 0.82 */
const OPACITY_KEY = "r-todo/opacity";
const MIN_OPACITY = 0.2;

export function getWindowOpacity(): number {
  const raw = localStorage.getItem(OPACITY_KEY);
  const n = raw === null ? NaN : Number(raw);
  if (!Number.isFinite(n)) return 0.82;
  return Math.min(1, Math.max(MIN_OPACITY, n));
}

export function setWindowOpacity(alpha: number): void {
  const v = Math.min(1, Math.max(MIN_OPACITY, alpha));
  localStorage.setItem(OPACITY_KEY, String(v));
  document.body.style.setProperty("--win-alpha", String(v));
}

/** 安全调用：命令不存在（非 Windows）时静默忽略 */
async function safeInvoke(cmd: string, args: Record<string, unknown>): Promise<void> {
  try {
    await invoke(cmd, args);
  } catch {
    /* non-windows or failure: ignore */
  }
}

/**
 * 按当前（材质偏好 + 是否挂件模式）刷新窗口视觉。
 *
 * 挂件模式（嵌入桌面，shell ownership + 置底）下 Tauri/WebView2 的 DWM
 * SystemBackdrop 失焦会黑屏（上游 bug，window-vibrancy#183），所以一律用
 * **壁纸高斯模糊模拟**：Rust 读壁纸 → blur → data URI → CSS 背景。
 * 语义等同 Mica（官方定义就是壁纸快照取色）。
 * 换壁纸时 Rust 侧 emit("wallpaper-changed")，这里自动重新生成 —— 实时跟随。
 */
async function refreshWindowVisual(): Promise<void> {
  const kind = getBackdropPreference();
  // 透明度是全局的（半透明档全权用它，模糊档作为叠加底色的参考）
  document.body.style.setProperty("--win-alpha", String(getWindowOpacity()));

  if (kind === "none") {
    delete document.body.dataset.backdrop;
    document.body.style.removeProperty("--wp-mica");
    await safeInvoke("set_window_backdrop", { backdrop: "none" });
    return;
  }

  document.body.dataset.backdrop = kind;
  // DWM SystemBackdrop 关掉（失焦黑屏，不用）
  await safeInvoke("set_window_backdrop", { backdrop: "none" });

  // 半透明档：不读壁纸、不跑模糊，CSS 单图层搞定
  if (kind === "translucent") {
    document.body.style.removeProperty("--wp-mica");
    return;
  }

  // 壁纸模糊背景（实时模糊）
  const sigma = kind === "acrylic" ? 32 : kind === "mica_alt" ? 20 : 12;
  try {
    const uri = await invoke<string>("get_wallpaper_mica", { sigma });
    document.body.style.setProperty("--wp-mica", `url("${uri}")`);
  } catch {
    // 壁纸读取失败（纯色背景等）：自动降级为半透明
    document.body.dataset.backdrop = "translucent";
    document.body.style.removeProperty("--wp-mica");
  }
}

/** 监听系统壁纸变化，自动刷新模糊背景（挂件启动后调用一次） */
export async function watchWallpaperChanges(): Promise<void> {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen("wallpaper-changed", () => {
      refreshWindowVisual().catch(() => {});
    });
  } catch {
    /* non-tauri environment: ignore */
  }
}

/**
 * 应用材质并记录偏好。
 */
export async function setWindowBackdrop(kind: Backdrop): Promise<void> {
  localStorage.setItem(BACKDROP_KEY, kind);
  localStorage.setItem(BACKDROP_CHOSEN_KEY, "true"); // 之后不再被默认值覆盖
  await refreshWindowVisual();
}

/** 启动时恢复材质偏好（App 挂载后调用） */
export async function restoreBackdrop(): Promise<void> {
  await refreshWindowVisual();
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
