import { useEffect, useRef, useState } from "react";

interface Props {
  length: number;
  title: string;
  onDone: (pin: string) => void;
  onCancel: () => void;
}

export default function PinPad({ length, title, onDone, onCancel }: Props) {
  const [val, setVal] = useState("");
  const valRef = useRef("");
  const done = useRef(false);

  const set = (v: string) => {
    valRef.current = v;
    setVal(v);
  };

  const press = (k: string) => {
    if (done.current) return;
    if (k === "del") return set(valRef.current.slice(0, -1));
    if (valRef.current.length >= length) return;
    const v = valRef.current + k;
    set(v);
    if (v.length === length) {
      done.current = true;
      setTimeout(() => onDone(v), 150);
    }
  };

  const random = () => {
    if (done.current) return;
    done.current = true;
    const a = new Uint32Array(length);
    crypto.getRandomValues(a);
    onDone([...a].map((x) => x % 10).join(""));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^[0-9]$/.test(e.key)) press(e.key);
      else if (e.key === "Backspace") press("del");
      else if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="overlay" onClick={onCancel}>
      <div className="card glass strong pin-card" onClick={(e) => e.stopPropagation()}>
        <h2>{title}</h2>
        <div className="pin-dots">
          {Array.from({ length }, (_, i) => <i key={i} className={i < val.length ? "on" : ""} />)}
        </div>
        <div className="pin-grid">
          {["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((k) => (
            <button key={k} className="pin-key" onClick={() => press(k)}>{k}</button>
          ))}
          <span />
          <button className="pin-key" onClick={() => press("0")}>0</button>
          <button className="pin-key ghost" onClick={() => press("del")} aria-label="Стереть">
            <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 5a2 2 0 0 0-1.344.519l-6.328 5.74a1 1 0 0 0 0 1.481l6.328 5.741A2 2 0 0 0 10 19h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2z"/><path d="m12 9 6 6"/><path d="m18 9-6 6"/></svg>
          </button>
        </div>
        <div className="actions">
          <button className="pill lg" onClick={random}>Случайный код</button>
          <button className="pill lg" onClick={onCancel}>Отмена</button>
        </div>
      </div>
    </div>
  );
}