import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { human } from "./format";
import type { CoreEvent, Decision, Direction, Incoming, MyInfo, Peer, SendResult, Transfer, WebEvent } from "./types";

export interface Toast {
  id: number;
  kind: "info" | "success" | "warn" | "error";
  title: string;
  text?: string;
  action?: { label: string; run: () => void };
  key?: string;
  groupTitle?: string;
  names?: string[];
  namesLabel?: string;
  count?: number;
  ttl?: number;
}

interface State { peers: Peer[]; incoming: Incoming[]; transfers: Transfer[] }

type Action =
  | { kind: "peers"; peers: Peer[] }
  | { kind: "peerFound"; peer: Peer }
  | { kind: "peerLost"; id: string }
  | { kind: "queued"; id: number; peerId: string; name: string }
  | { kind: "drop"; ids: number[] }
  | { kind: "dismiss"; id: number }
  | { kind: "clearDone" }
  | { kind: "core"; e: CoreEvent };

const byName = (a: Peer, b: Peer) => a.name.localeCompare(b.name);

function patch(s: State, id: number, p: Partial<Transfer>): State {
  return { ...s, transfers: s.transfers.map((t) => (t.id === id ? { ...t, ...p } : t)) };
}

function reducer(s: State, a: Action): State {
  switch (a.kind) {
    case "peers":
      return { ...s, peers: [...a.peers].sort(byName) };
    case "peerFound":
      return { ...s, peers: [...s.peers.filter((p) => p.id !== a.peer.id), a.peer].sort(byName) };
    case "peerLost":
      return { ...s, peers: s.peers.filter((p) => p.id !== a.id) };
    case "queued":
      return {
        ...s,
        transfers: [
          { id: a.id, direction: "send", name: a.name, size: 0, done: 0, status: "waiting", peerId: a.peerId },
          ...s.transfers,
        ],
      };
    case "drop":
      return { ...s, transfers: s.transfers.filter((t) => !a.ids.includes(t.id)) };
    case "dismiss":
      return { ...s, incoming: s.incoming.filter((i) => i.id !== a.id) };
    case "clearDone":
      return { ...s, transfers: s.transfers.filter((t) => t.status === "waiting" || t.status === "active") };
    case "core": {
      const e = a.e;
      switch (e.type) {
        case "incoming":
          return { ...s, incoming: [...s.incoming, { ...e, expiresAt: Date.now() + e.timeoutSecs * 1000 }] };
        case "peerCode":
          return patch(s, e.id, { code: e.code, newDevice: e.newDevice, awaiting: !e.trusted });
        case "started": {
          const upd = {
            direction: e.direction, name: e.name, size: e.size,
            peerName: e.peerName, status: "active" as const,
          };
          const exists = s.transfers.some((t) => t.id === e.id);
          const next = exists
            ? patch(s, e.id, upd)
            : { ...s, transfers: [{ id: e.id, done: 0, ...upd }, ...s.transfers] };
          return { ...next, incoming: s.incoming.filter((i) => i.id !== e.id) };
        }
        case "progress":
          return patch(s, e.id, { done: e.done, size: e.total, awaiting: false });
        case "textReceived":
          return s;
        case "finished":
          return patch(s, e.id, { status: "done", path: e.path, awaiting: false });
        case "rejected":
          return { ...patch(s, e.id, { status: "rejected", awaiting: false }), incoming: s.incoming.filter((i) => i.id !== e.id) };
        case "failed":
          return { ...patch(s, e.id, { status: "failed", error: e.error, awaiting: false }), incoming: s.incoming.filter((i) => i.id !== e.id) };
      }
    }
  }
}

let lastSys = 0;
async function systemNotify(title: string, body: string, throttle = true) {
  if (document.hasFocus()) return;
  if (throttle) {
    if (Date.now() - lastSys < 3000) return;
    lastSys = Date.now();
  }
  try {
    let ok = await isPermissionGranted();
    if (!ok) ok = (await requestPermission()) === "granted";
    if (ok) sendNotification({ title, body });
  } catch {
  }
}

