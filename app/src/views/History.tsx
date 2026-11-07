import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { human } from "../format";
import type { Toast } from "../useLocalDrop";
import type { HistoryRecord } from "../types";

type Filter = "all" | "send" | "receive";

const STATUS = { done: "Готово", rejected: "Отклонено", failed: "Ошибка" } as const;

function dayLabel(ts: number) {
const d = new Date(ts);
const now = new Date();
const yest = new Date();
yest.setDate(now.getDate() - 1);
if (d.toDateString() === now.toDateString()) return "Сегодня";
if (d.toDateString() === yest.toDateString()) return "Вчера";
return d.toLocaleDateString("ru-RU", {
    day: "numeric",
    month: "long",
    year: d.getFullYear() !== now.getFullYear() ? "numeric" : undefined,
});
}

const time = (ts: number) =>
new Date(ts).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });

export default function HistoryView({ notify }: { notify: (t: Omit<Toast, "id">, ttl?: number) => void }) {
const [items, setItems] = useState<HistoryRecord[] | null>(null);
const [filter, setFilter] = useState<Filter>("all");

const load = useCallback(async () => {
    try {
    setItems(await invoke<HistoryRecord[]>("list_history", { limit: 200 }));
    } catch (e) {
    notify({ kind: "error", title: "Не удалось загрузить историю", text: String(e) });
    }
}, [notify]);

useEffect(() => {
    load();
    let alive = true;
    let un: (() => void) | undefined;
    listen("history-changed", load).then((u) => (alive ? (un = u) : u()));
    return () => {
    alive = false;
    un?.();
    };
}, [load]);

const reveal = (path: string) =>
    revealItemInDir(path).catch(() =>
    notify({ kind: "warn", title: "Файл не найден", text: "Возможно, он был перемещён или удалён" }),
    );

const remove = async (id: number) => {
    await invoke("delete_history", { id }).catch(() => {});
    load();
};

const clearAll = async () => {
    const ok = await ask("Удалить всю историю передач? Сами файлы останутся на диске.", {
    title: "Очистить историю",
    kind: "warning",
    });
    if (ok) {
    await invoke("clear_history").catch(() => {});
    load();
    }
};

const shown = (items ?? []).filter((i) => filter === "all" || i.direction === filter);
const groups: [string, HistoryRecord[]][] = [];
for (const r of shown) {
    const label = dayLabel(r.ts);
    const last = groups[groups.length - 1];
    if (last && last[0] === label) last[1].push(r);
    else groups.push([label, [r]]);
}

return (
    <section className="page">
    <h2>История</h2>
    <div className="toolbar">
        <div className="seg">
        {(["all", "send", "receive"] as Filter[]).map((f) => (
            <button key={f} className={filter === f ? "on" : ""} onClick={() => setFilter(f)}>
            {{ all: "Все", send: "Отправленные", receive: "Полученные" }[f]}
            </button>
        ))}
        </div>
        {(items?.length ?? 0) > 0 && (
        <button className="link" onClick={clearAll}>Очистить</button>
        )}
    </div>

    {items && shown.length === 0 ? (
        <p className="empty-page">Здесь появятся ваши передачи.</p>
    ) : (
        groups.map(([label, rows]) => (
        <div key={label}>
            <div className="day">{label}</div>
            <div className="group glass">
            <ul className="list">
                {rows.map((r) => (
                <li className="dev-row" key={r.id}>
                    <span className="t-icon">{r.direction === "send" ? <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12 7-7 7 7"/><path d="M12 19V5"/></svg> : <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14"/><path d="m19 12-7 7-7-7"/></svg>}</span>
                    <div className="dev-main">
                    <span className="dev-name">
                        {r.text ? "Текст: " + r.text.replace(/\s+/g, " ").slice(0, 60) : r.name}
                    </span>
                    <small className="hint">
                        {r.direction === "send" ? "Кому" : "От кого"}: {r.peer}
                        {r.size > 0 && ` • ${human(r.size)}`} • {time(r.ts)}
                    </small>
                    {r.status === "failed" && r.error && <small className="err">{r.error}</small>}
                    </div>
                    <span className={"h-status " + r.status}>{STATUS[r.status]}</span>
                    {r.direction === "receive" && r.status === "done" && r.path && (
                    <button className="link" onClick={() => reveal(r.path!)}>Показать</button>
                    )}
                    {r.text && (
                    <button
                        className="link"
                        onClick={() => navigator.clipboard.writeText(r.text!).then(() => notify({ kind: "success", title: "Скопировано" }, 1500))}
                    >
                        Копировать
                    </button>
                    )}
                    <button className="icon-btn" title="Удалить запись" onClick={() => remove(r.id)}>✕</button>
                </li>
                ))}
            </ul>
            </div>
        </div>
        ))
    )}
    </section>
);
}