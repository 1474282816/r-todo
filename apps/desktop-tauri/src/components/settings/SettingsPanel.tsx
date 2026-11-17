import { AnimatePresence, motion } from "framer-motion";
import {
  Cloud,
  CloudOff,
  Copy,
  Download,
  KeyRound,
  Loader2,
  LogOut,
  RefreshCw,
  Shield,
  Upload,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";

import { cn } from "../../lib/cn";
import {
  getSetting,
  importJson as importEngineJson,
  setSetting,
} from "../../lib/engine";
import { chooseExportPath, exportJsonFile, toIcal, toTickTickCsv } from "../../lib/export";
import {
  pickAndPreviewImport,
  type ImportPreview,
} from "../../lib/io_ui";
import {
  getMcpRunning,
  getMcpStatus,
  getMcpTemplatesForToken,
  rotateMcpToken,
  setMcpEnabled,
} from "../../lib/mcp";
import {
  BACKDROP_OPTIONS,
  embedToDesktop,
  getBackdropPreference,
  isAutostartEnabled,
  isEmbeddedPreference,
  setAutostart,
  setWindowBackdrop,
  unembedFromDesktop,
  type Backdrop,
} from "../../lib/system";
import { getDefaultOffset, setDefaultOffset } from "../../lib/reminder";
import { isCollapseOnSnapEnabled, setCollapseOnSnap } from "../../lib/snap";
import {
  devices as listDevices,
  fullSync,
  getStatus,
  logout,
  newPairingCode,
  pair,
  refreshStatus,
  revoke,
  subscribeStatus,
} from "../../lib/sync";
import { getTransferState, openTransferWindow, startTransfer, stopTransfer } from "../../lib/transfer";
import {
  checkForUpdate,
  downloadAndInstall,
  getAppVersion,
  relaunchApp,
  subscribeUpdateStatus,
  type UpdateStatus,
} from "../../lib/updater";
import { useTaskStore } from "../../lib/store";
import type { Device, McpTemplates, SyncStatus } from "../../lib/types";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SettingsPanel({ open, onClose }: Props) {
  const tasks = useTaskStore((state) => state.tasks);
  const add = useTaskStore((state) => state.add);
  const update = useTaskStore((state) => state.update);

  const [embedded, setEmbedded] = useState(isEmbeddedPreference());
  const [backdrop, setBackdropState] = useState<Backdrop>(getBackdropPreference());
  const [autostart, setAutostartState] = useState(false);
  const [collapse, setCollapseState] = useState(isCollapseOnSnapEnabled());
  const [defaultOffset, setDefaultOffsetState] = useState(getDefaultOffset());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const [mcpEnabled, setMcpEnabledState] = useState(false);
  const [mcpToken, setMcpToken] = useState("");
  const [mcpRunning, setMcpRunningState] = useState(false);
  const [mcpError, setMcpError] = useState<string | null>(null);
  const [templates, setTemplates] = useState<McpTemplates | null>(null);
  const [activeTemplate, setActiveTemplate] = useState<keyof McpTemplates>("claude_code");

  const [transferEnabled, setTransferEnabled] = useState(false);
  const [transferBusy, setTransferBusy] = useState(false);
  const [transferError, setTransferError] = useState<string | null>(null);

  const [sync, setSync] = useState<SyncStatus>(getStatus());
  const [urlInput, setUrlInput] = useState("https://todo.renrunqi.com");
  const [codeInput, setCodeInput] = useState("");
  const [deviceName, setDeviceName] = useState("RTools Windows");
  const [syncBusy, setSyncBusy] = useState(false);
  const [generatedCode, setGeneratedCode] = useState<{ code: string; expires_at: string } | null>(
    null,
  );
  const [deviceList, setDeviceList] = useState<Device[]>([]);

  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ state: "idle" });
  const [version, setVersion] = useState("...");
  const [importPreview, setImportPreview] = useState<ImportPreview | null>(null);
  const [importBusy, setImportBusy] = useState(false);

  useEffect(() => subscribeStatus(setSync), []);
  useEffect(() => subscribeUpdateStatus(setUpdateStatus), []);
  useEffect(() => {
    getAppVersion().then(setVersion);
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    if (!open) return;
    void (async () => {
      try {
        const [
          widget,
          material,
          collapseOnSnap,
          storedOffset,
        ] = await Promise.all([
          getSetting("desktop_widget"),
          getSetting("material"),
          getSetting("collapse_on_snap"),
          getSetting("default_reminder_offset"),
        ]);
        if (widget !== null) setEmbedded(widget === "true");
        if (material) {
          const matched = BACKDROP_OPTIONS.find((item) => item.value === material)?.value;
          if (matched) setBackdropState(matched);
        }
        if (collapseOnSnap !== null) setCollapseState(collapseOnSnap !== "false");
        if (storedOffset) {
          const parsed = Number(storedOffset);
          if (Number.isFinite(parsed)) {
            setDefaultOffsetState(parsed);
            setDefaultOffset(parsed);
          }
        }
        setAutostartState(await isAutostartEnabled().catch(() => false));
      } catch (loadError) {
        setError(String(loadError));
      }
    })();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    void (async () => {
      try {
        await refreshStatus();
        setDeviceList(await listDevices());
      } catch (syncError) {
        setError(String(syncError));
      }
    })();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    void loadMcp();
    void loadTransfer();
  }, [open]);

  async function loadMcp() {
    try {
      const status = await getMcpStatus();
      setMcpEnabledState(status.enabled);
      setMcpToken(status.token);
      const running = await getMcpRunning();
      setMcpRunningState(running.running);
      setMcpError(running.error);
      setTemplates(await getMcpTemplatesForToken(status.token));
    } catch (mcpLoadError) {
      setMcpError(String(mcpLoadError));
    }
  }

  async function loadTransfer() {
    try {
      setTransferEnabled((await getTransferState()).running);
    } catch (transferLoadError) {
      setTransferError(String(transferLoadError));
    }
  }

  async function toggleEmbed() {
    setBusy(true);
    setError(null);
    try {
      if (embedded) {
        await unembedFromDesktop();
        setEmbedded(false);
        await setSetting("desktop_widget", "false");
      } else {
        await embedToDesktop();
        setEmbedded(true);
        await setSetting("desktop_widget", "true");
      }
    } catch (toggleError) {
      setError(String(toggleError));
    } finally {
      setBusy(false);
    }
  }

  async function chooseBackdrop(next: Backdrop) {
    setBackdropState(next);
    try {
      await setWindowBackdrop(next);
      await setSetting("material", next);
    } catch (backdropError) {
      setError(String(backdropError));
    }
  }

  async function toggleAutostart() {
    setBusy(true);
    try {
      await setAutostart(!autostart);
      setAutostartState(!autostart);
      await setSetting("autostart", (!autostart).toString());
    } catch (autostartError) {
      setError(String(autostartError));
    } finally {
      setBusy(false);
    }
  }

  async function toggleCollapse() {
    setCollapseOnSnap(!collapse);
    setCollapseState(!collapse);
    await setSetting("collapse_on_snap", (!collapse).toString());
  }

  async function changeDefaultOffset(minutes: number) {
    setDefaultOffset(minutes);
    setDefaultOffsetState(minutes);
    await setSetting("default_reminder_offset", String(minutes));
  }

  async function toggleMcp() {
    try {
      setMcpError(null);
      const status = await setMcpEnabled(!mcpEnabled);
      setMcpEnabledState(status.enabled);
      setMcpToken(status.token);
      setMcpRunningState(status.running);
      setMcpError(status.error);
      if (status.enabled) {
        setTemplates(await getMcpTemplatesForToken(status.token));
      }
    } catch (mcpToggleError) {
      setMcpError(String(mcpToggleError));
    }
  }

  async function rotateToken() {
    try {
      const status = await rotateMcpToken();
      setMcpToken(status.token);
      setTemplates(await getMcpTemplatesForToken(status.token));
      setToast("MCP token 已重置");
    } catch (mcpRotateError) {
      setMcpError(String(mcpRotateError));
    }
  }

  async function toggleTransfer() {
    setTransferBusy(true);
    setTransferError(null);
    try {
      if (transferEnabled) {
        await stopTransfer();
        setTransferEnabled(false);
      } else {
        await startTransfer();
        setTransferEnabled(true);
      }
    } catch (transferToggleError) {
      setTransferError(String(transferToggleError));
    } finally {
      setTransferBusy(false);
    }
  }

  async function doPair() {
    setSyncBusy(true);
    setError(null);
    try {
      const next = await pair(urlInput.trim(), codeInput.trim(), deviceName.trim());
      setSync(next);
      setDeviceList(await listDevices());
      setCodeInput("");
      setToast("设备配对成功");
    } catch (pairError) {
      setError(String(pairError));
    } finally {
      setSyncBusy(false);
    }
  }

  async function doSync() {
    setSyncBusy(true);
    try {
      setSync(await fullSync());
      setToast("同步完成");
    } catch (syncError) {
      setError(String(syncError));
    } finally {
      setSyncBusy(false);
    }
  }

  async function doLogout() {
    setSyncBusy(true);
    try {
      setSync(await logout());
      setDeviceList([]);
      setToast("已退出同步");
    } catch (logoutError) {
      setError(String(logoutError));
    } finally {
      setSyncBusy(false);
    }
  }

  async function makeCode() {
    try {
      const code = await newPairingCode();
      setGeneratedCode(code);
    } catch (codeError) {
      setError(String(codeError));
    }
  }

  async function removeDevice(device: Device) {
    const { confirm } = await import("@tauri-apps/plugin-dialog");
    if (!(await confirm(`撤销设备“${device.name}”？`, { title: "撤销设备", kind: "warning" }))) {
      return;
    }
    try {
      setDeviceList(await revoke(device.id));
      setToast("设备已撤销");
    } catch (revokeError) {
      setError(String(revokeError));
    }
  }

  async function exportJson() {
    try {
      if (await exportJsonFile()) setToast("JSON 已导出");
    } catch (exportError) {
      setError(String(exportError));
    }
  }

  async function exportIcal() {
    setBusy(true);
    try {
      const path = await chooseExportPath("ics");
      if (!path) return;
      const { writeTextFile } = await import("@tauri-apps/plugin-fs");
      await writeTextFile(path, toIcal(tasks));
      setToast("iCalendar 已导出");
    } catch (exportError) {
      setError(String(exportError));
    } finally {
      setBusy(false);
    }
  }

  async function exportCsv() {
    setBusy(true);
    try {
      const path = await chooseExportPath("csv");
      if (!path) return;
      const { writeTextFile } = await import("@tauri-apps/plugin-fs");
      await writeTextFile(path, toTickTickCsv(tasks));
      setToast("TickTick CSV 已导出");
    } catch (exportError) {
      setError(String(exportError));
    } finally {
      setBusy(false);
    }
  }

  async function chooseImport() {
    setImportBusy(true);
    try {
      const preview = await pickAndPreviewImport(tasks);
      if (!preview) return;
      if (preview.path.toLowerCase().endsWith(".json")) {
        const imported = await importEngineJson(preview.path);
        setToast(`已导入 ${imported} 条任务`);
        await useTaskStore.getState().bulkImport();
      } else {
        setImportPreview(preview);
      }
    } catch (importError) {
      setError(String(importError));
    } finally {
      setImportBusy(false);
    }
  }

  async function confirmLocalImport() {
    if (!importPreview) return;
    setImportBusy(true);
    try {
      for (const task of importPreview.plan.toInsert) {
        await add({
          title: task.title,
          priority: task.priority,
          remind_at: task.remind_at,
          remind_offset_min: task.remind_offset_min,
          repeat_rule: task.repeat_rule,
          completed_at: task.completed_at,
        });
      }
      for (const task of importPreview.plan.toUpdate) {
        await update(task.id, {
          title: task.title,
          priority: task.priority,
          remind_at: task.remind_at,
          remind_offset_min: task.remind_offset_min,
          repeat_rule: task.repeat_rule,
          completed_at: task.completed_at,
        });
      }
      setToast(
        `已导入 · 新增 ${importPreview.plan.toInsert.length} · 覆盖 ${importPreview.plan.toUpdate.length}`,
      );
      setImportPreview(null);
    } catch (importError) {
      setError(String(importError));
    } finally {
      setImportBusy(false);
    }
  }

  async function copy(value: string, label: string) {
    try {
      await navigator.clipboard.writeText(value);
      setToast(`${label}已复制`);
    } catch {
      setError("复制失败");
    }
  }

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
            onClick={(event) => event.stopPropagation()}
            className="max-h-[92vh] w-full overflow-y-auto rounded-t-2xl bg-white/95 p-4 shadow-2xl dark:bg-neutral-900/95"
          >
            <div className="mb-3 flex items-center justify-between">
              <div>
                <h2 className="text-sm font-semibold text-neutral-700 dark:text-neutral-200">
                  设置
                </h2>
                <p className="mt-0.5 text-[10px] text-neutral-400">
                  RTools {version}
                </p>
              </div>
              <button
                onClick={onClose}
                className="rounded p-1 text-neutral-400 hover:bg-black/5 hover:text-neutral-700 dark:hover:bg-white/10 dark:hover:text-neutral-200"
                aria-label="关闭设置"
              >
                <X size={16} />
              </button>
            </div>

            <Section title="桌面挂件">
              <Row
                label="嵌入桌面"
                desc="挂靠壁纸层，Win+D 不隐藏"
                checked={embedded}
                onToggle={toggleEmbed}
                disabled={busy}
              />
              <Row
                label="吸附后收起为边条"
                desc="鼠标进入边条时自动展开"
                checked={collapse}
                onToggle={toggleCollapse}
              />
              <Row
                label="开机自启"
                desc="登录 Windows 后自动打开"
                checked={autostart}
                onToggle={toggleAutostart}
                disabled={busy}
              />
              <div className="rounded-xl bg-black/[0.035] p-3 dark:bg-white/[0.045]">
                <div className="text-[12px] font-medium text-neutral-700 dark:text-neutral-200">
                  窗口材质
                </div>
                <div className="mt-2 grid grid-cols-3 gap-1.5">
                  {BACKDROP_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      onClick={() => void chooseBackdrop(option.value)}
                      className={cn(
                        "rounded-md px-2 py-1.5 text-[11px] transition-colors",
                        backdrop === option.value
                          ? "bg-neutral-900 text-white dark:bg-white dark:text-neutral-900"
                          : "bg-black/5 text-neutral-500 hover:bg-black/10 dark:bg-white/10 dark:text-neutral-300 dark:hover:bg-white/15",
                      )}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>
            </Section>

            <Section title="提醒">
              <div className="rounded-xl px-3 py-2.5">
                <div className="text-[12px] text-neutral-700 dark:text-neutral-200">
                  默认提前量
                </div>
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {[0, 5, 10, 15, 30, 60, 120].map((minutes) => (
                    <button
                      key={minutes}
                      onClick={() => void changeDefaultOffset(minutes)}
                      className={cn(
                        "rounded-full px-2.5 py-1 text-[11px] transition-colors",
                        defaultOffset === minutes
                          ? "bg-neutral-900 text-white dark:bg-white dark:text-neutral-900"
                          : "bg-black/5 text-neutral-500 hover:bg-black/10 dark:bg-white/10 dark:text-neutral-300",
                      )}
                    >
                      {minutes === 0 ? "准时" : `${minutes} 分钟`}
                    </button>
                  ))}
                </div>
              </div>
            </Section>

            <Section title="同步">
              <div className="flex items-center justify-between rounded-xl bg-black/[0.035] px-3 py-2.5 dark:bg-white/[0.045]">
                <div>
                  <div className="text-[12px] text-neutral-700 dark:text-neutral-200">
                    {sync.paired ? "设备已配对" : "设备未配对"}
                  </div>
                  <div className="mt-0.5 text-[10px] text-neutral-400">
                    {sync.connected ? "已连接" : sync.paired ? "当前离线" : "输入一次性配对码接入"}
                  </div>
                </div>
                {sync.connected ? (
                  <Cloud size={15} className="text-emerald-500" />
                ) : (
                  <CloudOff size={15} className="text-neutral-400" />
                )}
              </div>

              {!sync.paired ? (
                <div className="space-y-1.5">
                  <input
                    value={urlInput}
                    onChange={(event) => setUrlInput(event.target.value)}
                    placeholder="同步服务 URL"
                    className={inputClass}
                  />
                  <input
                    value={codeInput}
                    onChange={(event) => setCodeInput(event.target.value.toUpperCase())}
                    placeholder="一次性配对码"
                    className={inputClass}
                  />
                  <input
                    value={deviceName}
                    onChange={(event) => setDeviceName(event.target.value)}
                    placeholder="设备名称"
                    className={inputClass}
                  />
                  <ActionButton
                    onClick={doPair}
                    disabled={syncBusy || !codeInput.trim() || !urlInput.trim()}
                  >
                    <KeyRound size={12} /> 配对并同步
                  </ActionButton>
                </div>
              ) : (
                <div className="space-y-1.5">
                  <div className="rounded-lg bg-black/5 px-3 py-2 text-[11px] dark:bg-white/5">
                    <div className="truncate text-neutral-600 dark:text-neutral-300">
                      {sync.server_url}
                    </div>
                    <div className="mt-0.5 text-[10px] text-neutral-400">
                      {sync.last_sync_at
                        ? `上次同步 ${new Date(sync.last_sync_at).toLocaleString()}`
                        : "尚未同步"}
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-1.5">
                    <ActionButton onClick={doSync} disabled={syncBusy}>
                      <RefreshCw size={12} /> 立即同步
                    </ActionButton>
                    <ActionButton onClick={doLogout} disabled={syncBusy}>
                      <LogOut size={12} /> 退出同步
                    </ActionButton>
                  </div>
                  <ActionButton onClick={makeCode}>
                    <Shield size={12} /> 生成新设备配对码
                  </ActionButton>
                  {generatedCode && (
                    <button
                      onClick={() => void copy(generatedCode.code, "配对码")}
                      className="flex w-full items-center justify-center gap-2 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 font-mono text-[12px] text-emerald-700 dark:text-emerald-300"
                    >
                      {generatedCode.code}
                      <Copy size={11} />
                    </button>
                  )}
                </div>
              )}

              {deviceList.length > 0 && (
                <div className="space-y-1">
                  {deviceList.map((device) => (
                    <div
                      key={device.id}
                      className="flex items-center justify-between rounded-lg bg-black/5 px-3 py-2 text-[11px] dark:bg-white/5"
                    >
                      <div className="min-w-0">
                        <div className="truncate text-neutral-700 dark:text-neutral-200">
                          {device.name}
                        </div>
                        <div className="text-[9px] text-neutral-400">
                          {device.last_used_at
                            ? `最近使用 ${new Date(device.last_used_at).toLocaleDateString()}`
                            : "尚未使用"}
                        </div>
                      </div>
                      <button
                        onClick={() => void removeDevice(device)}
                        className="rounded px-2 py-1 text-[10px] text-red-500 hover:bg-red-500/10"
                      >
                        撤销
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </Section>

            <Section title="AI 与互传">
              <Row
                label="MCP 服务器"
                desc="允许 AI 读取和修改待办"
                checked={mcpEnabled}
                onToggle={toggleMcp}
                disabled={busy}
              />
              {mcpEnabled && (
                <div className="rounded-xl bg-black/[0.035] p-3 dark:bg-white/[0.045]">
                  <div className="flex items-center gap-2">
                    <span
                      className={cn(
                        "h-1.5 w-1.5 rounded-full",
                        mcpRunning ? "bg-emerald-500" : "bg-neutral-400",
                      )}
                    />
                    <span className="text-[10px] text-neutral-500">
                      {mcpRunning ? "MCP 运行中" : "MCP 未运行"}
                    </span>
                  </div>
                  <div className="mt-2 flex gap-1.5">
                    <ActionButton onClick={() => void copy(mcpToken, "token")}>
                      <Copy size={11} /> 复制 token
                    </ActionButton>
                    <ActionButton onClick={rotateToken}>
                      <RefreshCw size={11} /> 重置 token
                    </ActionButton>
                  </div>
                  {templates && (
                    <>
                      <div className="mt-2 flex flex-wrap gap-1">
                        {(Object.keys(templates) as (keyof McpTemplates)[]).map((key) => (
                          <button
                            key={key}
                            onClick={() => setActiveTemplate(key)}
                            className={cn(
                              "rounded-full px-2 py-1 text-[10px]",
                              activeTemplate === key
                                ? "bg-neutral-900 text-white dark:bg-white dark:text-neutral-900"
                                : "bg-black/5 text-neutral-500 dark:bg-white/10",
                            )}
                          >
                            {templateLabel(key)}
                          </button>
                        ))}
                      </div>
                      <pre className="mt-2 max-h-32 overflow-auto whitespace-pre-wrap rounded-lg bg-white/70 p-2 font-mono text-[9px] text-neutral-600 dark:bg-black/30 dark:text-neutral-300">
                        {templates[activeTemplate]}
                      </pre>
                    </>
                  )}
                  {mcpError && (
                    <div className="mt-2 text-[10px] text-red-500">{mcpError}</div>
                  )}
                </div>
              )}

              <Row
                label="局域网互传"
                desc="LocalSend 兼容的文件与聊天互传"
                checked={transferEnabled}
                onToggle={toggleTransfer}
                disabled={transferBusy}
              />
              {transferEnabled && (
                <ActionButton onClick={() => void openTransferWindow()}>
                  打开互传窗口
                </ActionButton>
              )}
              {transferError && (
                <div className="text-[10px] text-red-500">{transferError}</div>
              )}
            </Section>

            <Section title="数据">
              <div className="grid grid-cols-2 gap-1.5">
                <ActionButton onClick={exportJson} disabled={busy}>
                  <Download size={12} /> 导出 JSON
                </ActionButton>
                <ActionButton onClick={exportIcal} disabled={busy}>
                  <Download size={12} /> 导出 .ics
                </ActionButton>
                <ActionButton onClick={exportCsv} className="col-span-2" disabled={busy}>
                  <Download size={12} /> 导出 TickTick CSV
                </ActionButton>
              </div>
              <ActionButton onClick={chooseImport} disabled={importBusy}>
                <Upload size={12} /> 导入 JSON / ICS / TickTick CSV
              </ActionButton>
              {importPreview && (
                <div className="rounded-xl border border-blue-500/30 bg-blue-500/10 p-3">
                  <div className="text-[12px] font-medium text-neutral-700 dark:text-neutral-200">
                    确认导入
                  </div>
                  <div className="mt-1 text-[11px] text-neutral-500">
                    新增 {importPreview.plan.toInsert.length} · 覆盖{" "}
                    {importPreview.plan.toUpdate.length} · 跳过{" "}
                    {importPreview.plan.toSkip.length}
                  </div>
                  <div className="mt-2 flex justify-end gap-1.5">
                    <button
                      onClick={() => setImportPreview(null)}
                      className="rounded-md px-2.5 py-1.5 text-[11px] text-neutral-500 hover:bg-black/5 dark:hover:bg-white/5"
                    >
                      取消
                    </button>
                    <button
                      onClick={() => void confirmLocalImport()}
                      className="rounded-md bg-neutral-900 px-2.5 py-1.5 text-[11px] text-white dark:bg-white dark:text-neutral-900"
                    >
                      确认导入
                    </button>
                  </div>
                </div>
              )}
            </Section>

            <Section title="关于">
              <div className="flex items-center justify-between rounded-xl px-3 py-2.5">
                <span className="text-[12px] text-neutral-700 dark:text-neutral-200">
                  RTools {version}
                </span>
                <ActionButton onClick={checkForUpdate} className="max-w-36">
                  {updateStatus.state === "checking" ? (
                    <>
                      <Loader2 size={11} className="animate-spin" /> 检查中
                    </>
                  ) : (
                    <>
                      <RefreshCw size={11} /> 检查更新
                    </>
                  )}
                </ActionButton>
              </div>
              {updateStatus.state === "not-available" && (
                <Info tone="success">当前已是最新版本</Info>
              )}
              {updateStatus.state === "available" && (
                <Alert>
                  <div className="font-medium">发现新版本 {updateStatus.version}</div>
                  {updateStatus.body && (
                    <div className="mt-1 whitespace-pre-wrap">{updateStatus.body}</div>
                  )}
                  <ActionButton onClick={downloadAndInstall}>下载并安装</ActionButton>
                </Alert>
              )}
              {updateStatus.state === "ready" && (
                <Alert>
                  <div>更新已就绪</div>
                  <ActionButton onClick={relaunchApp}>立即重启</ActionButton>
                </Alert>
              )}
              {updateStatus.state === "error" && (
                <Info tone="error">更新失败：{updateStatus.message}</Info>
              )}
            </Section>

            {error && <Info tone="error">{error}</Info>}
            {toast && <Info tone="success">{toast}</Info>}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function templateLabel(key: keyof McpTemplates): string {
  switch (key) {
    case "claude_code":
      return "Claude Code";
    case "claude_desktop":
      return "Claude Desktop";
    case "opencode":
      return "OpenCode";
    case "codex":
      return "Codex";
  }
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-3 border-t border-black/5 pt-3 first:border-0 first:pt-0 dark:border-white/5">
      <div className="mb-1.5 px-2 text-[10px] font-medium uppercase tracking-[0.14em] text-neutral-400">
        {title}
      </div>
      <div className="space-y-1.5">{children}</div>
    </section>
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
}) {
  return (
    <button
      onClick={onToggle}
      disabled={disabled}
      className="flex w-full items-center justify-between rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-black/5 disabled:opacity-50 dark:hover:bg-white/5"
    >
      <div>
        <div className="text-[12px] text-neutral-800 dark:text-neutral-100">{label}</div>
        <div className="mt-0.5 text-[10px] text-neutral-500 dark:text-neutral-400">{desc}</div>
      </div>
      <span
        className={cn(
          "flex h-5 w-9 items-center rounded-full transition-colors",
          checked ? "bg-emerald-500" : "bg-neutral-300 dark:bg-neutral-600",
        )}
      >
        <span
          className="h-4 w-4 rounded-full bg-white shadow transition-transform duration-200"
          style={{ transform: `translateX(${checked ? 18 : 2}px)` }}
        />
      </span>
    </button>
  );
}

function ActionButton({
  children,
  onClick,
  disabled,
  className,
}: {
  children: React.ReactNode;
  onClick: () => void | Promise<void>;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      onClick={() => void onClick()}
      disabled={disabled}
      className={cn(
        "inline-flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-black/[0.045] px-3 py-2 text-[11px] text-neutral-600 transition-colors hover:bg-black/[0.075] disabled:opacity-45 dark:bg-white/[0.055] dark:text-neutral-300 dark:hover:bg-white/[0.09]",
        className,
      )}
    >
      {children}
    </button>
  );
}

function Info({ tone, children }: { tone: "success" | "error"; children: React.ReactNode }) {
  return (
    <div
      className={cn(
        "mt-2 rounded-lg px-3 py-2 text-[10px]",
        tone === "success"
          ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
          : "bg-red-500/10 text-red-600 dark:text-red-400",
      )}
    >
      {children}
    </div>
  );
}

function Alert({ children }: { children: React.ReactNode }) {
  return (
    <div className="mt-2 space-y-2 rounded-lg border border-blue-500/30 bg-blue-500/10 p-3 text-[11px] text-neutral-600 dark:text-neutral-300">
      {children}
    </div>
  );
}

const inputClass =
  "w-full rounded-lg bg-black/5 px-3 py-2 text-[11px] text-neutral-700 outline-none placeholder:text-neutral-400 focus:bg-black/[0.07] dark:bg-white/10 dark:text-neutral-200 dark:placeholder:text-neutral-500 dark:focus:bg-white/[0.14]";
