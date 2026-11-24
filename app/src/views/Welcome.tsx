import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IS_MAC } from "../components/TitleBar";
import type { SettingsData } from "../types";

const KEY = "ld-welcome";
export const welcomeSeen = () => {
  try { return localStorage.getItem(KEY) === "1"; } catch { return true; }
};
export const markWelcome = () => {
  try { localStorage.setItem(KEY, "1"); } catch { }
};

type Mode = "everyone" | "trustedOnly" | "off";
const MODES: [Mode, string, string][] = [
  ["everyone", "Все", "Неизвестные устройства спросят подтверждение у вас."],
  ["trustedOnly", "Только доверенные", "Остальным отказ приходит автоматически."],
  ["off", "Никто", "Ничего не принимать. Отправлять можно по-прежнему."],
];
const LAST = 3;
const isWin = /Win/i.test(navigator.platform);

function RadarArt() {
  const peers: [string, number, string][] = [["M", 0, "#FF9F0A"], ["P", 90, "#30D158"], ["L", 180, "#B07CFF"], ["T", 270, "#5AC8FA"]];
  return (
    <div className="wl-radar">
      <i className="r1" /><i className="r2" /><i className="r3" />
      <span className="wl-me">Я</span>
      <div className="wl-orbit">
        {peers.map(([l, a, c]) => (
          <span key={l} className="wl-peer" style={{ "--a": `${a}deg`, background: c } as CSSProperties}><b>{l}</b></span>
        ))}
      </div>
    </div>
  );
}

function CodeArt() {
  return (
    <div className="wl-code glass">
      <small>Код подтверждения</small>
      <div className="wl-digits">
        {"4827 1963".split("").map((ch, i) => (
          <span key={i} style={{ animationDelay: `${0.25 + i * 0.07}s` }}>{ch}</span>
        ))}
      </div>
      <small>Одинаковый код на обоих экранах: можно принимать</small>
    </div>
  );
}

export default function Welcome({ onClose }: { onClose: () => void }) {
  const [page, setPage] = useState(0);
  const [dir, setDir] = useState<1 | -1>(1);
  const [name, setName] = useState("");
  const [orig, setOrig] = useState("");
  const [mode, setMode] = useState<Mode>("everyone");
  const [net, setNet] = useState<string | null>(null);

  useEffect(() => {
    invoke<SettingsData>("get_settings")
      .then((s) => { setName(s.deviceName); setOrig(s.deviceName); setMode(s.visibility); })
      .catch(() => {});
  }, []);

  const go = (n: number) => {
    if (n < 0 || n > LAST) return;
    setDir(n > page ? 1 : -1);
    setPage(n);
  };
  const skip = () => { markWelcome(); onClose(); };
  const finish = async () => {
    try {
      const n = name.trim();
      if (n && n !== orig) await invoke("set_device_name", { name: n });
      await invoke("set_visibility", { mode });
    } catch { /* can be changed later */ }
    markWelcome();
    onClose();
  };
  const next = () => (page < LAST ? go(page + 1) : finish());

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = (e.target as HTMLElement | null)?.tagName === "INPUT";
      if (e.key === "Escape") skip();
      else if (e.key === "Enter") { e.preventDefault(); next(); }
      else if (!typing && e.key === "ArrowRight") next();
      else if (!typing && e.key === "ArrowLeft") go(page - 1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const checkNet = async () => {
    setNet("Проверяем…");
    try {
      const s = await invoke<{ problem: string | null }>("net_status");
      setNet(
        s.problem === "noLan" ? "Нет подключения к локальной сети. Подключитесь к Wi-Fi или Ethernet."
        : s.problem === "noDiscovery" ? "Обнаружение может быть заблокировано. Проверьте разрешения выше."
        : "Всё в порядке: сеть доступна.",
      );
    } catch {
      setNet("Не удалось проверить.");
    }
  };

  const tip = IS_MAC
    ? ["При первом запуске macOS спросит доступ к локальной сети. Нажмите «Разрешить».", "Позже: Системные настройки → Конфиденциальность и безопасность → Локальная сеть."]
    : isWin
      ? ["Windows спросит про брандмауэр. Отметьте «Частные сети» и нажмите «Разрешить доступ».", "Без этого другие устройства не увидят этот компьютер."]
      : ["Если включён брандмауэр, откройте порты: 53317/udp, 53318/tcp и 5353/udp.", "Например: sudo ufw allow 53317/udp"];

  return (
    <div className="wl" role="dialog" aria-label="Добро пожаловать в LocalDrop">
      <div className="wl-header" data-tauri-drag-region>
        <div className="wl-logo">
            <svg className="logo" viewBox="0 0 24 24" aria-hidden>
            <circle cx="12" cy="12" r="3" fill="currentColor" />
            <circle cx="12" cy="12" r="7" fill="none" stroke="currentColor" strokeWidth="1.6" opacity=".6" />
            <circle cx="12" cy="12" r="10.5" fill="none" stroke="currentColor" strokeWidth="1.4" opacity=".3" />
            </svg>
            <span className="brand-name" data-tauri-drag-region>LocalDrop</span>
        </div>
        <button className="link wl-skip" onClick={skip}>Пропустить</button>
      </div>
      <div className="wl-card">
        <div className={"wl-page " + (dir > 0 ? "fwd" : "back")} key={page}>
          {page === 0 && (<>
            <RadarArt />
            <h2>Файлы между вашими устройствами</h2>
            <p>Отправляйте файлы, папки и текст на устройства в той же сети. Без облака, аккаунтов и интернета.</p>
          </>)}
          {page === 1 && (<>
            <CodeArt />
            <h2>Безопасно и под вашим контролем</h2>
            <p>Передачи шифруются. Для нового устройства на обоих экранах показывается один код: сверьте его перед приёмом.</p>
          </>)}
          {page === 2 && (<>
            <h2>Разрешите доступ к сети</h2>
            <div className="wl-tip glass">
              <b>{tip[0]}</b>
              <small>{tip[1]}</small>
            </div>
            <button className="pill" onClick={checkNet}>Проверить сеть</button>
            {net && <p className="wl-net">{net}</p>}
          </>)}
          {page === 3 && (<>
            <h2>Последний штрих</h2>
            <label className="wl-field">
              <span>Имя этого устройства</span>
              <input className="input" value={name} maxLength={40} onChange={(e) => setName(e.target.value)} />
            </label>
            <div className="wl-modes">
              {MODES.map(([k, t, d]) => (
                <button key={k} className={"opt" + (mode === k ? " on" : "")} onClick={() => setMode(k)}>
                  <span className="radio" /><span><b>{t}</b><small>{d}</small></span>
                </button>
              ))}
            </div>
          </>)}
        </div>

        <div className="wl-foot">
          <div className="wl-dots">
            {Array.from({ length: LAST + 1 }, (_, i) => (
              <i key={i} className={i === page ? "on" : ""} onClick={() => go(i)} />
            ))}
          </div>
          <div className="wl-btns">
            {page > 0 && <button className="pill" onClick={() => go(page - 1)}>Назад</button>}
            <button className="pill primary lg" onClick={next}>{page < LAST ? "Далее" : "Начать"}</button>
          </div>
        </div>
      </div>
    </div>
  );
}