interface Meta { direction: Direction; name: string; peerName?: string }
const clip = (s: string, n: number) => (s.length > n ? s.slice(0, n) + "…" : s);

export function useLocalDrop() {
  const [state, dispatch] = useReducer(reducer, { peers: [], incoming: [], transfers: [] });
  const [info, setInfo] = useState<MyInfo | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastsRef = useRef<Toast[]>([]);
  const timers = useRef(new Map<number, number>());
  const seq = useRef(1);
  const meta = useRef(new Map<number, Meta>());
  const asked = useRef(new Map<number, { name: string; sender: string }>());
  const answered = useRef(new Set<number>());
  const texts = useRef(new Set<number>());
  const peersRef = useRef<Peer[]>([]);
  peersRef.current = state.peers;

  const commit = useCallback((l: Toast[]) => {
    toastsRef.current = l;
    setToasts(l);
  }, []);

  const dismissToast = useCallback((id: number) => {
    window.clearTimeout(timers.current.get(id));
    timers.current.delete(id);
    commit(toastsRef.current.filter((t) => t.id !== id));
  }, [commit]);

  const push = useCallback((t: Omit<Toast, "id">, ttl = 4500) => {
    const arm = (id: number) => {
      window.clearTimeout(timers.current.get(id));
      if (ttl > 0) timers.current.set(id, window.setTimeout(() => dismissToast(id), ttl));
    };
    const cur = toastsRef.current;
    const ex = t.key ? cur.find((x) => x.key === t.key) : undefined;
    if (ex) {
      const merged: Toast = {
        ...ex, ...t, id: ex.id, ttl,
        count: (ex.count ?? 1) + 1,
        names: [...(ex.names ?? []), ...(t.names ?? [])],
      };
      commit(cur.map((x) => (x.id === ex.id ? merged : x)));
      arm(ex.id);
    } else {
      const id = seq.current++;
      commit([...cur.slice(-2), { ...t, id, ttl, count: 1, names: t.names ?? [] }]);
      arm(id);
    }
  }, [commit, dismissToast]);

  useEffect(() => {
    let alive = true;
    const unlisten: Array<() => void> = [];
    const reg = (p: Promise<() => void>) => p.then((u) => (alive ? unlisten.push(u) : u()));

    const forget = (id: number) => {
      meta.current.delete(id);
      asked.current.delete(id);
      answered.current.delete(id);
      texts.current.delete(id);
    };

    const notify = (e: CoreEvent) => {
      switch (e.type) {
        case "incoming": {
          const who = e.knownName ?? e.sender;
          asked.current.set(e.id, { name: e.name, sender: who });
          systemNotify(
            `${who} хочет отправить ${e.kind === "text" ? "текст" : "файл"}`,
            e.kind === "text" ? clip(e.preview ?? "", 100) : `${e.name} • ${human(e.size)}`,
            false,
          );
          break;
        }
        case "started": {
          meta.current.set(e.id, { direction: e.direction, name: e.name, peerName: e.peerName });
          if (e.direction === "receive" && !asked.current.has(e.id)) {
            push({
              kind: "info", key: "auto", title: "Автоприём", groupTitle: "Автоприём: {n}",
              text: `${e.name} • от ${e.peerName}`, names: [e.peerName], namesLabel: "От",
            });
            systemNotify("Входящая передача", `${e.name} • от ${e.peerName}`);
          }
          asked.current.delete(e.id);
          answered.current.delete(e.id);
          break;
        }
        case "textReceived": {
          texts.current.add(e.id);
          const from = meta.current.get(e.id)?.peerName ?? "устройства";
          push(
            {
              kind: "success", title: "Текст получен",
              text: `от ${from}: ${clip(e.text.replace(/\s+/g, " "), 100)}`,
              action: { label: "Копировать", run: () => navigator.clipboard.writeText(e.text).catch(() => {}) },
            },
            10000,
          );
          systemNotify("Текст получен", `от ${from}`);
          break;
        }
        case "finished": {
          const m = meta.current.get(e.id);
          const wasText = texts.current.has(e.id);
          forget(e.id);
          if (!m) break;
          const peer = m.peerName ?? "устройства";
          if (m.direction === "receive") {
            if (wasText) break;
            const path = e.path;
            push(
              {
                kind: "success", key: "received", title: "Получено", groupTitle: "Получено: {n}",
                text: `${m.name} • от ${peer}`, names: [peer], namesLabel: "От",
                action: path ? { label: "Показать", run: () => revealItemInDir(path).catch(() => {}) } : undefined,
              },
              7000,
            );
            systemNotify("Получено", `${m.name} • от ${peer}`);
          } else {
            push({
              kind: "success", key: "sent", title: "Отправлено", groupTitle: "Отправлено: {n}",
              text: `${m.name} → ${peer}`, names: [peer], namesLabel: "Кому",
            });
          }
          break;
        }
        case "rejected": {
          const m = meta.current.get(e.id);
          const q = asked.current.get(e.id);
          const timedOut = q && !answered.current.has(e.id);
          forget(e.id);
          if (timedOut) {
            push(
              {
                kind: "warn", key: "timeout", title: "Запрос не принят", groupTitle: "Запросов не принято: {n}",
                text: `${q.sender}: ${q.name} — время вышло или отправитель отменил`,
                names: [q.sender], namesLabel: "От",
              },
              6000,
            );
          }
          if (m?.direction === "send") {
            const peer = m.peerName ?? "Получатель";
            push({
              kind: "warn", key: "rejected", title: "Отклонено", groupTitle: "Отклонено: {n}",
              text: `${peer} не принял «${m.name}»`, names: [peer], namesLabel: "Отклонили",
            });
          }
          break;
        }
        case "failed": {
          const m = meta.current.get(e.id);
          forget(e.id);
          if (m) {
            const send = m.direction === "send";
            push(
              {
                kind: "error", key: send ? "fail-send" : "fail-recv",
                title: send ? "Не удалось отправить" : "Ошибка приёма",
                groupTitle: send ? "Не удалось отправить: {n}" : "Ошибок приёма: {n}",
                text: `${m.name}: ${e.error}`,
              },
              8000,
            );
          }
          break;
        }
      }
    };

    (async () => {
      await Promise.all([
        reg(listen<CoreEvent>("core-event", (ev) => {
          dispatch({ kind: "core", e: ev.payload });
          notify(ev.payload);
        })),
        reg(listen<Peer>("peer-found", (ev) => dispatch({ kind: "peerFound", peer: ev.payload }))),
        reg(listen<string>("peer-lost", (ev) => dispatch({ kind: "peerLost", id: ev.payload }))),
        reg(listen<{ id: number; peerId: string; name: string }>("send-queued", (ev) => {
          meta.current.set(ev.payload.id, { direction: "send", name: ev.payload.name });
          dispatch({ kind: "queued", ...ev.payload });
        })),
        reg(listen("info-changed", () => {
          invoke<MyInfo>("my_info").then(setInfo).catch(() => {});
        })),
        reg(listen<WebEvent>("web-event", (ev) => {
          const e = ev.payload;
          if (e.type === "downloaded") {
            push({ kind: "success", key: "web-dl", title: "Файл скачан", groupTitle: "Скачано файлов: {n}", text: `${e.name} -> браузер (${e.ip})` });
          } else if (e.type === "uploaded") {
            push(
              {
                kind: "success",
                title: "Получено из браузера",
                key: "web-up",
                groupTitle: "Получено из браузера: {n}",
                text: `${e.name} • ${human(e.size)} • ${e.ip}`,
                action: { label: "Показать", run: () => revealItemInDir(e.path).catch(() => {}) },
              },
              7000,
            );
            systemNotify("Получено из браузера", `${e.name} • ${human(e.size)}`);
          } else if (e.type === "pinLocked") {
            push(
              {
                kind: "warn",
                title: "Неверный PIN-код",
                text: `${e.title}: ${e.failures} неудачных попыток с ${e.ip}, ввод временно заблокирован`,
              },
              8000,
            );
          } else if (e.type === "closed" && e.reason !== "manual") {
            const why = {
              limit: "все файлы скачаны",
              expired: "срок действия истёк",
              attempts: "слишком много неверных попыток ввода PIN-кода",
            }[e.reason];
            push({
              kind: "info",
              title: "Ссылка закрыта",
              text: `${e.title}: ${why}`,
            }, e.reason === "attempts" ? 8000 : 4500);
          }
        })),
      ]);
      const [peers, me] = await Promise.all([invoke<Peer[]>("list_peers"), invoke<MyInfo>("my_info")]);
      if (alive) {
        dispatch({ kind: "peers", peers });
        setInfo(me);
      }
    })();

    return () => {
      alive = false;
      unlisten.forEach((u) => u());
    };
  }, [push]);

  const trySend = useCallback(
    async (peerId: string, run: (allowConflict: boolean) => Promise<SendResult>) => {
      const name = peersRef.current.find((p) => p.id === peerId)?.name ?? "Устройство";
      try {
        const res = await run(false);
        if (res.outcome === "nameConflict") {
          dispatch({ kind: "drop", ids: [res.id] });
          meta.current.delete(res.id);
          const ok = await ask(
            `«${name}»: устройство с таким именем раньше имело другой отпечаток. Это может быть переустановка приложения, а может быть подмена. Всё равно отправить?`,
            { title: "Возможная подмена устройства", kind: "warning" },
          );
          if (ok) await run(true);
        }
      } catch (e) {
        push({ kind: "error", title: `Не удалось отправить («${name}»)`, text: String(e) }, 8000);
      }
    },
    [push],
  );

  const sendTo = useCallback(
    (peerId: string, paths: string[], batch?: string) =>
      paths.length
        ? trySend(peerId, (allowConflict) => invoke<SendResult>("send_files", { peerId, paths, allowConflict, batch: batch ?? null }))
        : Promise.resolve(),
    [trySend],
  );

  const sendText = useCallback(
    (peerId: string, text: string, batch?: string) =>
      trySend(peerId, (allowConflict) => invoke<SendResult>("send_text", { peerId, text, allowConflict, batch: batch ?? null })),
    [trySend],
  );

  const sendToMany = useCallback(
    (peerIds: string[], paths: string[]) => {
      const batch = peerIds.length > 1 ? crypto.randomUUID() : undefined;
      return Promise.all(peerIds.map((id) => sendTo(id, paths, batch))).then(() => {});
    },
    [sendTo],
  );

  const sendTextToMany = useCallback(
    (peerIds: string[], text: string) => {
      const batch = peerIds.length > 1 ? crypto.randomUUID() : undefined;
      return Promise.all(peerIds.map((id) => sendText(id, text, batch))).then(() => {});
    },
    [sendText],
  );

  const respond = useCallback(
    async (id: number, decision: Decision) => {
      answered.current.add(id);
      dispatch({ kind: "dismiss", id });
      const ok = await invoke<boolean>("respond_incoming", { id, decision }).catch(() => false);
      if (!ok) push({ kind: "warn", title: "Запрос уже неактуален", text: "Отправитель отменил передачу или время ожидания истекло" });
    },
    [push],
  );

  const clearFinished = useCallback(() => dispatch({ kind: "clearDone" }), []);

  const refreshInfo = useCallback(() => {
    invoke<MyInfo>("my_info").then(setInfo).catch(() => {});
  }, []);

  return { ...state, info, toasts, dismissToast, notify: push, 
    refreshInfo, sendTo, sendText, respond, clearFinished, sendTextToMany, sendToMany };
}