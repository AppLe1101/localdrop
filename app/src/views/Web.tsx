import { useCallback, useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { human } from "../format";
import PinPad from "../components/PinPad";
import type { Toast } from "../useLocalDrop";
import type { WebSession } from "../types";
import { canvasBlob, renderQrCard } from "../qrcard";

type Notify = (t: Omit<Toast, "id">, ttl?: number) => void;

const TTLS: [number, string][] = [[5, "5 мин"], [15, "15 мин"], [60, "1 час"], [1440, "24 часа"]];
const LIMITS: [number | null, string][] = [[1, "1"], [3, "3"], [10, "10"], [null, "Без лимита"]];
const SIZES: [number | null, string][] = [[1024, "1 ГБ"], [10240, "10 ГБ"], [null, "Без лимита"]];
const PINS: [number, string][] = [[0, "Без PIN"], [4, "4 цифры"], [6, "6 цифр"]];

function Seg<T>({ value, options, onChange }: {
  value: T; options: [T, string][]; onChange: (v: T) => void;
}) {
  return (
    <div className="seg">
      {options.map(([v, label]) => (
        <button key={label} className={v === value ? "on" : ""} onClick={() => onChange(v)}>{label}</button>
      ))}
    </div>
  );
}

const clock = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

function ShareDialog({ s, url, onClose, notify }: {
  s: WebSession; url: string; onClose: () => void; notify: Notify;
}) {
  const cv = useRef<HTMLCanvasElement | null>(null);
  const [img, setImg] = useState("");
  const [err, setErr] = useState("");
  const canShare = typeof navigator.canShare === "function" && typeof navigator.share === "function";

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const svg = await invoke<string>("qr_svg", { text: url });
        const c = await renderQrCard({
          svg,
          title: s.title,
          subtitle: s.kind === "share" ? `Файлов: ${s.files} • ${human(s.totalSize)}` : "Загрузка файлов с телефона",
          expiresAt: s.expiresAt,
          pin: !!s.pin,
        });
        if (!alive) return;
        cv.current = c;
        setImg(c.toDataURL("image/png"));
      } catch (e) {
        if (alive) setErr(String(e));
      }
    })();
    return () => { alive = false; };
  }, [url, s.title, s.expiresAt, s.pin, s.kind, s.files, s.totalSize]);

  const blob = () => canvasBlob(cv.current!);
  const done = (title: string) => notify({ kind: "success", title }, 2000);
  const fail = (title: string) => (e: unknown) => notify({ kind: "error", title, text: String(e) }, 6000);

  const copyImage = () =>
    blob()
      .then((b) => navigator.clipboard.write([new ClipboardItem({ "image/png": b })]))
      .then(() => done("Изображение скопировано"))
      .catch(fail("Не удалось скопировать изображение"));

  const saveImage = async () => {
    try {
      const path = await save({ defaultPath: "LocalDrop-QR.png", filters: [{ name: "PNG", extensions: ["png"] }] });
      if (!path) return;
      const data = Array.from(new Uint8Array(await (await blob()).arrayBuffer()));
      await invoke("save_file", { path, data });
      done("Изображение сохранено");
    } catch (e) {
      fail("Не удалось сохранить")(e);
    }
  };

  const systemShare = async () => {
    try {
      const file = new File([await blob()], "LocalDrop-QR.png", { type: "image/png" });
      await navigator.share({ files: [file], text: url });
    } catch (e) {
      if ((e as Error)?.name !== "AbortError") fail("Не удалось поделиться")(e);
    }
  };

  return (
    <div className="overlay" onClick={onClose}>
      <div className="card glass strong share-card" onClick={(e) => e.stopPropagation()}>
        <h2 className="share-card-header">Поделиться ссылкой</h2>
        {err ? <p className="err">{err}</p> : img ? <img className="qr-preview" src={img} alt="QR-код ссылки" /> : <p className="hint">Готовим изображение…</p>}
        <div className="actions">
          <h2 className="actions-header">Поделиться ссылкой</h2>
          <button className="pill primary lg" disabled={!img} onClick={copyImage}>Копировать изображение</button>
          <button className="pill lg" disabled={!img} onClick={saveImage}>Сохранить как PNG…</button>
          {canShare && <button className="pill lg" disabled={!img} onClick={systemShare}>Отправить через систему…</button>}
          <button className="pill lg" onClick={() =>
            navigator.clipboard.writeText(url).then(() => done("Ссылка скопирована"))}>Копировать ссылку</button>
          <button className="pill lg" onClick={onClose}>Закрыть</button>
        </div>
      </div>
    </div>
  );
}

