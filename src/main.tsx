import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyWindowLabel } from "./utils/windowLabel";

// data-window 必须在首帧渲染前同步设置：html/body/#root 的透明与滚动
// 规则按窗口隔离，React effect 首帧后才设置会闪出默认不透明背景。
applyWindowLabel(document);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
