import type { Priority } from "../../lib/types";
import { PRIORITY_COLOR } from "../../lib/types";

interface Props {
  priority: Priority;
  size?: number;
}

/** 优先级左侧竖条（Things 3 风） */
export function PriorityBar({ priority, size = 3 }: Props) {
  return (
    <div
      className="shrink-0 rounded-full"
      style={{
        width: size,
        alignSelf: "stretch",
        backgroundColor: PRIORITY_COLOR[priority],
        opacity: priority === "low" ? 0.35 : 0.8,
      }}
    />
  );
}
