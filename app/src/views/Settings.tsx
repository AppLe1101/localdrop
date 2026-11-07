import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import type { Toast } from "../useLocalDrop";
import type { MyInfo, SettingsData, Visibility } from "../types";
import { getTheme, setTheme, type Theme } from "../theme";
import { getVersion } from "@tauri-apps/api/app";

const MODES: { key: Visibility; title: string; text: string }[] = [
  {
    key: "everyone",
    title: "Все",
    text: "Принимать передачи от любых устройств в сети. Неизвестные устройства потребуют вашего подтверждения.",
  },
  {
    key: "trustedOnly",
    title: "Только доверенные",
    text: "Всё, кроме доверенных устройств, отклоняется автоматически. Другие устройства по-прежнему видят вас в сети.",
  },
  {
    key: "off",
    title: "Выключено",
    text: "Ничего не принимать и скрыть устройство из списков. Отправлять файлы можно по-прежнему.",
  },
];

interface Props {
  info: MyInfo | null;
  notify: (t: Omit<Toast, "id">, ttl?: number) => void;
  onChanged: () => void;
}

const DEFAULT_HOTKEY = "CommandOrControl+Alt+D";
const pretty = (a: string) =>
  a ? a.replace("CommandOrControl", "Ctrl").replace(/Key([A-Z])$/, "$1").replace(/Digit(\d)$/, "$1") : "Отключено";
function toAccel(e: KeyboardEvent): string | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  return mods.length ? [...mods, e.code].join("+") : "";
}

