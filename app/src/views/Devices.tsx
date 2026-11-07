import { useCallback, useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { hue, initial } from "../format";
import type { Toast } from "../useLocalDrop";
import type { DeviceInfo, Peer } from "../types";

interface Props {
  peers: Peer[];
  notify: (t: Omit<Toast, "id">, ttl?: number) => void;
}

export default function DevicesView({ peers, notify }: Props) {
  const [devices, setDevices] = useState<DeviceInfo[] | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const load = useCallback(async () => {
    try {
      setDevices(await invoke<DeviceInfo[]>("list_devices"));
    } catch (e) {
      notify({ kind: "error", title: "Не удалось загрузить устройства", text: String(e) });
    }
  }, [notify]);

  useEffect(() => {
    load();
  }, [load]);

  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      notify({ kind: "error", title: "Ошибка", text: String(e) });
    }
    load();
  };

  const online = (d: DeviceInfo) => peers.some((p) => p.fp === d.fingerprint);

  const saveName = (d: DeviceInfo) => {
    const name = draft.trim();
    setEditing(null);
    if (name && name !== d.name) act(() => invoke("rename_device", { fingerprint: d.fingerprint, name }));
  };

  const forget = async (d: DeviceInfo) => {
    const ok = await ask(
      `Забыть «${d.name}»? При следующей передаче устройство снова потребует подтверждения.`,
      { title: "Забыть устройство", kind: "warning" },
    );
    if (ok) act(() => invoke("forget_device", { prefix: d.fingerprint }));
  };

  const sorted = [...(devices ?? [])].sort(
    (a, b) => Number(b.trusted) - Number(a.trusted) || a.name.localeCompare(b.name),
  );

  return (
    <section className="page">
      <h2>Устройства</h2>
      <p className="sub">
        Доверенные устройства могут отправлять вам файлы без подтверждения. Устройство попадает в список
        после первой успешной передачи.
      </p>

      {devices && sorted.length === 0 ? (
        <p className="empty-page">Пока нет известных устройств.<br />Они появятся здесь после первой передачи.</p>
      ) : (
        <div className="group glass">
          <ul className="list">
            {sorted.map((d) => (
              <li className="dev-row" key={d.fingerprint}>
                <span className="avatar sm" style={{ "--h": hue(d.name) } as CSSProperties}>
                  {initial(d.name)}
                </span>
                <div className="dev-main">
                  {editing === d.fingerprint ? (
                    <input
                      className="input"
                      autoFocus
                      value={draft}
                      maxLength={40}
                      onChange={(e) => setDraft(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") saveName(d);
                        if (e.key === "Escape") setEditing(null);
                      }}
                      onBlur={() => setEditing(null)}
                    />
                  ) : (
                    <span className="dev-name">{d.name}</span>
                  )}
                  <small className="hint">
                    <i className={"st" + (online(d) ? " on" : "")} />
                    {online(d) ? "В сети" : "Не в сети"} • {d.fingerprint.slice(0, 16)}
                  </small>
                </div>
                <span className="trust">
                  Доверять
                  <button
                    role="switch"
                    aria-checked={d.trusted}
                    className={"switch" + (d.trusted ? " on" : "")}
                    onClick={() =>
                      act(() => invoke("set_device_trusted", { fingerprint: d.fingerprint, trusted: !d.trusted }))
                    }
                  />
                </span>
                <button
                  className="icon-btn"
                  title="Переименовать"
                  onClick={() => {
                    setEditing(d.fingerprint);
                    setDraft(d.name);
                  }}
                >
                  ✎
                </button>
                <button className="icon-btn danger" title="Забыть" onClick={() => forget(d)}>✕</button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}