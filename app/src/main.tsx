import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import DropZone from "./DropZone";
import { initTheme } from "./theme";

const Root = getCurrentWindow().label === "dropzone" ? DropZone : App;

initTheme();

if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (e) => {
    const t = e.target as HTMLElement | null;
    const hasSelection = !!window.getSelection()?.toString();
    if (t?.closest("input, textarea") || hasSelection) return;
    e.preventDefault();
  });
  window.addEventListener("keydown", (e) => {
    const k = e.key.toLowerCase();
    const mod = e.ctrlKey || e.metaKey;
    if (e.key === "F5" || (mod && ["r", "p", "u", "f", "g", "s", "j"].includes(k)) || (mod && ["=", "+", "-", "0"].includes(k))) {
      e.preventDefault();
    }
  });
  window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false });
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
