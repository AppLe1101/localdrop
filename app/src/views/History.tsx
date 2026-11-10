import { useCallback, useEffect, useState, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { human } from "../format";
import type { Toast } from "../useLocalDrop";
import type { HistoryRecord } from "../types";

type Notify = (t: Omit<Toast, "id">, ttl?: number) => void;
type Filter = "all" | "send" | "receive";
type Range = { from: Date; to: Date } | null;
interface Group { key: string; kind: "batch" | "burst" | "single"; items: HistoryRecord[] }

const STATUS = { done: "Готово", rejected: "Отклонено", failed: "Ошибка" } as const;

// Dates
const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate());
const addDays = (d: Date, n: number) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
const dkey = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
const sameDay = (a: Date, b: Date) => dkey(a) === dkey(b);
const shortDate = (d: Date) => d.toLocaleDateString("ru-RU", { day: "numeric", month: "short" });
const time = (ts: number) => new Date(ts).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });

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

function plural(n: number, one: string, few: string, many: string) {
    const m10 = n % 10, m100 = n % 100;
    const w = m10 === 1 && m100 !== 11 ? one : m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14) ? few : many;
    return `${n} ${w}`;
}

// groups
function groupRecords(rows: HistoryRecord[]): Group[] {
    const out: Group[] = [];
    const byBatch = new Map<string, Group>();
    let burst: Group | null = null;
    for (const r of rows) {
        if (r.batch) {
            let g = byBatch.get(r.batch);
            if (!g) {
                g = { key: "b:" + r.batch, kind: "batch", items: [] };
                byBatch.set(r.batch, g);
                out.push(g);
            }
            g.items.push(r);
            burst = null;
            continue;
        }
        const last: HistoryRecord | undefined = burst?.items[burst.items.length - 1];
        if (burst && last && last.direction === r.direction && last.peer === r.peer && last.ts - r.ts < 90_000) {
            burst.items.push(r);
        } else {
            burst = { key: "s:" + r.id, kind: "single", items: [r] };
            out.push(burst);
        }
    }
    return out.map((g) => (g.kind === "single" && g.items.length > 1 ? { ...g, kind: "burst" as const } : g));
}

// calendar
function Calendar({ range, marks, onChange, onClose }: {
    range: Range; marks: Set<string>; onChange: (r: Range) => void; onClose: () => void;
}) {
    const [view, setView] = useState(() => {
        const b = range?.to ?? new Date();
        return new Date(b.getFullYear(), b.getMonth(), 1);
    });
    const [anchor, setAnchor] = useState<Date | null>(null);
    const today = startOfDay(new Date());

    const lead = (new Date(view.getFullYear(), view.getMonth(), 1).getDay() + 6) % 7;
    const total = new Date(view.getFullYear(), view.getMonth() + 1, 0).getDate();
    const cells: (Date | null)[] = [
        ...Array<null>(lead).fill(null),
        ...Array.from({ length: total }, (_, i) => new Date(view.getFullYear(), view.getMonth(), i + 1)),
    ];
    const title = view.toLocaleDateString("ru-RU", { month: "long", year: "numeric" });

    const pick = (d: Date) => {
    if (!anchor) {
        setAnchor(d);
        onChange({ from: d, to: d });
    } else {
        const [a, b] = d < anchor ? [d, anchor] : [anchor, d];
        setAnchor(null);
        onChange({ from: a, to: b });
        onClose();
    }
    };
    const preset = (from: Date, to: Date) => {
        onChange({ from, to });
        onClose();
    };
    const nav = (n: number) => setView(new Date(view.getFullYear(), view.getMonth() + n, 1));

    return (
        <div className="cal glass strong">
            <div className="cal-presets">
                <button onClick={() => preset(today, today)}>Сегодня</button>
                <button onClick={() => preset(addDays(today, -1), addDays(today, -1))}>Вчера</button>
                <button onClick={() => preset(addDays(today, -6), today)}>7 дней</button>
                <button onClick={() => preset(addDays(today, -29), today)}>30 дней</button>
            </div>
            <div className="cal-head">
                <button className="icon-btn" onClick={() => nav(-1)} aria-label="Предыдущий месяц"><svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m15 18-6-6 6-6"/></svg></button>
                <b>{title.charAt(0).toUpperCase() + title.slice(1)}</b>
                <button className="icon-btn" onClick={() => nav(1)} aria-label="Следующий месяц"><svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m9 18 6-6-6-6"/></svg></button>
            </div>
            <div className="cal-grid">
                {["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"].map((w) => <span key={w} className="cal-w">{w}</span>)}
                {cells.map((d, i) => {
                    if (!d) return <span key={i} />;
                    const inR = !!range && d >= range.from && d <= range.to;
                    const start = !!range && sameDay(d, range.from);
                    const end = !!range && sameDay(d, range.to);
                    return (
                        <button
                            key={i}
                            className={"cal-d" + (inR ? " in" : "") + (start ? " start" : "") + (end ? " end" : "") +
                                (sameDay(d, today) ? " today" : "") + (marks.has(dkey(d)) ? " mark" : "")}
                            onClick={() => pick(d)}
                        >
                            {d.getDate()}
                        </button>
                    );
                })}
            </div>
            <div className="cal-foot">
                <button className="link" onClick={() => { onChange(null); onClose(); }}>Все даты</button>
                <button className="pill primary" onClick={onClose}>Готово</button>
            </div>
        </div>
    );
}

