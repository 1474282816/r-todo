import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

import {
  addManualIp,
  cancelTransfer,
  discoverDevices,
  errorLabel,
  getPendingIncoming,
  getTransferState,
  rejectTransfer,
  respondPrepare,
  sendFiles,
  startTransfer,
  stopTransfer,
  subscribeTransfer,
  updateTransferSettings,
  type TransferDevice,
  type TransferDirection,
  type TransferEventPayload,
  type TransferSessionEndReason,
  type TransferSettings,
  type TransferStateView,
} from "../../lib/transfer";

export interface FileProgress {
  id: string;
  name: string;
  size: number;
  transferred: number;
  done: boolean;
  failed: boolean;
  path?: string;
}

export interface SessionView {
  id: string;
  direction: TransferDirection;
  peer: string;
  ip?: string;
  files: FileProgress[];
  /** 待用户确认的接收请求 */
  pending: boolean;
  ended?: TransferSessionEndReason;
  error?: string;
}

export interface Notice {
  kind: "ok" | "err";
  text: string;
}

const MAX_FINISHED_SESSIONS = 6;

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** 窗口在后台时给系统通知，前台只用窗口内 toast，避免重复打扰 */
async function systemNotify(title: string, body: string) {
  try {
    if (document.visibilityState === "visible" && document.hasFocus()) return;

    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendNotification({ title, body });
  } catch {
    // 通知不可用不影响传输
  }
}

