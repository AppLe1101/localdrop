import { useCallback, useEffect, useRef, useState, ReactNode } from "react";
import type { CSSProperties } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useLocalDrop } from "./useLocalDrop";
import { baseName, hue, human, initial } from "./format";
import DevicesView from "./views/Devices";
import HistoryView from "./views/History";
import SettingsView from "./views/Settings";
import WebView from "./views/Web";
import TitleBar from "./components/TitleBar";
import type { Peer, Transfer } from "./types";
import { checkForUpdates, getAutoCheck, installUpdate, useUpdater } from "./updater";
import "./App.css";

type View = "radar" | "devices" | "web" | "history" | "settings";
const TABS: [View, string][] = [
  ["radar", "Радар"],
  ["devices", "Устройства"],
  ["web", "Ссылки"],
  ["history", "История"],
  ["settings", "Настройки"],
];

const C = 2 * Math.PI * 46;

const Ico = ({ children }: { children: ReactNode }) => (
  <svg className="ico" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor"
       strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    {children}
  </svg>
);

const ICON = {
  file: <><path d="M11.35 22H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.706.706l3.588 3.588A2.4 2.4 0 0 1 20 8v5.35"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M14 19h6"/><path d="M17 16v6"/></>,
  folder: <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>,
  text: <><path d="M15 5h6"/><path d="M15 12h6"/><path d="M3 19h18"/><path d="m3 12 3.553-7.724a.5.5 0 0 1 .894 0L11 12"/><path d="M3.92 10h6.16"/></>,
  transfers: <><path d="m21 16-4 4-4-4"/><path d="M17 20V4"/><path d="m3 8 4-4 4 4"/><path d="M7 4v16"/></>,
  users: <><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></>,
  send: <><path d="m15 17 5-5-5-5"/><path d="M4 18v-2a4 4 0 0 1 4-4h12"/></>,
  close: <><path d="M18 6 6 18"/><path d="m6 6 12 12"/></>,
  clipboard: <><rect width="8" height="4" x="8" y="2" rx="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/></>,
};

function place(i: number, n: number) {
  const inner = i < 5;
  const count = inner ? Math.min(n, 5) : n - 5;
  const idx = inner ? i : i - 5;
  const r = inner ? 27 : 39;
  const a = -Math.PI / 2 + (inner ? 0 : Math.PI / count) + (idx * 2 * Math.PI) / count;
  return { x: 50 + r * Math.cos(a), y: 50 + r * Math.sin(a) };
}

function PeerBubble({ peer, pos, hover, transfer, onClick, selectMode, selected }: {
  peer: Peer; pos: { x: number; y: number }; hover: boolean; transfer?: Transfer; onClick: () => void; selectMode: boolean; selected: boolean;
}) {
  const frac = transfer && transfer.size > 0 ? transfer.done / transfer.size : 0;
  const waiting = transfer?.status === "waiting" || !!transfer?.awaiting;
  const status = transfer
    ? waiting ? "Ждём ответа…" : `${Math.round(frac * 100)}%`
    : hover ? "Отпустите, чтобы отправить" : "";
  return (
    <button
      className={"peer" + (hover ? " hover" : "") + (selectMode ? " selectable" : "") + (selected ? " picked" : "")}
      data-peer-id={peer.id}
      style={{ left: `${pos.x}%`, top: `${pos.y}%` }}
      onClick={onClick}
    >
      <span className="bubble">
        <span className="avatar" style={{ "--h": hue(peer.name) } as CSSProperties}>
          {initial(peer.name)}
        </span>
        {selectMode && <span className="check">{selected ? "✓" : ""}</span>}
        {transfer && (
          <svg className={"ring" + (waiting ? " spin" : "")} viewBox="0 0 100 100">
            <circle cx="50" cy="50" r="46" className="ring-bg" />
            <circle
              cx="50" cy="50" r="46" className="ring-fg"
              strokeDasharray={C}
              strokeDashoffset={waiting ? C * 0.75 : C * (1 - frac)}
              transform="rotate(-90 50 50)"
            />
          </svg>
        )}
      </span>
      <span className="peer-name">{peer.name}</span>
      <span className="peer-status">{status}</span>
    </button>
  );
}

