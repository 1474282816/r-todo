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
  // 拖滑杆会高频触发，节流后再广播给副窗口，否则事件刷屏
  scheduleOpacityBroadcast();
}

let opacityBroadcastTimer: number | null = null;

/** 节流广播透明度变化（100ms 合并一次）。 */
function scheduleOpacityBroadcast(): void {
  if (opacityBroadcastTimer !== null) return;
  opacityBroadcastTimer = window.setTimeout(() => {
    opacityBroadcastTimer = null;
    // 用当前实际值而不是闭包里的旧值：拖动过程中 alpha 一直在变
    const current = getWindowOpacity();
    broadcastVisual(getEffectiveBackdrop(), current);
  }, 100);
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
  const alpha = getWindowOpacity();
  // 透明度是全局的（半透明档全权用它，模糊档作为叠加底色的参考）
  document.body.style.setProperty("--win-alpha", String(alpha));

  // 实际生效的材质。降级时它会和 `kind` 分叉，广播必须用它 ——
  // 否则副窗口按"mica"渲染却没有 --wp-mica，只剩一层薄底，几乎全透明。
  let effective: Backdrop = kind;

  if (kind === "none") {
    delete document.body.dataset.backdrop;
    document.body.style.removeProperty("--wp-mica");
    await safeInvoke("set_window_backdrop", { backdrop: "none" });
    broadcastVisual(effective, alpha);
    return;
  }

  document.body.dataset.backdrop = kind;
  // DWM SystemBackdrop 关掉（失焦黑屏，不用）
  await safeInvoke("set_window_backdrop", { backdrop: "none" });

  // 半透明档：不读壁纸、不跑模糊，CSS 单图层搞定
  if (kind === "translucent") {
    document.body.style.removeProperty("--wp-mica");
    broadcastVisual(effective, alpha);
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
    effective = "translucent";
  }

  // 把最终生效的材质广播给其它窗口。
  //
  // 为什么要广播：每个 Tauri 窗口是独立 webview，localStorage 不共享
  // （WebView2 各环境隔离），电量挂件自己读 BACKDROP_KEY 只会拿到默认值
  // translucent —— 表现出来就是"两个窗口材质不一样"。
  // 主窗口是唯一决定材质的窗口（设置面板在它里面），由它推送、其余窗口跟随。
  broadcastVisual(effective, alpha);
}

/** 当前生效的材质状态，跨窗口共享用。 */
interface VisualState {
  kind: Backdrop;
  /** 已算好的壁纸模糊图 data URI；`translucent` / `none` 时为 null。 */
  mica: string | null;
  alpha: number;
}

/**
 * 当前**实际生效**的材质档位。
 *
 * 和 `getBackdropPreference()` 的区别：后者是用户选的，前者是真正渲染出来的。
 * 壁纸读取失败时偏好仍是 `mica`，但实际已降级成 `translucent` —— 广播必须用实际值。
 */
function getEffectiveBackdrop(): Backdrop {
  const applied = document.body.dataset.backdrop as Backdrop | undefined;
  if (!applied) return "none";
  return applied;
}

/**
 * 把材质状态推给所有窗口。
 *
 * 只由主窗口调用 —— 它持有设置面板，是材质的唯一真相来源。
 */
async function broadcastVisual(kind: Backdrop, alpha: number): Promise<void> {
  try {
    const { emit } = await import("@tauri-apps/api/event");
    const micaVar = document.body.style.getPropertyValue("--wp-mica");
    // 从 `url("data:image/png;base64,....")` 里取出原始 data URI
    const mica = micaVar ? micaVar.replace(/^url\("?/, "").replace(/"?\)$/, "") : null;
    await emit("visual-changed", { kind, mica, alpha });
  } catch {
    /* 非 Tauri 环境：忽略 */
  }
}

/**
 * 订阅主窗口推来的材质状态，并应用到当前窗口。
 *
 * 供副窗口（电量挂件等）调用。副窗口不要自己读 localStorage 决定材质 ——
 * 读不到用户在设置面板里的选择。
 */
export async function followVisualChanges(): Promise<void> {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<VisualState>("visual-changed", (e) => {
      const { kind, mica, alpha } = e.payload;
      document.body.style.setProperty("--win-alpha", String(alpha));
      if (kind === "none") {
        delete document.body.dataset.backdrop;
        document.body.style.removeProperty("--wp-mica");
        return;
      }
      document.body.dataset.backdrop = kind;
      if (mica) {
        document.body.style.setProperty("--wp-mica", `url("${mica}")`);
      } else {
        document.body.style.removeProperty("--wp-mica");
      }
    });

    // 主动要一次当前状态：副窗口可能在主窗口广播之后才创建
    const { emit } = await import("@tauri-apps/api/event");
    await emit("visual-request");
  } catch {
    /* 非 Tauri 环境：忽略 */
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
  // 副窗口（电量挂件）创建后可能主动来要一次当前材质，
  // 这里监听并重新广播 —— 否则它在主窗口广播之后才打开时永远是默认色。
  try {
    const { listen } = await import("@tauri-apps/api/event");
    await listen("visual-request", () => {
      refreshWindowVisual().catch(() => {});
    });
  } catch {
    /* 非 Tauri 环境：忽略 */
  }
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