function Card({ s, onClose, onShare, notify }: { s: WebSession; onClose: () => void; onShare: (url: string) => void; notify: Notify }) {
  const [hi, setHi] = useState(0);
  const [svg, setSvg] = useState("");
  const [now, setNow] = useState(Date.now());
  const [showPin, setShowPin] = useState(false);

  const host = s.hosts[Math.min(hi, s.hosts.length - 1)];
  const url = host ? `http://${host}:${s.port}/s/${s.id}` : "";

  useEffect(() => {
    if (!url) return setSvg("");
    let alive = true;
    invoke<string>("qr_svg", { text: url }).then((v) => alive && setSvg(v)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [url]);

  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  const left = Math.max(0, Math.ceil((s.expiresAt - now) / 1000));
  const stats =
    s.kind === "share"
      ? `Скачано: ${s.downloads}` + (s.maxDownloads ? ` • лимит ${s.maxDownloads} на файл` : "")
      : `Получено файлов: ${s.uploads} • ${human(s.receivedBytes)}` +
        (s.maxBytes ? ` из ${human(s.maxBytes)}` : "");

  return (
    <div className="wcard group glass">
      <div className="qr" dangerouslySetInnerHTML={{ __html: svg }} />
      <div className="wmain">
        <div className="wtitle">{s.kind === "share" ? <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m5 12 7-7 7 7"/><path d="M12 19V5"/></svg> : <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 5v14"/><path d="m19 12-7 7-7-7"/></svg>}{s.title}</div>
        <small className="hint">
          {s.kind === "share" ? `${s.files} • ${human(s.totalSize)}` : "Принимает файлы из браузера"}
        </small>
        {url ? (
          <button
            className="url"
            title="Копировать ссылку"
            onClick={() =>
              navigator.clipboard.writeText(url).then(() => notify({ kind: "success", title: "Ссылка скопирована" }, 1500))
            }
          >
            {url}
          </button>
        ) : (
          <small className="err">Нет подключения к локальной сети</small>
        )}
        {s.pin && (
          <small className="hint">
            PIN-код: <b className="pinval">{showPin ? s.pin : "•".repeat(s.pin.length)}</b>
            <button className="link" onClick={() => setShowPin((v) => !v)}>{showPin ? "Скрыть" : "Показать"}</button>
          </small>
        )}
        {s.hosts.length > 1 && (
          <label className="hint">
            Если не открывается, смените адрес:{" "}
            <select className="input sel" value={hi} onChange={(e) => setHi(Number(e.target.value))}>
              {s.hosts.map((h, i) => <option key={h} value={i}>{h}</option>)}
            </select>
          </label>
        )}
        <small className="hint">{stats}</small>
        <div className="wfoot">
          <span className={"hint" + (left < 60 ? " low" : "")}>Осталось {clock(left)}</span>
          <span className="wbtns">
            {url && <button className="pill" onClick={() => onShare(url)}>Поделиться</button>}
            <button className="pill danger" onClick={onClose}>Закрыть сейчас</button>
          </span>
        </div>
      </div>
    </div>
  );
}

export default function WebView({ notify }: { notify: Notify }) {
  const [sessions, setSessions] = useState<WebSession[] | null>(null);
  const [ttl, setTtl] = useState(15);
  const [limit, setLimit] = useState<number | null>(3);
  const [size, setSize] = useState<number | null>(10240);
  const [pinLen, setPinLen] = useState(0);
  const [padResolve, setPadResolve] = useState<((v: string | null) => void) | null>(null);
  const [sharing, setSharing] = useState<{ id: string; url: string } | null>(null);
  const sharedSession = sessions?.find((x) => x.id === sharing?.id);

  const load = useCallback(() => {
    invoke<WebSession[]>("web_sessions").then(setSessions).catch(() => {});
  }, []);

  useEffect(() => {
    load();
    let alive = true;
    let un: (() => void) | undefined;
    listen("web-event", load).then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
    };
  }, [load]);

  const fail = (e: unknown) =>
    notify({ kind: "error", title: "Не удалось создать ссылку", text: String(e) }, 7000);

  const askPin = () => new Promise<string | null>((resolve) => setPadResolve(() => resolve));

  const finishPad = (pin: string | null) => {
    padResolve?.(pin);
    setPadResolve(null);
  };

  const share = async () => {
    const r = await open({ multiple: true });
    const paths = Array.isArray(r) ? r : r ? [r] : [];
    if (!paths.length) return;
    let pin: string | null = null;
    if (pinLen) {
      pin = await askPin();
      if (pin === null) return;
    }
    try {
      await invoke("web_share", { paths, ttlMinutes: ttl, maxDownloads: limit, pin });
      load();
    } catch (e) {
      fail(e);
    }
  };

  const receive = async () => {
    let pin: string | null = null;
    if (pinLen) {
      pin = await askPin();
      if (pin === null) return;
    }
    try {
      await invoke("web_receive", { ttlMinutes: ttl, maxMb: size, pin });
      load();
    } catch (e) {
      fail(e);
    }
  };

  const close = async (id: string) => {
    await invoke("web_close", { id }).catch(() => {});
    load();
  };

  return (
    <section className="page">
      <h2>Ссылки</h2>
      <p className="sub">
        Для устройств без LocalDrop. Откройте ссылку в браузере или отсканируйте QR-код камерой телефона.
        Передача идёт напрямую по вашей сети, но <b>не шифруется</b>, в том числе и PIN-код: он защищает
        от случайного доступа, а не от перехвата трафика. Создавайте ссылки только в доверенной сети.
      </p>

      <div className="group glass">
        <div className="srow">
          <div className="grow"><div className="label">Срок действия ссылки</div></div>
          <Seg value={ttl} options={TTLS} onChange={setTtl} />
        </div>
        <div className="srow">
          <div className="grow">
            <div className="label">PIN-код</div>
            <div className="value">Откроется только после ввода кода. 5 ошибок подряд блокируют ввод</div>
          </div>
          <Seg value={pinLen} options={PINS} onChange={setPinLen} />
        </div>
        <div className="srow">
          <div className="grow">
            <div className="label">Скачиваний каждого файла</div>
            <div className="value">Для «Поделиться файлами»</div>
          </div>
          <Seg value={limit} options={LIMITS} onChange={setLimit} />
        </div>
        <div className="srow">
          <div className="grow">
            <div className="label">Максимальный размер</div>
            <div className="value">Для «Принять из браузера»</div>
          </div>
          <Seg value={size} options={SIZES} onChange={setSize} />
        </div>
        <div className="srow">
          <button className="pill primary lg" onClick={share}><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 3v12"/><path d="m17 8-5-5-5 5"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/></svg>Поделиться файлами…</button>
          <button className="pill lg" onClick={receive}><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/></svg>Принять из браузера</button>
        </div>
      </div>

      <div className="day">Активные ссылки</div>
      {sessions && sessions.length === 0 ? (
        <p className="empty-page">Активных ссылок нет.</p>
      ) : (
        sessions?.map((s) => <Card key={s.id} s={s} onClose={() => close(s.id)} notify={notify} onShare={(url) => setSharing({ id: s.id, url })} />)
      )}

      {padResolve && (
        <PinPad
          length={pinLen}
          title="Задайте PIN-код для ссылки"
          onDone={finishPad}
          onCancel={() => finishPad(null)}
        />
      )}
      {sharing && sharedSession && (
        <ShareDialog s={sharedSession} url={sharing.url} notify={notify} onClose={() => setSharing(null)} />
      )}
    </section>
  );
}