function TransferRow({ t }: { t: Transfer }) {
  const pct = t.size > 0 ? Math.min(100, Math.floor((t.done * 100) / t.size)) : 0;
  const label = {
    waiting: "Ожидание ответа…",
    active: `${pct}% • ${human(t.size)}`,
    done: "Готово",
    failed: "Ошибка",
    rejected: "Отклонено",
  }[t.status];
  const text = t.awaiting ? "Ожидание ответа..." : label
  return (
    <li className={"t-row " + t.status}>
      <span className={`t-icon ${t.direction === "send" ? "t-icon-send" : "t-icon-receive"}`}>{t.direction === "send" ? 
        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m5 12 7-7 7 7"/><path d="M12 19V5"/></svg>
       : <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 5v14"/><path d="m19 12-7 7-7-7"/></svg>}</span>
      <div className="t-main">
        <div className="t-top">
          <span className="t-name">{t.name}</span>
          <span className="t-status">{text}</span>
        </div>
        {t.status === "active" && <div className="bar"><div style={{ width: `${pct}%` }} /></div>}
        {t.peerName && (
          <small className="hint">{t.direction === "send" ? "Кому" : "От кого"}: {t.peerName}</small>
        )}
        {t.status === "failed" && <small className="err">{t.error}</small>}
        {t.status === "done" && t.path && <small className="hint path">{t.path}</small>}
      </div>
    </li>
  );
}

function Countdown({ expiresAt, total }: { expiresAt: number; total: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 200);
    return () => clearInterval(t);
  }, []);
  const left = Math.max(0, expiresAt - now);
  return (
    <div className="countdown">
      <div className="cd-track">
        <div
          className={"cd-bar" + (left < 10000 ? " low" : "")}
          style={{ width: `${Math.min(100, (left / (total * 1000)) * 100)}%` }}
        />
      </div>
      <span>Автоотклонение через {Math.ceil(left / 1000)} с</span>
    </div>
  );
}

