import { useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { baseName, hue, initial } from "./format";
import type { CoreEvent, Peer, SendResult } from "./types";
import "./App.css";

type Staged = { kind: "files"; paths: string[] } | { kind: "text"; text: string };
type Status = { peerId: string; text: string; error?: boolean } | null;

const byName = (a: Peer, b: Peer) => a.name.localeCompare(b.name);
const hide = () => invoke("hide_dropzone").catch(() => {});

export default function DropZone() {
  const [peers, setPeers] = useState<Peer[]>([]);
  const [staged, setStaged] = useState<Staged | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [status, setStatus] = useState<Status>(null);
  const [code, setCode] = useState<{ id: number; code: string } | null>(null);
  const busy = useRef(false);

  // Список устройств
  useEffect(() => {
    let alive = true;
    const un: Array<() => void> = [];
    const reg = (p: Promise<() => void>) => p.then((u) => (alive ? un.push(u) : u()));
    (async () => {
      await Promise.all([
        reg(listen<Peer>("peer-found", (e) =>
          setPeers((l) => [...l.filter((p) => p.id !== e.payload.id), e.payload].sort(byName)))),
        reg(listen<string>("peer-lost", (e) => setPeers((l) => l.filter((p) => p.id !== e.payload)))),
        reg(listen<CoreEvent>("core-event", (e) => {
          const ev = e.payload;
          if (ev.type === "peerCode") setCode(ev.trusted ? null : { id: ev.id, code: ev.code });
          else if (
            (ev.type === "progress" || ev.type === "finished" || ev.type === "rejected" || ev.type === "failed")
          ) setCode((c) => (c && c.id === ev.id ? null : c));
        })),
      ]);
      const list = await invoke<Peer[]>("list_peers");
      if (alive) setPeers(list.sort(byName));
    })();
    return () => {
      alive = false;
      un.forEach((u) => u());
    };
  }, []);

  const send = useCallback(async (peerId: string, what: Staged) => {
    if (busy.current) return;
    busy.current = true;
    setStatus({ peerId, text: "Отправка…" });
    const run = (allowConflict: boolean) =>
      what.kind === "files"
        ? invoke<SendResult>("send_files", { peerId, paths: what.paths, allowConflict })
        : invoke<SendResult>("send_text", { peerId, text: what.text, allowConflict });
    try {
      let res = await run(false);
      if (res.outcome === "nameConflict") {
        const ok = await ask(
          "Устройство с таким именем раньше имело другой отпечаток. Это может быть переустановка приложения, а может быть подмена. Всё равно отправить?",
          { title: "Возможная подмена устройства", kind: "warning" },
        );
        if (!ok) {
          setStatus(null);
          return;
        }
        res = await run(true);
      }
      const text = {
        sent: "Отправлено ✓",
        rejected: "Отклонено",
        failed: "Не удалось (см. историю)",
        nameConflict: "Не удалось",
      }[res.outcome];
      setStatus({ peerId, text, error: res.outcome !== "sent" });
      if (res.outcome === "sent") {
        setStaged(null);
        setTimeout(() => {
          setStatus(null);
          hide();
        }, 1400);
      }
    } catch (e) {
      setStatus({ peerId, text: String(e), error: true });
    } finally {
      busy.current = false;
    }
  }, []);

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
        if (p.type === "enter" || p.type === "over") {
          setDragging(true);
          setHover(peerAt(p.position));
        } else if (p.type === "drop") {
          const target = peerAt(p.position);
          setDragging(false);
          setHover(null);
          if (!p.paths.length) return;
          if (target) send(target, { kind: "files", paths: p.paths });
          else {
            setStatus(null);
            setStaged((s) => ({
              kind: "files",
              paths: [...new Set([...(s?.kind === "files" ? s.paths : []), ...p.paths])],
            }));
          }
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
  }, [send]);

  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const text = e.clipboardData?.getData("text/plain");
      if (text && text.trim()) {
        e.preventDefault();
        setStatus(null);
        setStaged({ kind: "text", text });
      }
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && hide();
    window.addEventListener("paste", onPaste);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("paste", onPaste);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const onPeerClick = async (p: Peer) => {
    if (staged) return send(p.id, staged);
    const r = await open({ multiple: true });
    const paths = Array.isArray(r) ? r : r ? [r] : [];
    if (paths.length) send(p.id, { kind: "files", paths });
  };

  const hint = staged
    ? staged.kind === "files"
      ? `${staged.paths.length} ${staged.paths.length === 1 ? "объект" : "объектов"}: ${baseName(staged.paths[0])}`
      : `Текст: «${staged.text.trim().slice(0, 30)}${staged.text.trim().length > 30 ? "…" : ""}»`
    : "Перетащите файл или папку сюда, либо вставьте текст (Ctrl+V)";

  return (
    <main className={"dz" + (dragging ? " dragging" : "")}>
      <div className="bg" />
      <div className="dz-head" data-tauri-drag-region>
        <span data-tauri-drag-region>Drop Zone</span>
        <button className="icon-btn" title="Закрыть (Esc)" onClick={hide}>✕</button>
      </div>

      <div className="dz-area">
        <p className="hint dz-hint">
          {hint}
          {staged && <> - выберите устройство <button className="link" onClick={() => setStaged(null)}>Очистить</button></>}
        </p>

        {code && (
          <div className="dz-code">
            Код подтверждения <b>{code.code}</b>
            <small>Сверьте с экраном получателя</small>
          </div>
        )}

        {peers.length === 0 ? (
          <p className="hint">Устройства не найдены</p>
        ) : (
          <div className="dz-peers">
            {peers.map((p) => (
              <button
                key={p.id}
                className={"peer" + (hover === p.id ? " hover" : "")}
                data-peer-id={p.id}
                onClick={() => onPeerClick(p)}
              >
                <span className="bubble">
                  <span className="avatar" style={{ "--h": hue(p.name) } as CSSProperties}>{initial(p.name)}</span>
                </span>
                <span className="peer-name">{p.name}</span>
                <span className="peer-status" style={status?.peerId === p.id && status.error ? { color: "var(--danger)" } : undefined}>
                  {status?.peerId === p.id ? status.text : hover === p.id ? "Отпустите" : ""}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </main>
  );
}