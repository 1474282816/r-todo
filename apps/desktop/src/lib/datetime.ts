/** 统一时间格式：YYYY-MM-DD HH:mm:ss（本地时区） */
export function formatDateTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const p = (n: number) => n.toString().padStart(2, "0");
  return (
    `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ` +
    `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
  );
}

/** 统一格式（保留旧函数名，调用方无需改） */
export function relative(iso: string | null): string {
  return formatDateTime(iso);
}

/** 统一格式（保留旧函数名，调用方无需改） */
export function niceDatetime(iso: string | null): string {
  return formatDateTime(iso);
}

/** 已完成任务的"次日 0:00 自动隐藏"过滤 */
export function isCompletedBeforeToday(iso: string | null): boolean {
  if (!iso) return false;
  const completed = new Date(iso);
  const todayStart = new Date();
  todayStart.setHours(0, 0, 0, 0);
  return completed < todayStart;
}