export default function App() {
  const {
    peers, incoming, transfers, info, toasts, dismissToast, notify, refreshInfo,
    sendTo, sendText, respond, clearFinished, sendTextToMany, sendToMany
  } = useLocalDrop();
  const [stagedText, setStagedText] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);
  const [draft, setDraft] = useState("");
  const [view, setView] = useState<View>("radar");
  const [staged, setStaged] = useState<string[]>([]);
  const [hover, setHover] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [sheet, setSheet] = useState(false);

  const [scrolled, setScrolled] = useState(false);
  const [focused, setFocused] = useState(true);

  const [multi, setMulti] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const multiRef = useRef(false);
  multiRef.current = multi;

  const viewRef = useRef(view);
  viewRef.current = view;

  const hasStaged = staged.length > 0 || stagedText !== null;
  const stagedLabel =
    staged.length > 0
      ? `${staged.length} ${staged.length === 1 ? "объект" : "объектов"}: ${baseName(staged[0])}${staged.length > 1 ? "..." : ""}`
      : stagedText !== null
        ? `Текст: «${stagedText.trim().slice(0, 40)}${stagedText.trim().length > 40 ? "..." : ""}»`
        : "";

  const pasteFromClipboard = useCallback(async () => {
    try {
      const c = await invoke<{ files: string[]; text: string | null }>("read_clipboard");
      if (c.files.length) {
        setStagedText(null);
        setStaged((s) => [...new Set([...s, ...c.files])]);
      } else if (c.text?.trim()) {
        setStaged([]);
        setStagedText(c.text);
      } else {
        notify({ kind: "info", title: "Буфер обмена пуст", text: "Скопируйте файл, изображение или текст" }, 3000);
      }
    } catch (e) {
      notify({ kind: "error", title: "Не удалось прочитать буфер", text: String(e) }, 6000);
    }
  }, [notify]);

  useEffect(() => setScrolled(false), [view]);

  useEffect(() => {
    setSelected((s) => s.filter((id) => peers.some((p) => p.id === id)));
  }, [peers]);

  useEffect(() => {
    const on = () => setFocused(true);
    const off = () => setFocused(false);
    window.addEventListener("focus", on);
    window.addEventListener("blur", off);
    return () => {
      window.removeEventListener("focus", on);
      window.removeEventListener("blur", off);
    };
  }, []);

  // Drag & Drop
  useEffect(() => {
    let alive = true;
    let un: (() => void) | undefined;
    const peerAt = (pos: { x: number; y: number }) => {
      const r = window.devicePixelRatio || 1;
      const el = document.elementFromPoint(pos.x / r, pos.y / r);
      return el?.closest<HTMLElement>("[data-peer-id]")?.dataset.peerId ?? null;
    };
    getCurrentWebview()
      .onDragDropEvent((e) => {
        const p = e.payload;
        if (viewRef.current !== "radar") {
          setDragging(false);
          setHover(null);
          return;
        }
        if (p.type === "enter") {
          setDragging(true);
          setHover(peerAt(p.position));
        } else if (p.type === "over") {
          setHover(peerAt(p.position));
        } else if (p.type === "drop") {
          const target = multiRef.current ? null : peerAt(p.position);
          setDragging(false);
          setHover(null);
          if (target) sendTo(target, p.paths);
          else { setStagedText(null); setStaged((s) => [...new Set([...s, ...p.paths])]); }
        } else {
          setDragging(false);
          setHover(null);
        }
      })
      .then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
    };
  }, [sendTo]);

  // Paste
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      if (viewRef.current !== "radar") return;
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
      e.preventDefault();
      const dt = e.clipboardData;
      const hasFiles = !!dt && (dt.files.length > 0 || dt.types.includes("Files"));
      const text = dt?.getData("text/plain");
      if (!hasFiles && text && text.trim()) {
        setStaged([]);
        setStagedText(text);
      } else {
        pasteFromClipboard();
      }
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, [pasteFromClipboard]);

  useEffect(() => {
    let alive = true;
    let un: (() => void) | undefined;
    const take = async () => {
      const paths = await invoke<string[]>("take_external_files").catch(() => [] as string[]);
      if (!alive || !paths.length) return;
      setView("radar");
      setStagedText(null);
      setStaged((s) => [...new Set([...s, ...paths])]);
      notify({ kind: "info", title: `Подготовлено объектов: ${paths.length}`, text: "Выберите устройство на радаре" }, 5000);
    };
    take();
    listen("external-files", take).then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
    };
  }, [notify]);

  // Updates timer
  const upd = useUpdater();
  useEffect(() => {
    if (!getAutoCheck()) return;
    const first = setTimeout(() => checkForUpdates(true), 8000);
    const every = setInterval(() => getAutoCheck() && checkForUpdates(true), 6 * 3600 * 1000);
    return () => {
      clearTimeout(first);
      clearInterval(every);
    };
  }, []);

  // Update
  const toldVersion = useRef("");
  useEffect(() => {
    if (upd.s === "available" && toldVersion.current !== upd.version) {
      toldVersion.current = upd.version;
      notify(
        {
          kind: "info",
          title: `Доступна версия ${upd.version}`,
          text: "Приложение перезапустится после установки",
          action: { label: "Обновить", run: () => installUpdate() },
        },
        20000,
      );
    }
  }, [upd, notify])

  const pickFolder = useCallback(async () => {
  const r = await open({ directory: true, multiple: false });
    return typeof r === "string" ? [r] : [];
  }, []);

  const pickFiles = useCallback(async () => {
    const r = await open({ multiple: true });
    return Array.isArray(r) ? r : r ? [r] : [];
  }, []);

  const clearStaged = () => {
    setStaged([]);
    setStagedText(null);
  };

  const addFiles = async (picker: () => Promise<string[]>) => {
    const f = await picker();
    if (!f.length) return;
    setStagedText(null);
    setStaged((s) => [...new Set([...s, ...f])]);
  };

  const onPeerClick = async (peer: Peer) => {
    if (multi) return toggle(peer.id);
    if (stagedText !== null) {
      const t = stagedText;
      setStagedText(null);
      sendText(peer.id, t);
    } else if (staged.length) {
      const files = staged;
      setStaged([]);
      sendTo(peer.id, files);
    } else {
      sendTo(peer.id, await pickFiles());
    }
  };

  const toggle = (id: string) => setSelected((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]));
  const exitMulti = () => {
    setMulti(false);
    setSelected([]);
  };
  const allSelected = peers.length > 0 && selected.length === peers.length;

  const sendSelected = async () => {
    const ids = selected;
    if (!ids.length) return;
    if (stagedText !== null) {
      const t = stagedText;
      setStagedText(null);
      exitMulti();
      await sendTextToMany(ids, t);
      return;
    }
    const files = staged.length ? staged : await pickFiles();
    if (!files.length) return;
    setStaged([]);
    exitMulti();
    await sendToMany(ids, files);
  };

  const active = transfers.filter((t) => t.status === "waiting" || t.status === "active");
  const activeFor = (peerId: string) => active.find((t) => t.peerId === peerId);
  //const waitingCode = transfers.find((t) => t.awaiting && t.code);
  const cur = incoming[0];

  return (
    <main className={
      (dragging ? "dragging " : "") + (focused ? "" : "unfocused ") + (scrolled ? "scrolled" : "")
    }>
      <div className="bg" />
      <div className="topfx" aria-hidden><i /><i /><i /></div>
      <TitleBar
        tabs={TABS}
        view={view}
        onView={(v) => setView(v as View)}
        busy={active.length > 0}
        info={info}
      />

      <div className="toasts">
        {transfers.filter((t) => t.awaiting && t.code).map((t) => (
          <div key={t.id} className="toast glass code-toast">
            <small>Ждём подтверждения от «{t.peerName ?? "устройства"}»</small>
            <span className="code-big">{t.code}</span>
            <small>Код должен совпадать с кодом на экране получателя. Если нет, попросите его отклонить передачу.</small>
          </div>
        ))}
        {dragging && (
          <div className="toast glass">
            <span className="t-dot" />
            <div className="toast-body"><b>Перетащите файл на устройство</b></div>
          </div>
        )}
        {toasts.map((t) => (
          <div key={t.id} className={"toast glass " + t.kind} onClick={() => dismissToast(t.id)}>
            <span className="t-dot" />
            <div className="toast-body">
              <b>{t.title}</b>
              {t.text && <small>{t.text}</small>}
            </div>
            {t.action && (
              <button
                className="link"
                onClick={(e) => {
                  e.stopPropagation();
                  t.action!.run();
                  dismissToast(t.id);
                }}
              >
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>

      {view === "radar" && (
        <div className="radar-view">
          <section className="stage">
            <div className="radar">
              <i className="orbit o3" /><i className="orbit o2" /><i className="orbit o1" />
              <i className="pulse" /><i className="pulse" /><i className="pulse" />

              <div className="me">
                <span className="avatar me-avatar">{initial(info?.name ?? "")}</span>
              </div>

              {peers.map((p, i) => (
                <PeerBubble
                  key={p.id}
                  peer={p}
                  pos={place(i, peers.length)}
                  hover={hover === p.id}
                  transfer={activeFor(p.id)}
                  onClick={() => onPeerClick(p)}
                  selectMode={multi}
                  selected={selected.includes(p.id)}
                />
              ))}

              {peers.length === 0 && (
                <p className="empty">
                  Ищем устройства рядом…
                  <small>Запустите LocalDrop на другом устройстве в этой же сети</small>
                </p>
              )}
            </div>
          </section>

          <footer className={"dock glass" + (multi || hasStaged ? " multi" : "")}>
            {hasStaged && (
              <span className="staged-chip">
                <span className="chip-text">{stagedLabel}</span>
                <button className="chip-x" title="Очистить" aria-label="Очистить" onClick={clearStaged}>
                  <Ico>{ICON.close}</Ico>
                </button>
              </span>
            )}

            <span className="dock-text hint">
              {multi
                ? selected.length
                  ? `Выбрано: ${selected.length} из ${peers.length}`
                  : "Выберите устройства"
                : hasStaged
                  ? "выберите устройство"
                  : "Перетащите файл или папку, либо вставьте текст (Ctrl+V)"}
            </span>

            {multi ? (
              <>
                <button className="pill" onClick={() => setSelected(allSelected ? [] : peers.map((p) => p.id))}>
                  {allSelected ? "Снять все" : "Выбрать все"}
                </button>
                <button className="pill primary" disabled={!selected.length} onClick={sendSelected}>
                  <Ico>{ICON.send}</Ico><span className="pill-text">Отправить</span>
                </button>
                <button className="pill" onClick={exitMulti}>Отмена</button>
              </>
            ) : (
              <>
                {peers.length > 1 && (
                  <button className="pill" title="Выбрать несколько устройств" aria-label="Выбрать несколько устройств" onClick={() => setMulti(true)}>
                    <Ico>{ICON.users}</Ico>
                  </button>
                )}
                <button className="pill" title="Файлы" aria-label="Файлы" onClick={() => addFiles(pickFiles)}>
                  <Ico>{ICON.file}</Ico>
                </button>
                <button className="pill" title="Папка" aria-label="Папка" onClick={() => addFiles(pickFolder)}>
                  <Ico>{ICON.folder}</Ico>
                </button>
                <button className="pill" title="Текст" aria-label="Текст" onClick={() => { setDraft(""); setComposing(true); }}>
                  <Ico>{ICON.text}</Ico>
                </button>
                <button className="pill" title="Вставить из буфера" aria-label="Вставить из буфера" onClick={pasteFromClipboard}>
                  <Ico>{ICON.clipboard}</Ico>
                </button>
              </>
            )}

            <button
              className={"pill" + (sheet ? " primary" : "")}
              title="Передачи"
              aria-label="Передачи"
              onClick={() => setSheet((v) => !v)}
            >
              <Ico>{ICON.transfers}</Ico>
              <span className="pill-text">Передачи</span>
              {active.length > 0 && <span className="badge">{active.length}</span>}
            </button>
          </footer>

          {sheet && (
            <section className="sheet glass">
              <div className="sheet-head">
                <span>Передачи</span>
                <span>
                  <button className="link" onClick={clearFinished}>Очистить завершённые</button>
                  <button className="link close" onClick={() => setSheet(false)} aria-label="Закрыть">✕</button>
                </span>
              </div>
              {transfers.length === 0 ? (
                <p className="hint center pad">Пока ничего не передавалось</p>
              ) : (
                <ul className="sheet-list">
                  {transfers.map((t) => <TransferRow key={t.id} t={t} />)}
                </ul>
              )}
            </section>
          )}
        </div>
      )}

      {view !== "radar" && (
        <>
          <div className="scroll" key={view} onScroll={(e) => setScrolled(e.currentTarget.scrollTop > 4)}>
            {view === "devices" && <DevicesView peers={peers} notify={notify} />}
            {view === "web" && <WebView notify={notify} />}
            {view === "history" && <HistoryView notify={notify} />}
            {view === "settings" && <SettingsView info={info} notify={notify} onChanged={refreshInfo} />}
          </div>
          <div className="botfx" aria-hidden />
        </>
      )}
      

      {composing && (
        <div className="overlay" onClick={() => setComposing(false)}>
          <div className="card glass strong" onClick={(e) => e.stopPropagation()}>
            <h2>Текст для отправки</h2>
            <textarea
              className="input area"
              autoFocus
              rows={6}
              value={draft}
              placeholder="Введите или вставьте текст"
              onChange={(e) => setDraft(e.target.value)}
            />
            <div className="actions">
              <button
                className="pill primary lg"
                disabled={!draft.trim()}
                onClick={() => { setStaged([]); setStagedText(draft); setComposing(false); }}
              >
                Далее: выбрать устройство
              </button>
              <button className="pill lg" onClick={() => setComposing(false)}>Отмена</button>
            </div>
          </div>
        </div>
      )}

      {cur && (
        <div className="overlay">
          <div className="card glass strong" key={cur.id}>
            <span className="avatar big" style={{ "--h": hue(cur.knownName ?? cur.sender) } as CSSProperties}>
              {initial(cur.knownName ?? cur.sender)}
            </span>
            <h2>
              «{cur.knownName ?? cur.sender}» хочет отправить вам{" "}
              {cur.kind === "text" ? "текст" : cur.count > 1 ? "файлы" : "файл"}
            </h2>
            {cur.kind === "text" ? (
              <p className="preview">{cur.preview ?? ""}</p>
            ) : (
              <p className="file">{cur.name}</p>
            )}
            <p className="hint">{human(cur.size)}</p>
            {!cur.knownName && <p className="warn">Новое устройство</p>}
            <div className="code">{cur.code}</div>
            <p className="hint">
              Код должен совпадать с кодом на экране отправителя
              <br />Отпечаток: {cur.fpShort}
            </p>
            <Countdown expiresAt={cur.expiresAt} total={cur.timeoutSecs} />
            <div className="actions">
              <button className="pill primary lg" onClick={() => respond(cur.id, "acceptOnce")}>Принять</button>
              <button className="pill lg" onClick={() => respond(cur.id, "acceptAndTrust")}>Принять и доверять</button>
              <button className="pill lg danger" onClick={() => respond(cur.id, "reject")}>Отклонить</button>
            </div>
            {incoming.length > 1 && <p className="hint center">Ещё запросов: {incoming.length - 1}</p>}
          </div>
        </div>
      )}
    </main>
  );
}