export function useTransfer() {
  const [state, setState] = useState<TransferStateView | null>(null);
  const [devices, setDevices] = useState<TransferDevice[]>([]);
  const [sessions, setSessions] = useState<SessionView[]>([]);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [discovering, setDiscovering] = useState(false);

  // 事件回调里需要读最新 sessions，但不应触发重新订阅
  const sessionsRef = useRef<SessionView[]>([]);
  useEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);

  // 已发起但还没收到第一条事件（尚无 session id）的发送请求，FIFO 匹配
  const pendingSendsRef = useRef<{ peer: string; paths: string[] }[]>([]);

  /** 事件比 invoke 返回更早到达时，用待发队列补出会话 */
  const ensureSendSession = useCallback((sessionId: string, peerHint?: string) => {
    if (sessionsRef.current.some((s) => s.id === sessionId)) return;

    const pending = pendingSendsRef.current[0];
    if (!pending) return;
    pendingSendsRef.current = pendingSendsRef.current.slice(1);

    const session: SessionView = {
      id: sessionId,
      direction: "send",
      peer: pending.peer || peerHint || "对方设备",
      pending: false,
      files: pending.paths.map((path) => ({
        id: "",
        name: baseName(path),
        size: 0,
        transferred: 0,
        done: false,
        failed: false,
      })),
    };

    sessionsRef.current = [session, ...sessionsRef.current];
    setSessions((prev) => [session, ...prev]);
  }, []);

  const notify = useCallback((kind: Notice["kind"], text: string) => {
    setNotice({ kind, text });
  }, []);

  useEffect(() => {
    if (!notice) return;
    const t = window.setTimeout(() => setNotice(null), 3200);
    return () => window.clearTimeout(t);
  }, [notice]);

  const refresh = useCallback(async () => {
    try {
      const view = await getTransferState();
      setState(view);
      setError(view.error);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // 会话更新工具：同 id 合并，finished 会话最多保留若干个
  const upsertSession = useCallback(
    (id: string, patch: (session: SessionView) => SessionView) => {
      setSessions((prev) => {
        const index = prev.findIndex((s) => s.id === id);
        if (index < 0) return prev;
        const next = [...prev];
        next[index] = patch(next[index]);
        return next;
      });
    },
    [],
  );

  const handleEvent = useCallback(
    (payload: TransferEventPayload) => {
      switch (payload.type) {
        case "devices":
          setDevices(payload.devices ?? []);
          break;
        case "incoming": {
          const session: SessionView = {
            id: payload.session_id,
            direction: "receive",
            peer: payload.device.alias || payload.ip,
            ip: payload.ip,
            pending: true,
            files: payload.files.map((f) => ({
              id: f.id,
              name: f.file_name,
              size: f.size,
              transferred: 0,
              done: false,
              failed: false,
            })),
          };
          setSessions((prev) => [session, ...prev.filter((s) => s.id !== session.id)]);
          break;
        }
        case "progress": {
          if (payload.direction === "send") ensureSendSession(payload.session_id);
          upsertSession(payload.session_id, (session) => {
            const files = [...session.files];
            let index = files.findIndex((f) => f.id === payload.file_id);
            if (index < 0) {
              // 发送侧创建会话时还不知道 file id：按顺序补位
              index = files.findIndex((f) => f.id === "");
              if (index < 0) {
                files.push({
                  id: payload.file_id,
                  name: `文件 ${files.length + 1}`,
                  size: payload.total,
                  transferred: 0,
                  done: false,
                  failed: false,
                });
                index = files.length - 1;
              }
            }
            const target = files[index];
            files[index] = {
              ...target,
              id: payload.file_id,
              size: payload.total || target.size,
              transferred: payload.transferred,
              done: payload.percent >= 100,
            };
            return { ...session, files };
          });
          break;
        }
        case "completed": {
          if (payload.direction === "send") ensureSendSession(payload.session_id);
          upsertSession(payload.session_id, (session) => {
            const files = session.files.map((f) =>
              f.id === payload.file_id
                ? { ...f, done: true, transferred: f.size, path: payload.path }
                : f,
            );
            return { ...session, files };
          });
          if (payload.direction === "receive" && payload.path) {
            notify("ok", `已保存 ${baseName(payload.path)}`);
          }
          break;
        }
        case "error": {
          // core 已经给了中文文案时直接展示，避免出现“对方拒绝：对方拒绝了本次传输”
          const hasCjk = /[\u4e00-\u9fa5]/.test(payload.message);
          const text = hasCjk
            ? payload.message
            : `${errorLabel(payload.code)}：${payload.message}`;
          setError(text);
          notify("err", text);
          break;
        }
        case "session_ended": {
          upsertSession(payload.session_id, (session) => ({
            ...session,
            pending: false,
            ended: payload.reason,
          }));
          if (payload.reason === "completed") {
            notify("ok", "传输完成");
            const session = sessionsRef.current.find((s) => s.id === payload.session_id);
            if (session?.direction === "receive") {
              const count = session.files.length;
              systemNotify("互传完成", `已接收 ${count} 个文件`);
            }
          } else if (payload.reason === "cancelled") {
            notify("err", "传输已取消");
          } else if (payload.reason === "rejected") {
            notify("err", "已拒绝接收");
          }
          // 清理过期的已完成会话
          setSessions((prev) => {
            const finished = prev.filter((s) => s.ended);
            if (finished.length <= MAX_FINISHED_SESSIONS) return prev;
            const drop = new Set(
              finished.slice(0, finished.length - MAX_FINISHED_SESSIONS).map((s) => s.id),
            );
            return prev.filter((s) => !drop.has(s.id));
          });
          break;
        }
        default:
          break;
      }
    },
    [ensureSendSession, notify, upsertSession],
  );

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let cancelled = false;

    subscribeTransfer((payload) => {
      if (!cancelled) handleEvent(payload);
    })
      .then((fn) => {
        if (cancelled) fn();
        else dispose = fn;
      })
      .catch((e) => setError(String(e)));

    refresh();

    // 窗口重开/前端重载时，把仍在等待确认的请求补回弹窗
    getPendingIncoming()
      .then((list) => {
        if (cancelled || list.length === 0) return;
        setSessions((prev) => {
          const existing = new Set(prev.map((s) => s.id));
          const added: SessionView[] = list
            .filter((p) => !existing.has(p.session_id))
            .map((p) => ({
              id: p.session_id,
              direction: "receive",
              peer: p.device.alias || p.ip,
              ip: p.ip,
              pending: true,
              files: p.files.map((f) => ({
                id: f.id,
                name: f.file_name,
                size: f.size,
                transferred: 0,
                done: false,
                failed: false,
              })),
            }));
          return added.length > 0 ? [...added, ...prev] : prev;
        });
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      dispose?.();
    };
  }, [handleEvent, refresh]);

  const start = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const info = await startTransfer();
      setState((prev) =>
        prev
          ? { ...prev, ...info, settings: { ...prev.settings, enabled: true }, error: null }
          : prev,
      );
      await discoverDevices().catch(() => undefined);
      await refresh();
    } catch (e) {
      setError(String(e));
      notify("err", String(e));
    } finally {
      setBusy(false);
    }
  }, [notify, refresh]);

  const stop = useCallback(async () => {
    setBusy(true);
    try {
      await stopTransfer();
      setDevices([]);
      setState((prev) =>
        prev ? { ...prev, running: false, settings: { ...prev.settings, enabled: false } } : prev,
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const discover = useCallback(async () => {
    setDiscovering(true);
    setError(null);
    try {
      await discoverDevices();
    } catch (e) {
      setError(String(e));
    } finally {
      setDiscovering(false);
    }
  }, []);

  const addIp = useCallback(
    async (host: string, port?: number) => {
      if (!host.trim()) return;
      setBusy(true);
      setError(null);
      try {
        const device = await addManualIp(host.trim(), port);
        if (!device) {
          notify("err", "该地址没有响应（可能不是 LocalSend 服务）");
        }
      } catch (e) {
        setError(String(e));
        notify("err", String(e));
      } finally {
        setBusy(false);
      }
    },
    [notify],
  );

  const sendTo = useCallback(
    async (device: TransferDevice, files: string[], pin?: string) => {
      if (files.length === 0) return;
      setBusy(true);
      setError(null);

      // 先登记待发，进度事件可能比 invoke 返回更早到达
      const entry = { peer: device.alias, paths: files };
      pendingSendsRef.current = [...pendingSendsRef.current, entry];

      try {
        const sessionId = await sendFiles(files, device.fingerprint, pin);
        ensureSendSession(sessionId, device.alias);
        // 兜底：对端 204 / 立刻失败时不会再有进度事件，过期清理
        window.setTimeout(() => {
          pendingSendsRef.current = pendingSendsRef.current.filter((e) => e !== entry);
        }, 2000);
        return true;
      } catch (e) {
        pendingSendsRef.current = pendingSendsRef.current.filter((e) => e !== entry);
        const text = String(e);
        setError(text);
        notify("err", text);
        return false;
      } finally {
        setBusy(false);
      }
    },
    [ensureSendSession, notify],
  );

  const confirmReceive = useCallback(
    async (session: SessionView, acceptedIds: string[]) => {
      try {
        await respondPrepare(session.id, acceptedIds);
        upsertSession(session.id, (s) => ({ ...s, pending: false }));
      } catch (e) {
        setError(String(e));
        notify("err", String(e));
      }
    },
    [notify, upsertSession],
  );

  const rejectReceive = useCallback(
    async (session: SessionView) => {
      try {
        await rejectTransfer(session.id);
        upsertSession(session.id, (s) => ({
          ...s,
          pending: false,
          ended: "rejected",
        }));
      } catch (e) {
        setError(String(e));
      }
    },
    [upsertSession],
  );

  const cancelSession = useCallback(
    async (session: SessionView) => {
      try {
        await cancelTransfer(session.id);
      } catch (e) {
        setError(String(e));
      }
    },
    [],
  );

  const saveSettings = useCallback(
    async (patch: Partial<TransferSettings>) => {
      setBusy(true);
      setError(null);
      try {
        const settings = await updateTransferSettings({
          alias: patch.alias,
          port: patch.port,
          receiveDir: patch.receiveDir,
          pin: patch.pin ?? undefined,
          autoAcceptFavorites: patch.autoAcceptFavorites,
        });
        setState((prev) => (prev ? { ...prev, settings } : prev));
        notify("ok", "设置已保存");
        await refresh();
      } catch (e) {
        setError(String(e));
        notify("err", String(e));
      } finally {
        setBusy(false);
      }
    },
    [notify, refresh],
  );

  const pendingIncoming = useMemo(
    () => sessions.find((s) => s.pending) ?? null,
    [sessions],
  );

  const activeSessions = useMemo(
    () => sessions.filter((s) => !s.pending),
    [sessions],
  );

  return {
    state,
    devices,
    sessions: activeSessions,
    pendingIncoming,
    notice,
    error,
    busy,
    discovering,
    start,
    stop,
    discover,
    addIp,
    sendTo,
    confirmReceive,
    rejectReceive,
    cancelSession,
    saveSettings,
    refresh,
  };
}
