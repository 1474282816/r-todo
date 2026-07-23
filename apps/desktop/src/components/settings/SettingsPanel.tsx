import { AnimatePresence, motion } from "framer-motion";
import { Cloud, CloudOff, Download, Loader2, RefreshCw, Upload, X } from "lucide-react";
import { useEffect, useState } from "react";

import { exportTasks, pickAndPreviewImport, type ExportFormat } from "../../lib/io_ui";
import { formatDateTime } from "../../lib/datetime";
import type { MergePlan } from "../../lib/io/import";
import { useTaskStore } from "../../lib/store";
import {
  embedToDesktop,
  isAutostartEnabled,
  isEmbeddedPreference,
  setAutostart,
  unembedFromDesktop,
} from "../../lib/system";
import {
  isCollapseOnSnapEnabled,
  setCollapseOnSnap,
} from "../../lib/snap";
import {
  fullSync,
  getStatus as getSyncStatus,
  login as syncLogin,
  logout as syncLogout,
  subscribeStatus,
  type SyncStatus,
} from "../../lib/sync";
import {
  getDefaultOffset,
  setDefaultOffset,
} from "../../lib/reminder";
import {
  checkForUpdate,
  downloadAndInstall,
  getAppVersion,
  relaunchApp,
  subscribeUpdateStatus,
  type UpdateStatus,
} from "../../lib/updater";
import { cn } from "../../lib/cn";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SettingsPanel({ open, onClose }: Props) {
  const tasks = useTaskStore((s) => s.tasks);
  const bulkImport = useTaskStore((s) => s.bulkImport);

  const [embedded, setEmbedded] = useState(isEmbeddedPreference());
  const [autostart, setAutostartState] = useState(false);
  const [collapse, setCollapseState] = useState(isCollapseOnSnapEnabled());
  const [defaultOffset, setDefaultOffsetState] = useState(getDefaultOffset());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [importPlan, setImportPlan] = useState<MergePlan | null>(null);
  const [importSource, setImportSource] = useState<string | null>(null);

  // 同步状态
  const [sync, setSync] = useState<SyncStatus>(getSyncStatus());
  const [urlInput, setUrlInput] = useState("");
  const [emailInput, setEmailInput] = useState("");
  const [pwdInput, setPwdInput] = useState("");
  const [loginBusy, setLoginBusy] = useState(false);

  // 更新状态
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ state: "idle" });
  const [appVersion, setAppVersion] = useState("...");

  useEffect(() => subscribeStatus(setSync), []);
  useEffect(() => subscribeUpdateStatus(setUpdateStatus), []);

  useEffect(() => {
    if (!toast) return;
    const t = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(t);
  }, [toast]);

  useEffect(() => {
    if (open) {
      isAutostartEnabled().then(setAutostartState);
    }
  }, [open]);

  // 获取版本号
  useEffect(() => {
    getAppVersion().then(setAppVersion);
  }, []);

  const toggleEmbed = async () => {
    setBusy(true);
    setError(null);
    try {
      if (embedded) {
        await unembedFromDesktop();
        setEmbedded(false);
      } else {
        await embedToDesktop();
        setEmbedded(true);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleAutostart = async () => {
    setBusy(true);
    try {
      await setAutostart(!autostart);
      setAutostartState(!autostart);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doExport = async (format: ExportFormat) => {
    setBusy(true);
    setError(null);
    try {
      const path = await exportTasks(format, tasks);
      if (path) setToast(`已导出 ${tasks.length} 条到 ${path}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doPickImport = async () => {
    setBusy(true);
    setError(null);
    try {
      const preview = await pickAndPreviewImport(tasks);
      if (!preview) return;
      setImportPlan(preview.plan);
      setImportSource(preview.path);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doConfirmImport = async () => {
    if (!importPlan) return;
    setBusy(true);
    try {
      await bulkImport(importPlan.toInsert, importPlan.toUpdate);
      setToast(
        `已导入 · 新增 ${importPlan.toInsert.length} · 覆盖 ${importPlan.toUpdate.length} · 跳过 ${importPlan.toSkip.length}`,
      );
      setImportPlan(null);
      setImportSource(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="absolute inset-0 z-40 flex items-end bg-black/20 backdrop-blur-[2px]"
          onClick={onClose}
        >
          <motion.div
            initial={{ y: 40, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: 40, opacity: 0 }}
            transition={{ type: "spring", stiffness: 320, damping: 30 }}
            onClick={(e) => e.stopPropagation()}
            className="w-full rounded-t-2xl bg-white/95 p-4 shadow-2xl dark:bg-neutral-900/95"
          >
            <div className="mb-3 flex items-center justify-between">
              <h2 className="text-sm font-semibold text-neutral-700 dark:text-neutral-200">
                设置
              </h2>
              <button
                onClick={onClose}
                className="rounded p-1 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200"
              >
                <X size={16} />
              </button>
            </div>

            <Row
              label="嵌入桌面"
              desc="窗口挂靠壁纸层，Win+D 不隐藏"
              checked={embedded}
              onToggle={toggleEmbed}
              disabled={busy}
            />
            <Row
              label="开机自启"
              desc="登录后自动打开"
              checked={autostart}
              onToggle={toggleAutostart}
              disabled={busy}
            />
            <Row
              label="吸附后收起为边条"
              desc="拖到屏幕边缘后仅露一条，鼠标进入自动展开"
              checked={collapse}
              onToggle={() => {
                setCollapseOnSnap(!collapse);
                setCollapseState(!collapse);
              }}
              disabled={busy}
            />

            {/* 默认提前提醒 */}
            <div className="rounded-lg px-3 py-2.5">
              <div className="text-xs text-neutral-800 dark:text-neutral-100">
                默认提前提醒
              </div>
              <div className="mt-0.5 text-[10px] text-neutral-500">
                新建任务时的默认提前量
              </div>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {[0, 5, 10, 15, 30, 60, 120].map((n) => (
                  <button
                    key={n}
                    onClick={() => {
                      setDefaultOffset(n);
                      setDefaultOffsetState(n);
                    }}
                    className={cn(
                      "rounded-full px-2.5 py-0.5 text-[11px] transition-colors",
                      defaultOffset === n
                        ? "bg-neutral-800 text-white dark:bg-white dark:text-neutral-900"
                        : "text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5",
                    )}
                  >
                    {n === 0 ? "准时" : `${n} 分钟`}
                  </button>
                ))}
              </div>
            </div>

            {/* 云同步 */}
            <div className="mt-3 border-t border-black/5 pt-3 dark:border-white/5">
              <div className="mb-1.5 flex items-center justify-between px-1">
                <div className="text-[10px] font-medium uppercase tracking-wide text-neutral-400">
                  云同步
                </div>
                <div className="flex items-center gap-1 text-[10px] text-neutral-500 dark:text-neutral-400">
                  {sync.syncing ? (
                    <>
                      <Loader2 size={11} className="animate-spin" />
                      同步中
                    </>
                  ) : sync.connected ? (
                    <>
                      <Cloud size={11} className="text-emerald-500" />
                      已连接
                    </>
                  ) : (
                    <>
                      <CloudOff size={11} />
                      未登录
                    </>
                  )}
                </div>
              </div>

              {sync.connected ? (
                <div className="space-y-1.5">
                  <div className="rounded-md bg-black/5 px-3 py-2 text-[11px] dark:bg-white/5">
                    <div className="truncate text-neutral-700 dark:text-neutral-200">
                      {sync.username || sync.userEmail}
                    </div>
                    <div className="truncate text-[10px] text-neutral-400">
                      {sync.serverUrl}
                    </div>
                    {sync.lastSyncAt && (
                      <div className="mt-0.5 text-[10px] text-neutral-400">
                        上次同步：{formatDateTime(sync.lastSyncAt)}
                      </div>
                    )}
                    {sync.error && (
                      <div className="mt-1 text-[10px] text-red-500">
                        错误：{sync.error}
                      </div>
                    )}
                  </div>
                  <div className="flex gap-1.5">
                    <ActionButton
                      onClick={() =>
                        fullSync().then(() =>
                          setToast("同步完成"),
                        ).catch((e) => setError(String(e)))
                      }
                      disabled={busy || sync.syncing}
                    >
                      立即同步
                    </ActionButton>
                    <ActionButton
                      onClick={() => {
                        syncLogout();
                        setToast("已退出同步");
                      }}
                      disabled={busy}
                    >
                      退出登录
                    </ActionButton>
                  </div>
                </div>
              ) : (
                <div className="space-y-1.5">
                  <input
                    type="text"
                    value={urlInput}
                    onChange={(e) => setUrlInput(e.target.value)}
                    placeholder="服务端 URL (如 http://192.168.10.10:8090)"
                    className="w-full rounded-md bg-black/5 px-3 py-1.5 text-[11px] outline-none placeholder:text-neutral-400 dark:bg-white/5"
                  />
                  <input
                    type="text"
                    value={emailInput}
                    onChange={(e) => setEmailInput(e.target.value)}
                    placeholder="用户名"
                    autoComplete="username"
                    className="w-full rounded-md bg-black/5 px-3 py-1.5 text-[11px] outline-none placeholder:text-neutral-400 dark:bg-white/5"
                  />
                  <input
                    type="password"
                    value={pwdInput}
                    onChange={(e) => setPwdInput(e.target.value)}
                    placeholder="密码"
                    autoComplete="current-password"
                    className="w-full rounded-md bg-black/5 px-3 py-1.5 text-[11px] outline-none placeholder:text-neutral-400 dark:bg-white/5"
                  />
                  <ActionButton
                    onClick={async () => {
                      if (!urlInput || !emailInput || !pwdInput) {
                        setError("请填写 URL、用户名、密码");
                        return;
                      }
                      setLoginBusy(true);
                      setError(null);
                      try {
                        await syncLogin(urlInput.trim(), emailInput.trim(), pwdInput);
                        setToast("登录成功，正在首次同步…");
                        setPwdInput("");
                      } catch (e) {
                        setError(String(e));
                      } finally {
                        setLoginBusy(false);
                      }
                    }}
                    disabled={loginBusy}
                  >
                    {loginBusy ? (
                      <>
                        <Loader2 size={11} className="animate-spin" />
                        登录中…
                      </>
                    ) : (
                      "登录并同步"
                    )}
                  </ActionButton>
                </div>
              )}
            </div>

            {/* 数据（导入导出） */}
            <div className="mt-3 border-t border-black/5 pt-3 dark:border-white/5">
              <div className="mb-1.5 px-1 text-[10px] font-medium uppercase tracking-wide text-neutral-400">
                数据
              </div>

              <div className="flex flex-col gap-1.5">
                <div className="flex items-center gap-1.5">
                  <ActionButton onClick={() => doExport("json")} disabled={busy}>
                    <Download size={12} />
                    导出 JSON
                  </ActionButton>
                  <ActionButton onClick={() => doExport("ical")} disabled={busy}>
                    <Download size={12} />
                    导出 .ics
                  </ActionButton>
                </div>
                <ActionButton onClick={doPickImport} disabled={busy}>
                  <Upload size={12} />
                  导入 (JSON / .ics / TickTick CSV)
                </ActionButton>
              </div>
            </div>

            {/* 检查更新 */}
            <div className="mt-3 border-t border-black/5 pt-3 dark:border-white/5">
              <div className="mb-1.5 flex items-center justify-between px-1">
                <div className="text-[10px] font-medium uppercase tracking-wide text-neutral-400">
                  关于
                </div>
                {updateStatus.state === "available" && (
                  <div className="h-1.5 w-1.5 rounded-full bg-blue-500" />
                )}
              </div>

              <div className="flex items-center justify-between rounded-lg px-3 py-2.5">
                <div className="text-xs text-neutral-800 dark:text-neutral-100">
                  版本 {appVersion}
                </div>
                <ActionButton
                  onClick={checkForUpdate}
                  disabled={updateStatus.state === "checking" || updateStatus.state === "downloading"}
                >
                  {updateStatus.state === "checking" ? (
                    <>
                      <Loader2 size={11} className="animate-spin" />
                      检查中…
                    </>
                  ) : (
                    <>
                      <RefreshCw size={11} />
                      检查更新
                    </>
                  )}
                </ActionButton>
              </div>

              {updateStatus.state === "not-available" && (
                <div className="mt-1 rounded-md bg-emerald-500/10 px-3 py-2 text-[11px] text-emerald-600 dark:text-emerald-400">
                  已是最新版本
                </div>
              )}

              {updateStatus.state === "available" && (
                <div className="mt-1 space-y-1.5 rounded-md border border-blue-500/30 bg-blue-500/10 p-3">
                  <div className="text-xs font-medium text-neutral-800 dark:text-neutral-200">
                    发现新版本 v{updateStatus.version}
                  </div>
                  {updateStatus.body && (
                    <div className="whitespace-pre-wrap text-[11px] text-neutral-600 dark:text-neutral-400">
                      {updateStatus.body}
                    </div>
                  )}
                  <ActionButton onClick={downloadAndInstall}>
                    <Download size={11} />
                    下载并安装
                  </ActionButton>
                </div>
              )}

              {updateStatus.state === "downloading" && (
                <div className="mt-1 rounded-md bg-blue-500/10 px-3 py-2 text-[11px] text-blue-600 dark:text-blue-400">
                  <div className="flex items-center gap-1.5">
                    <Loader2 size={11} className="animate-spin" />
                    下载中…
                    {updateStatus.total > 0 && (
                      <span>
                        {Math.round((updateStatus.downloaded / 1024 / 1024) * 10) / 10} /{" "}
                        {Math.round((updateStatus.total / 1024 / 1024) * 10) / 10} MB
                      </span>
                    )}
                  </div>
                </div>
              )}

              {updateStatus.state === "ready" && (
                <div className="mt-1 space-y-1.5 rounded-md border border-emerald-500/30 bg-emerald-500/10 p-3">
                  <div className="text-xs text-emerald-700 dark:text-emerald-300">
                    更新已就绪，重启生效
                  </div>
                  <ActionButton onClick={relaunchApp}>
                    <RefreshCw size={11} />
                    立即重启
                  </ActionButton>
                </div>
              )}

              {updateStatus.state === "error" && (
                <div className="mt-1 rounded-md bg-red-500/10 px-3 py-2 text-[11px] text-red-500">
                  更新失败：{updateStatus.message}
                </div>
              )}
            </div>

            {toast && (
              <div className="mt-3 rounded-md bg-emerald-500/10 px-3 py-2 text-[11px] text-emerald-600 dark:text-emerald-400">
                {toast}
              </div>
            )}

            {importPlan && (
              <div className="mt-3 rounded-md border border-blue-500/30 bg-blue-500/10 p-3">
                <div className="text-xs font-medium text-neutral-800 dark:text-neutral-200">
                  确认导入
                </div>
                {importSource && (
                  <div className="mt-0.5 truncate text-[10px] text-neutral-500">
                    源: {importSource}
                  </div>
                )}
                <div className="mt-2 space-y-0.5 text-[11px] text-neutral-700 dark:text-neutral-300">
                  <div>
                    · 新增 <b>{importPlan.toInsert.length}</b> 条
                  </div>
                  <div>
                    · 覆盖 <b>{importPlan.toUpdate.length}</b> 条（本地版本较旧）
                  </div>
                  <div>
                    · 跳过 <b>{importPlan.toSkip.length}</b> 条（本地版本更新）
                  </div>
                </div>
                <div className="mt-2 flex justify-end gap-1.5">
                  <button
                    onClick={() => {
                      setImportPlan(null);
                      setImportSource(null);
                    }}
                    className="rounded px-2 py-1 text-[11px] text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5"
                  >
                    取消
                  </button>
                  <button
                    onClick={doConfirmImport}
                    disabled={busy}
                    className="rounded bg-neutral-900 px-2 py-1 text-[11px] text-white hover:bg-neutral-700 disabled:opacity-50 dark:bg-white dark:text-neutral-900 dark:hover:bg-neutral-200"
                  >
                    确认导入
                  </button>
                </div>
              </div>
            )}

            {error && (
              <div className="mt-3 rounded-md bg-red-500/10 px-3 py-2 text-[11px] text-red-500">
                {error}
              </div>
            )}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function Row({
  label,
  desc,
  checked,
  onToggle,
  disabled,
}: {
  label: string;
  desc: string;
  checked: boolean;
  onToggle: () => void;
  disabled?: boolean;
}) {  return (
    <button
      onClick={onToggle}
      disabled={disabled}
      className={cn(
        "flex w-full items-center justify-between rounded-lg px-3 py-2.5 text-left transition-colors",
        "hover:bg-black/5 dark:hover:bg-white/5",
        disabled && "opacity-50",
      )}
    >
      <div>
        <div className="text-xs text-neutral-800 dark:text-neutral-100">
          {label}
        </div>
        <div className="mt-0.5 text-[10px] text-neutral-500">{desc}</div>
      </div>
      <div
        className={cn(
          "flex h-5 w-9 items-center rounded-full transition-colors",
          checked
            ? "bg-emerald-500"
            : "bg-neutral-300 dark:bg-neutral-600",
        )}
      >
        <div
          className="h-4 w-4 rounded-full bg-white shadow transition-transform duration-200 ease-out"
          style={{ transform: `translateX(${checked ? 18 : 2}px)` }}
        />
      </div>
    </button>
  );
}

function ActionButton({
  children,
  onClick,
  disabled,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "inline-flex flex-1 items-center justify-center gap-1.5 rounded-md bg-black/5 px-3 py-1.5 text-[11px] transition-colors",
        "hover:bg-black/10 dark:bg-white/5 dark:hover:bg-white/10",
        disabled && "opacity-50",
      )}
    >
      {children}
    </button>
  );
}