// rows
function Row({ r, byPeer, nested, notify, onRemove }: {
    r: HistoryRecord; byPeer?: boolean; nested?: boolean; notify: Notify; onRemove: (id: number) => void;
}) {
    const reveal = (path: string) =>
        revealItemInDir(path).catch(() =>
            notify({ kind: "warn", title: "Файл не найден", text: "Возможно, он был перемещён или удалён" }),
        );
    const label = r.text ? "Текст: " + r.text.replace(/\s+/g, " ").slice(0, 60) : r.name;
    return (
        <li className={"dev-row" + (nested ? " nested" : "")}>
            <span className="t-icon">{r.direction === "send" ? <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m5 12 7-7 7 7"/><path d="M12 19V5"/></svg> : <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 5v14"/><path d="m19 12-7 7-7-7"/></svg>}</span>
            <div className="dev-main">
                <span className="dev-name">{byPeer ? r.peer : label}</span>
                <small className="hint">
                    {byPeer ? label : `${r.direction === "send" ? "Кому" : "От кого"}: ${r.peer}`}
                    {r.size > 0 && ` • ${human(r.size)}`} • {time(r.ts)}
                </small>
                {r.status === "failed" && r.error && <small className="err">{r.error}</small>}
            </div>
            <span className={"h-status " + r.status}>{STATUS[r.status]}</span>
            {r.direction === "receive" && r.status === "done" && r.path && (
                <button className="link" onClick={() => reveal(r.path!)}>Показать</button>
            )}
            {r.text && (
            <button className="link" onClick={() =>
                navigator.clipboard.writeText(r.text!).then(() => notify({ kind: "success", title: "Скопировано" }, 1500))}>
                Копировать
            </button>
            )}
            <button className="icon-btn" title="Удалить запись" onClick={() => onRemove(r.id)}>✕</button>
        </li>
    );
}

