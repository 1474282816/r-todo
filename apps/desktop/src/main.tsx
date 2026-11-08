import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import { TransferWindow } from "./features/transfer/TransferWindow";
import { MonthWindow } from "./windows/MonthWindow";
import "./styles.css";

/**
 * 路由分发：根据 URL query 参数决定渲染哪个窗口。
 * - 无参数（主窗口）→ App（待办列表）
 * - ?view=month → MonthWindow（月视图）
 * - ?view=transfer → TransferWindow（互传）
 *
 * 我们不用 react-router，因为窗口切换=进程内新起 webview，URL 一次性
 */
const params = new URLSearchParams(window.location.search);
const view = params.get("view");

// 让 CSS 能根据视图区分样式（主窗口圆角 + 半透明；月视图/互传无圆角）
document.body.dataset.view = view ?? "main";

let Root: React.ComponentType;
if (view === "month") {
  Root = MonthWindow;
} else if (view === "transfer") {
  Root = TransferWindow;
} else {
  Root = App;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
