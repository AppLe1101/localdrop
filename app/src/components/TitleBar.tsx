import { useEffect, useState, ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MyInfo } from "../types";

export const IS_MAC = /Mac/i.test(navigator.platform);

interface Props {
  tabs: [string, string][];
  view: string;
  onView: (v: string) => void;
  busy: boolean;
  info: MyInfo | null;
}

const ICONS: Record<string, ReactNode> = {
  radar: (
    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19.07 4.93A10 10 0 0 0 6.99 3.34"/><path d="M4 6h.01"/><path d="M2.29 9.62A10 10 0 1 0 21.31 8.35"/><path d="M16.24 7.76A6 6 0 1 0 8.23 16.67"/><path d="M12 18h.01"/><path d="M17.99 11.66A6 6 0 0 1 15.77 16.67"/><circle cx="12" cy="12" r="2"/><path d="m13.41 10.59 5.66-5.66"/></svg>
  ),
  devices: (
    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h8"/><path d="M10 19v-3.96 3.15"/><path d="M7 19h5"/><rect width="6" height="10" x="16" y="12" rx="2"/></svg>
  ),
  web: (
    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
  ),
  history: (
    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg>
  ),
  settings: (
    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/></svg>
  ),
};

function Controls() {
  const [max, setMax] = useState(false);
  const win = getCurrentWindow();

  useEffect(() => {
    let alive = true;
    let un: (() => void) | undefined;
    const sync = () => win.isMaximized().then((m) => alive && setMax(m)).catch(() => {});
    sync();
    win.onResized(sync).then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
    };
  }, []);

  return (
    <div className="wc">
      <button className="wc-btn" aria-label="Свернуть" onClick={() => win.minimize()}>
        <svg viewBox="0 0 10 10"><path d="M0 5.5h10" /></svg>
      </button>
      <button className="wc-btn" aria-label={max ? "Восстановить" : "Развернуть"} onClick={() => win.toggleMaximize()}>
        {max ? (
          <svg viewBox="0 0 10 10"><rect x="0.5" y="2.5" width="7" height="7" /><path d="M2.5 2.5v-2h7v7h-2" /></svg>
        ) : (
          <svg viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" /></svg>
        )}
      </button>
      <button className="wc-btn close" aria-label="Закрыть" onClick={() => win.close()}>
        <svg viewBox="0 0 10 10"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9" /></svg>
      </button>
    </div>
  );
}

export default function TitleBar({ tabs, view, onView, busy }: Props) {
    return (
        <header className={"bar" + (IS_MAC ? " mac" : "")} data-tauri-drag-region>
        <div className="bar-l" data-tauri-drag-region>
            <svg className="logo" viewBox="0 0 24 24" aria-hidden>
            <circle cx="12" cy="12" r="3" fill="currentColor" />
            <circle cx="12" cy="12" r="7" fill="none" stroke="currentColor" strokeWidth="1.6" opacity=".6" />
            <circle cx="12" cy="12" r="10.5" fill="none" stroke="currentColor" strokeWidth="1.4" opacity=".3" />
            </svg>
            <span className="brand-name" data-tauri-drag-region>LocalDrop</span>
        </div>

        <nav className="tabs glass">
            {tabs.map(([k, label]) => (
                <button
                key={k}
                className={"tab" + (view === k ? " on" : "")}
                onClick={() => onView(k)}
                title={label}
                aria-label={label}
                >
                <svg className={"tab-ico" + (view === k ? " selected" : "")} viewBox="0 0 24 24" aria-hidden>{ICONS[k]}</svg>
                <span className={"tab-label" + (view === k ? " selected" : "")}>{label}</span>
                {k === "radar" && busy && <span className="dot" />}
                </button>
            ))}
        </nav>

        <div className="bar-r" data-tauri-drag-region>
            {!IS_MAC && <Controls />}
        </div>
        </header>
    );
}