function GroupRow({ g, open, onToggle, onDelete, notify, onRemove }: {
    g: Group; open: boolean; onToggle: () => void; onDelete: () => void; notify: Notify; onRemove: (id: number) => void;
}) {
    const it = g.items;
    const n = (s: string) => it.filter((i) => i.status === s).length;
    const done = n("done"), rej = n("rejected"), fail = n("failed");
    const size = it.reduce((a, i) => a + i.size, 0);
    const peers = [...new Set(it.map((i) => i.peer))];
    const send = it[0].direction === "send";
    const names = [...new Set(it.map((i) => (i.text ? "Текст" : i.name)))];
    const batch = g.kind === "batch";
    const title = batch
        ? names.length === 1 ? names[0] : `Передач: ${it.length}`
        : plural(it.length, "файл", "файла", "файлов");
    const who = send ? "Кому" : "От кого";
    const sub = batch
        ? `${who}: ${peers.slice(0, 3).join(", ")}${peers.length > 3 ? ` и ещё ${peers.length - 3}` : ""} • ${plural(peers.length, "устройство", "устройства", "устройств")}`
        : `${who}: ${peers[0]}${size > 0 ? ` • ${human(size)}` : ""}`;

    return (
        <li className="hgroup">
            <div className="ghead" onClick={onToggle}>
                <span className={"chev" + (open ? " open" : "")}><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m9 18 6-6-6-6"/></svg></span>
                <span className="t-icon">{send ? <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m5 12 7-7 7 7"/><path d="M12 19V5"/></svg> : <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 5v14"/><path d="m19 12-7 7-7-7"/></svg>}</span>
                <div className="dev-main">
                    <span className="dev-name">{title}</span>
                    <small className="hint">{sub} • {time(it[0].ts)}</small>
                </div>
                <span className="gstat">
                    {done > 0 && <i className="ok"><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M20 6 9 17l-5-5"/></svg> {done}</i>}
                    {rej > 0 && <i className="warn">✕ {rej}</i>}
                    {fail > 0 && <i className="bad">! {fail}</i>}
                </span>
                <button className="icon-btn" title="Удалить группу" onClick={(e) => { e.stopPropagation(); onDelete(); }}>✕</button>
            </div>
            {open && (
                <ul className="list gchildren">
                    {it.map((r) => <Row key={r.id} r={r} byPeer={batch} nested notify={notify} onRemove={onRemove} />)}
                </ul>
            )}
        </li>
    );
}

// screen
export default function HistoryView({ notify }: { notify: Notify }) {
    const [items, setItems] = useState<HistoryRecord[] | null>(null);
    const [filter, setFilter] = useState<Filter>("all");
    const [range, setRange] = useState<Range>(null);
    const [calOpen, setCalOpen] = useState(false);
    const [open, setOpen] = useState<Set<string>>(new Set());
    const calRef = useRef<HTMLDivElement>(null);

    const load = useCallback(async () => {
        try {
        setItems(await invoke<HistoryRecord[]>("list_history", { limit: 1000 }));
        } catch (e) {
        notify({ kind: "error", title: "Не удалось загрузить историю", text: String(e) });
        }
    }, [notify]);

    useEffect(() => {
        load();
        let alive = true;
        let un: (() => void) | undefined;
        listen("history-changed", load).then((u) => (alive ? (un = u) : u()));
        return () => { alive = false; un?.(); }
    }, [load]);

    useEffect(() => {
        if (!calOpen) return;
        const down = (e: MouseEvent) => { if (!calRef.current?.contains(e.target as Node)) setCalOpen(false); };
        const esc = (e: KeyboardEvent) => e.key === "Escape" && setCalOpen(false);
        document.addEventListener("mousedown", down);
        document.addEventListener("keydown", esc);
        return () => { document.removeEventListener("mousedown", down); document.removeEventListener("keydown", esc); };
    }, [calOpen]);

    const marks = useMemo(() => new Set((items ?? []).map((r) => dkey(new Date(r.ts)))), [items]);

    const shown = useMemo(() => {
        const from = range ? range.from.getTime() : 0;
        const to = range ? addDays(range.to, 1).getTime() : Infinity;
        return (items ?? []).filter((r) => (filter === "all" || r.direction === filter) && r.ts >= from && r.ts < to);
    }, [items, filter, range]);

    const days = useMemo(() => {
        const out: [string, Group[]][] = [];
        for (const g of groupRecords(shown)) {
            const label = dayLabel(g.items[0].ts);
            const last = out[out.length - 1];
            if (last && last[0] === label) last[1].push(g);
            else out.push([label, [g]]);
        }
        return out;
    }, [shown]);

    const remove = async (id: number) => {
        await invoke("delete_history", { id }).catch(() => {});
        load();
    };

    const removeGroup = async (g: Group) => {
        await Promise.all(g.items.map((r) => invoke("delete_history", { id: r.id }).catch(() => {})));
        load();
    };

    const clearAll = async () => {
        const ok = await ask("Удалить всю историю передач? Сами файлы останутся на диске.", {
            title: "Очистить историю", kind: "warning",
        });
        if (ok) { await invoke("clear_history").catch(() => {}); load(); }
    };

    const toggle = (k: string) =>
        setOpen((s) => { const n = new Set(s); n.has(k) ? n.delete(k) : n.add(k); return n })

    const rangeLabel = !range
        ? "Все даты"
        : sameDay(range.from, range.to) ? shortDate(range.from) : `${shortDate(range.from)} - ${shortDate(range.to)}`;

    return (
        <section className="page">
            <h2>История</h2>
            <div className="toolbar">
                <div className="toolbar-l">
                <div className="seg">
                    {(["all", "send", "receive"] as Filter[]).map((f) => (
                    <button key={f} className={filter === f ? "on" : ""} onClick={() => setFilter(f)}>
                        {{ all: "Все", send: "Отправленные", receive: "Полученные" }[f]}
                    </button>
                    ))}
                </div>
                <div className="cal-wrap" ref={calRef}>
                    <button className={"pill" + (range ? " primary" : "")} onClick={() => setCalOpen((v) => !v)}>
                    <svg className="ico" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                        strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                        <rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>
                    </svg>
                    {rangeLabel}
                    </button>
                    {calOpen && <Calendar range={range} marks={marks} onChange={setRange} onClose={() => setCalOpen(false)} />}
                </div>
                {range && <button className="link" onClick={() => setRange(null)}>Сбросить</button>}
                </div>
                {(items?.length ?? 0) > 0 && <button className="link" onClick={clearAll}>Очистить</button>}
            </div>

            {items && shown.length === 0 ? (
                <p className="empty-page">{range ? "За выбранный период передач нет." : "Здесь появятся ваши передачи."}</p>
            ) : (
                days.map(([label, groups]) => (
                <div key={label}>
                    <div className="day">{label}</div>
                    <div className="group glass">
                    <ul className="list">
                        {groups.map((g) =>
                        g.items.length === 1 ? (
                            <Row key={g.key} r={g.items[0]} notify={notify} onRemove={remove} />
                        ) : (
                            <GroupRow key={g.key} g={g} open={open.has(g.key)} onToggle={() => toggle(g.key)}
                            onDelete={() => removeGroup(g)} notify={notify} onRemove={remove} />
                        ),
                        )}
                    </ul>
                    </div>
                </div>
                ))
            )}
        </section>
    );
}