export default function SettingsView({ info, notify, onChanged }: Props) {
  const [data, setData] = useState<SettingsData | null>(null);
  const [name, setName] = useState("");
  const [capturing, setCapturing] = useState(false);
  const [theme, setThemeState] = useState<Theme>(getTheme());
  const [ver, setVer] = useState("");
  useEffect(() => { getVersion().then(setVer).catch(() => {}); }, []);

  useEffect(() => {
    let alive = true;
    let un: (() => void) | undefined;
    invoke<SettingsData>("get_settings").then((s) => {
      setData(s);
      setName(s.deviceName);
    });
    listen("info-changed", () => invoke<SettingsData>("get_settings").then(setData)).then((u) =>
      alive ? (un = u) : u(),
    );
    return () => {
      alive = false;
      un?.();
    };
  }, []);

  const fail = (title: string) => (e: unknown) => notify({ kind: "error", title, text: String(e) }, 6000);
  const saved = () => notify({ kind: "success", title: "Сохранено" }, 2000);

  const saveName = async () => {
    try {
      const i = await invoke<MyInfo>("set_device_name", { name });
      setData((d) => d && { ...d, deviceName: i.name });
      setName(i.name);
      onChanged();
      saved();
    } catch (e) {
      fail("Не удалось сохранить имя")(e);
    }
  };

  const setMode = async (mode: Visibility) => {
    try {
      await invoke("set_visibility", { mode });
      setData((d) => d && { ...d, visibility: mode });
      onChanged();
    } catch (e) {
      fail("Не удалось изменить режим")(e);
    }
  };

  const setDir = async (path: string) => {
    try {
      const dir = await invoke<string>("set_download_dir", { path });
      setData((d) => d && { ...d, downloadDir: dir });
      saved();
    } catch (e) {
      fail("Не удалось изменить папку")(e);
    }
  };

  const chooseDir = async () => {
    const r = await open({ directory: true, multiple: false, defaultPath: data?.downloadDir });
    if (typeof r === "string") setDir(r);
  };

  const toggle = async (key: "autostart" | "quitOnClose") => {
    if (!data) return;
    const v = !data[key];
    try {
      if (key === "autostart") await invoke("set_autostart", { enabled: v });
      else await invoke("set_quit_on_close", { quit: v });
      setData((d) => d && { ...d, [key]: v });
    } catch (e) {
      fail("Не удалось изменить настройку")(e);
    }
  };

  const saveHotkey = async (accel: string) => {
    try {
      const v = await invoke<string>("set_hotkey", { accel });
      setData((d) => d && { ...d, hotkey: v });
      saved();
    } catch (e) {
      fail("Не удалось назначить сочетание")(e);
    }
  };

  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.key === "Escape") return setCapturing(false);
      const a = toAccel(e);
      if (a === null) return;
      setCapturing(false);
      if (a === "") return notify({ kind: "warn", title: "Нужен модификатор", text: "Добавьте Ctrl, Alt или Shift" }, 3000);
      saveHotkey(a);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing]);

  if (!data) return <section className="page" />;

  return (
    <section className="page">
      <h2>Настройки</h2>

      <div className="group glass">
        <div className="srow">
          <div className="grow">
            <div className="label">Имя устройства</div>
            <div className="value">Так вас видят другие устройства</div>
          </div>
          <input
            className="input"
            value={name}
            maxLength={40}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && name.trim() !== data.deviceName && saveName()}
          />
          <button className="pill primary" disabled={!name.trim() || name.trim() === data.deviceName} onClick={saveName}>
            Сохранить
          </button>
        </div>
      </div>

      <div className="day">Кто может отправлять вам файлы</div>
      <div className="group glass">
        <div className="opts">
          {MODES.map((m) => (
            <button
              key={m.key}
              className={"opt" + (data.visibility === m.key ? " on" : "")}
              onClick={() => setMode(m.key)}
            >
              <span className="radio" />
              <span>
                <b>{m.title}</b>
                <small>{m.text}</small>
              </span>
            </button>
          ))}
        </div>
      </div>

      <div className="day">Приём файлов</div>
      <div className="group glass">
        <div className="srow">
          <div className="grow">
            <div className="label">Папка загрузок</div>
            <div className="value">{data.downloadDir}</div>
          </div>
          <button className="pill" onClick={chooseDir}>Изменить…</button>
          <button className="pill" onClick={() => setDir("")}>По умолчанию</button>
        </div>
      </div>

      <div className="day">Drop Zone</div>
      <div className="group glass">
        <div className="srow">
          <div className="grow">
            <div className="label">Горячая клавиша</div>
            <div className="value">Открывает и закрывает Drop Zone из любого приложения</div>
          </div>
          <kbd className="kbd">{capturing ? "Нажмите сочетание…" : pretty(data.hotkey)}</kbd>
          <button className="pill" onClick={() => setCapturing((v) => !v)}>{capturing ? "Отмена" : "Изменить"}</button>
          <button className="pill" onClick={() => saveHotkey(DEFAULT_HOTKEY)}>По умолчанию</button>
          <button className="pill" onClick={() => saveHotkey("")}>Отключить</button>
        </div>
      </div>

      <div className="day">Оформление</div>
      <div className="group glass">
        <div className="srow">
          <div className="grow">
            <div className="label">Тема</div>
            <div className="value">«Системная» следует настройке Windows или macOS</div>
          </div>
          <div className="seg">
            {([["system", "Системная"], ["light", "Светлая"], ["dark", "Тёмная"]] as [Theme, string][]).map(([k, l]) => (
              <button key={k} className={theme === k ? "on" : ""} onClick={() => { setTheme(k); setThemeState(k); }}>
                {l}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="day">Запуск и фон</div>
      <div className="group glass">
        <div className="srow">
          <div className="grow">
            <div className="label">Запускать при входе в систему</div>
            <div className="value">Приложение стартует свёрнутым в трей</div>
          </div>
          <button role="switch" aria-checked={data.autostart}
            className={"switch" + (data.autostart ? " on" : "")} onClick={() => toggle("autostart")} />
        </div>
        <div className="srow">
          <div className="grow">
            <div className="label">Выходить при закрытии окна</div>
            <div className="value">Иначе приложение остаётся в трее и продолжает принимать файлы</div>
          </div>
          <button role="switch" aria-checked={data.quitOnClose}
            className={"switch" + (data.quitOnClose ? " on" : "")} onClick={() => toggle("quitOnClose")} />
        </div>
      </div>

      <div className="day">О устройстве</div>
      <div className="group glass">
        <div className="srow">
          <div className="grow">
            <div className="label">Отпечаток</div>
            <div className="value">Сверяйте его при добавлении устройств</div>
          </div>
          <code className="value">{info?.fpShort}</code>
        </div>
        <div className="srow">
          <div className="grow"><div className="label">Версия LocalDrop</div></div>
          <code className="value">{ver}</code>
        </div>
      </div>
    </section>
  );
}